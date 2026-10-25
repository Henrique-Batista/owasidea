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
    let port = 18081u16;
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

    // health
    let r = client.get(format!("{}/health", base)).send().await.unwrap();
    assert_eq!(r.text().await.unwrap(), "ok");

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

    // complete + cache
    let r = client
        .post(format!("{}/plugin/notes/complete", base))
        .json(&json!({"prompt": "int-test"}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let r = client
        .post(format!("{}/plugin/notes/complete", base))
        .json(&json!({"prompt": "int-test"}))
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

    // overview
    let r = client.get(format!("{}/overview", base)).send().await.unwrap();
    assert!(r.status().is_success());

    // metrics
    let r = client.get(format!("{}/metrics", base)).send().await.unwrap();
    assert!(r.status().is_success());
    let text = r.text().await.unwrap();
    assert!(text.contains("owasidea_requests_total"));

    // tampering: remove host imports from notes manifest, reload should fail
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
        std::fs::write(&manifest_path, original).unwrap();
        let r = client
            .post(format!("{}/plugin/notes/reload", base))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }
}
