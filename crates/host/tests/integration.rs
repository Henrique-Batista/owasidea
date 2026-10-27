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
        assert_eq!(body["last_error"], serde_json::Value::Null);
        assert_eq!(body["restarts"], 0);

        // PX-08: explicit restart drops the instance and starts a new one.
        let metrics_before: String = client
            .get(format!("{}/metrics", base))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let requests_before = metric_value(
            &metrics_before,
            "owasidea_plugin_requests_total",
            "heartbeat",
        )
        .expect("heartbeat requests metric");
        let r = client
            .post(format!("{}/plugin/heartbeat/service/restart", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "restart failed: {}", r.status());
        let body: serde_json::Value = r.json().await.unwrap();
        assert_eq!(body["running"], true);
        assert_eq!(body["address"], serde_json::Value::Null);
        assert_eq!(body["last_error"], serde_json::Value::Null);
        assert_eq!(body["restarts"], 1);

        let body: serde_json::Value = client
            .get(format!("{}/plugin/heartbeat/service/status", base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body["running"], true);
        assert_eq!(body["last_error"], serde_json::Value::Null);
        assert_eq!(body["restarts"], 1);

        // Restart counts as one PX-07 operation; the status probe is another.
        let metrics_after: String = client
            .get(format!("{}/metrics", base))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            metric_value(
                &metrics_after,
                "owasidea_plugin_requests_total",
                "heartbeat"
            )
            .expect("heartbeat requests metric")
                >= requests_before + 2.0,
            "restart/status did not increment plugin metrics: {metrics_after}"
        );

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

    // PX-08: restart is only valid for stateful services.
    let r = client
        .post(format!("{}/plugin/notes/service/restart", base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::CONFLICT);

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

    // per-plugin metrics (PX-07): the generic call above hit notes.
    assert!(text.contains("# TYPE owasidea_plugin_requests_total counter"));
    let notes_requests = metric_value(&text, "owasidea_plugin_requests_total", "notes")
        .expect("notes requests metric");
    assert!(notes_requests >= 1.0, "notes requests: {notes_requests}");
    assert!(
        metric_value(&text, "owasidea_plugin_latency_ms_count", "notes")
            .expect("notes latency count")
            >= 1.0
    );
    assert!(
        metric_value(&text, "owasidea_plugin_latency_ms_sum", "notes").expect("notes latency sum")
            >= 0.0
    );
    assert!(
        metric_value(&text, "owasidea_plugin_fuel_total", "notes").expect("notes fuel") > 0.0,
        "fuel should be metered for library calls"
    );
    // Only loaded plugins are labelled; an error call bumps the error counter.
    assert!(metric_value(&text, "owasidea_plugin_requests_total", "ghost").is_none());
    let r = client
        .post(format!("{}/plugin/notes/call", base))
        .json(&json!({"function": "does-not-exist", "args": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::NOT_FOUND);
    let r = client
        .get(format!("{}/metrics", base))
        .send()
        .await
        .unwrap();
    let text = r.text().await.unwrap();
    assert!(
        metric_value(&text, "owasidea_plugin_errors_total", "notes").expect("notes errors") >= 1.0
    );
    assert!(
        metric_value(&text, "owasidea_plugin_requests_total", "notes").expect("notes requests")
            >= notes_requests + 1.0
    );
    assert!(
        metric_value(&text, "owasidea_plugin_requests_total", "chat-go").expect("chat-go requests")
            >= 1.0
    );

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

/// Copy the manifest dir and add a `service` manifest whose component has no
/// `start-service` export, to exercise the PX-08 failure path.
fn failing_service_manifests(manifests: &std::path::Path) -> Option<std::path::PathBuf> {
    let (mut failing, component) = ["notes", "chat-go"].iter().find_map(|name| {
        let raw = std::fs::read_to_string(manifests.join(format!("{name}.manifest.json"))).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let component = manifests.join(value["component"].as_str()?);
        component.exists().then_some((value, component))
    })?;
    let dir = std::env::temp_dir().join(format!("owasidea-px08-manifests-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    for entry in std::fs::read_dir(manifests).ok()? {
        let path = entry.ok()?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).ok()?;
        let mut value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        // The temp dir has no sibling `plugins/`, so pin component paths.
        if let Some(component) = value.get("component").and_then(|v| v.as_str()) {
            let absolute = manifests.join(component);
            value["component"] = json!(absolute.to_string_lossy());
        }
        std::fs::write(
            dir.join(path.file_name()?),
            serde_json::to_vec_pretty(&value).ok()?,
        )
        .ok()?;
    }
    failing["name"] = json!("failing-service");
    failing["type"] = json!("service");
    failing["component"] = json!(component.to_string_lossy());
    if let Some(metadata) = failing
        .get_mut("metadata")
        .and_then(|value| value.as_object_mut())
    {
        for key in ["signature", "signature_format", "sha256"] {
            metadata.remove(key);
        }
    }
    std::fs::write(
        dir.join("failing-service.manifest.json"),
        serde_json::to_vec_pretty(&failing).ok()?,
    )
    .ok()?;
    Some(dir)
}

#[tokio::test]
async fn restart_reports_failure_reason_and_keeps_state() {
    let Some(manifests) = manifests_dir() else {
        eprintln!("skipping: manifests not found");
        return;
    };
    let Some(temp_manifests) = failing_service_manifests(&manifests) else {
        eprintln!("skipping: notes/chat-go component not built");
        return;
    };
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve ephemeral port")
        .local_addr()
        .expect("read ephemeral port")
        .port();
    let base = format!("http://127.0.0.1:{}", port);
    let child = Command::new(env!("CARGO_BIN_EXE_owasidea-host"))
        .arg(&temp_manifests)
        .arg("--addr")
        .arg(format!("127.0.0.1:{}", port))
        .spawn()
        .expect("spawn host");
    let guard = HostGuard { child };
    wait_ready(&base).await;
    let client = reqwest::Client::new();

    let body: serde_json::Value = client
        .get(format!("{}/plugin/failing-service/service/status", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["running"], false);
    assert_eq!(body["last_error"], serde_json::Value::Null);
    assert_eq!(body["restarts"], 0);

    // The component has no start-service export: the start fails and the reason
    // is kept for the next status call.
    let r = client
        .post(format!("{}/plugin/failing-service/service/restart", base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::NOT_FOUND);
    let text = r.text().await.unwrap();
    assert!(text.contains("start-service"), "unexpected error: {text}");

    let body: serde_json::Value = client
        .get(format!("{}/plugin/failing-service/service/status", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["running"], false);
    assert!(body["last_error"]
        .as_str()
        .expect("last_error after failed start")
        .contains("start-service"));
    assert_eq!(body["restarts"], 0);

    let metrics = client
        .get(format!("{}/metrics", base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        metric_value(&metrics, "owasidea_plugin_errors_total", "failing-service"),
        Some(1.0)
    );

    drop(guard);
    let _ = std::fs::remove_dir_all(&temp_manifests);
}

/// Read the value of a labelled Prometheus sample, e.g.
/// `owasidea_plugin_requests_total{plugin="notes"} 3`.
fn metric_value(text: &str, metric: &str, plugin: &str) -> Option<f64> {
    let needle = format!("{metric}{{plugin=\"{plugin}\"}}");
    text.lines()
        .find_map(|line| line.strip_prefix(&needle)?.trim().parse::<f64>().ok())
}
