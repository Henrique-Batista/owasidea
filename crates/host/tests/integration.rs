use serde_json::json;
use std::process::{Child, Command};
use std::time::Duration;

fn manifests_dir() -> Option<std::path::PathBuf> {
    if let Ok(v) = std::env::var("OWASIDEA_MANIFESTS") {
        let p = std::path::PathBuf::from(v);
        if p.exists() {
            return Some(p);
        }
    }
    // walk up from crate dir
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..5 {
        let candidate = dir.join("manifests");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

struct HostGuard {
    child: Child,
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn wait_ready(base: &str) {
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if let Ok(resp) = client.get(format!("{}/health", base)).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("host did not become ready at {}", base);
}

#[tokio::test]
async fn integration_smoke() {
    let Some(manifests) = manifests_dir() else {
        eprintln!("skipping: manifests not found");
        return;
    };
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve ephemeral port")
        .local_addr()
        .expect("read ephemeral port")
        .port();
    let base = format!("http://127.0.0.1:{}", port);
    let child = Command::new(env!("CARGO_BIN_EXE_owasidea-host"))
        .arg(&manifests)
        .arg("--addr")
        .arg(format!("127.0.0.1:{}", port))
        .spawn()
        .expect("spawn host");
    let _guard = HostGuard { child };
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Browser access is restricted to the desktop shell and local dev UI.
    let r = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/plugin/notes/call", base),
        )
        .header("origin", "https://attacker.example")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .unwrap();
    assert!(!r.headers().contains_key("access-control-allow-origin"));
    let r = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/plugin/notes/call", base),
        )
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:5173")
    );

    // health
    let r = client.get(format!("{}/health", base)).send().await.unwrap();
    assert_eq!(r.text().await.unwrap(), "ok");
    let dev_status: serde_json::Value = client
        .get(format!("{}/dev/status", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(dev_status["active"], false);

    // plugins list
    let plugins: serde_json::Value = client
        .get(format!("{}/plugins", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<&str> = plugins
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"notes"), "notes missing");
    assert!(names.contains(&"chat-go"), "chat-go missing");
    if manifests.join("heartbeat.manifest.json").exists() {
        assert!(
            names.contains(&"heartbeat"),
            "heartbeat service missing: {names:?}"
        );
    }

    let ui_plugins: serde_json::Value = client
        .get(format!("{}/ui-plugins", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(ui_plugins.is_array());

    if names.contains(&"heartbeat") {
        let r = client
            .post(format!("{}/plugin/heartbeat/service/start", base))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let body: serde_json::Value = r.json().await.unwrap();
        assert_eq!(body["output"], "heartbeat service started");

        let r = client
            .post(format!("{}/plugin/heartbeat/service/start", base))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.json::<serde_json::Value>().await.unwrap()["output"],
            "already running"
        );

        let r = client
            .get(format!("{}/plugin/heartbeat/service/status", base))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = r.json().await.unwrap();
        assert_eq!(body["running"], true);

        let r = client
            .post(format!("{}/plugin/heartbeat/reload", base))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), reqwest::StatusCode::CONFLICT);

        let r = client
            .post(format!("{}/plugin/heartbeat/service/stop", base))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.json::<serde_json::Value>().await.unwrap()["stopped"],
            true
        );
        let r = client
            .get(format!("{}/plugin/heartbeat/service/status", base))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.json::<serde_json::Value>().await.unwrap()["running"],
            false
        );
        let r = client
            .post(format!("{}/plugin/heartbeat/service/stop", base))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.json::<serde_json::Value>().await.unwrap()["stopped"],
            false
        );

        let r = client
            .post(format!("{}/plugin/heartbeat/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let r = client
            .post(format!("{}/plugin/heartbeat/service/start", base))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let r = client
            .post(format!("{}/plugin/heartbeat/service/stop", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }

    // greet
    let r = client
        .post(format!("{}/plugin/notes/greet", base))
        .json(&json!({"name": "Integration"}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["output"]
        .as_str()
        .unwrap()
        .contains("Hello, Integration!"));

    // complete + cache is retained across a component reload.
    let reload_state_key = format!("int-test-{}", std::process::id());
    let r = client
        .post(format!("{}/plugin/notes/complete", base))
        .json(&json!({"prompt": reload_state_key}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let r = client
        .post(format!("{}/plugin/notes/complete", base))
        .json(&json!({"prompt": reload_state_key}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["output"].as_str().unwrap().starts_with("[cached]"));

    // generic call
    let r = client
        .post(format!("{}/plugin/notes/call", base))
        .json(&json!({"function": "greet", "args": ["Gen"]}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // chat-go proxy
    let r = client
        .post(format!("{}/app/chat-go", base))
        .json(&json!({"message": "hi"}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["reply"], "chat-go heard: hi");

    // overview
    let r = client
        .get(format!("{}/overview", base))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // logs stream endpoint exists (just check 200 headers)
    let r = client
        .get(format!("{}/plugin/notes/logs/stream", base))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // example plugin
    if names.contains(&"example") {
        let r = client
            .post(format!("{}/plugin/example/call", base))
            .json(&json!({"function": "greet", "args": ["Ex"]}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let r = client
            .post(format!("{}/plugin/example/call", base))
            .json(&json!({"function": "compute", "args": [4, 5]}))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = r.json().await.unwrap();
        assert_eq!(body["result"], 9);
        let r = client
            .post(format!("{}/plugin/example/call", base))
            .json(&json!({"function": "remember", "args": ["k", "v"]}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }

    // composed plugin
    if names.contains(&"composed") {
        let r = client
            .post(format!("{}/plugin/composed/call", base))
            .json(&json!({"function": "run", "args": ["Compose"]}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let body: serde_json::Value = r.json().await.unwrap();
        assert!(body["result"].as_str().unwrap().contains("Hello, Compose!"));
    }

    // cross-language composed plugin (Go client -> Rust notes)
    if names.contains(&"composed-go") {
        let r = client
            .post(format!("{}/plugin/composed-go/call", base))
            .json(&json!({"function": "run", "args": ["Cross"]}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let body: serde_json::Value = r.json().await.unwrap();
        assert!(body["result"]
            .as_str()
            .unwrap()
            .contains("composed-go client got"));
    }

    // overview
    let r = client
        .get(format!("{}/overview", base))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // metrics
    let r = client
        .get(format!("{}/metrics", base))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let text = r.text().await.unwrap();
    assert!(text.contains("owasidea_requests_total"));

    // Tampering either host or WASI imports must fail manifest validation.
    let manifest_path = manifests.join("notes.manifest.json");
    if manifest_path.exists() {
        let original = std::fs::read_to_string(&manifest_path).unwrap();
        let mut val: serde_json::Value = serde_json::from_str(&original).unwrap();
        val["imports"]["host"] = json!([]);
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_server_error() || r.status().is_client_error());
        // restore
        std::fs::write(&manifest_path, &original).unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let mut val: serde_json::Value = serde_json::from_str(&original).unwrap();
        val["imports"]["wasi"] = json!([]);
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_server_error() || r.status().is_client_error());
        std::fs::write(&manifest_path, &original).unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let r = client
            .post(format!("{}/plugin/notes/complete", base))
            .json(&json!({"prompt": reload_state_key}))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = r.json().await.unwrap();
        assert!(body["output"].as_str().unwrap().starts_with("[cached]"));

        let mut read_only: serde_json::Value = serde_json::from_str(&original).unwrap();
        read_only["permissions"]["kv"]["access"] = json!("read");
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&read_only).unwrap(),
        )
        .unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let kv_read_only_key = format!("read-only-{}", std::process::id());
        for _ in 0..2 {
            let r = client
                .post(format!("{}/plugin/notes/complete", base))
                .json(&json!({"prompt": kv_read_only_key}))
                .send()
                .await
                .unwrap();
            let body: serde_json::Value = r.json().await.unwrap();
            assert!(!body["output"].as_str().unwrap().starts_with("[cached]"));
        }
        std::fs::write(&manifest_path, &original).unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }
}

#[tokio::test]
async fn structured_logs_endpoint_and_request_id() {
    let Some(manifests) = manifests_dir() else {
        eprintln!("skipping: manifests not found");
        return;
    };
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve ephemeral port")
        .local_addr()
        .expect("read ephemeral port")
        .port();
    let base = format!("http://127.0.0.1:{}", port);
    let child = Command::new(env!("CARGO_BIN_EXE_owasidea-host"))
        .arg(&manifests)
        .arg("--addr")
        .arg(format!("127.0.0.1:{}", port))
        .spawn()
        .expect("spawn host");
    let _guard = HostGuard { child };
    wait_ready(&base).await;
    let client = reqwest::Client::new();

    // trigger a log via example plugin call (if available)
    let _ = client
        .post(format!("{}/plugin/example/call", base))
        .json(&json!({"function": "greet", "args": ["Ex"]}))
        .send()
        .await;

    let r = client
        .get(format!("{}/plugin/example/logs", base))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body.get("logs").is_some());
    assert!(body.get("entries").is_some());
    let entries = body["entries"].as_array().cloned().unwrap_or_default();
    if !entries.is_empty() {
        let e = &entries[0];
        assert!(e.get("ts").is_some());
        assert!(e.get("level").is_some());
        assert!(e.get("plugin").is_some());
        assert!(e.get("request_id").is_some());
        assert!(e.get("message").is_some());
    }
}
