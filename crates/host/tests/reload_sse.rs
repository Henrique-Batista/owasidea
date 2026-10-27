//! FIX-04: end-to-end coverage for hot reload, the `409` guard on reloading a
//! running service, and the structured log SSE stream.
//!
//! Fixtures: the real components built for `examples/shipyard` — `core-rust`
//! (library that logs on every `ping`) and `watchtower-rust` (stateful
//! service). Their manifests are copied into a tempdir with absolute component
//! paths and stripped signatures, so the repository is never mutated. When the
//! shipyard artifacts are not built, the tests skip like the rest of
//! `crates/host/tests`.
//!
//! The host runs as a real process on an ephemeral loopback port: no external
//! network, and the SSE assertions exercise actual chunked HTTP framing.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

const LIBRARY: &str = "core-rust";
const SERVICE: &str = "watchtower-rust";
const PING_LOG: &str = "core-rust ping";

struct HostGuard {
    child: Child,
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Locate the built manifests: explicit env var, a repo-level `manifests/`, or
/// the shipyard example used by the other integration tests.
fn manifests_dir() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("OWASIDEA_MANIFESTS") {
        let path = PathBuf::from(value);
        if path.is_dir() {
            return Some(path);
        }
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..6 {
        for candidate in [
            dir.join("manifests"),
            dir.join("examples/shipyard/manifests"),
        ] {
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Copy the two manifests this test needs into `<temp>/manifests` and pin the
/// component paths absolute (the tempdir has no sibling `plugins/`). Signatures
/// are stripped because rewriting the manifest invalidates them.
fn fixture_root(source: &Path) -> Option<tempfile::TempDir> {
    let root = tempfile::tempdir().ok()?;
    let manifests = root.path().join("manifests");
    std::fs::create_dir_all(&manifests).ok()?;
    for name in [LIBRARY, SERVICE] {
        let raw = std::fs::read_to_string(source.join(format!("{name}.manifest.json"))).ok()?;
        let mut value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let component = source.join(value["component"].as_str()?);
        if !component.is_file() {
            return None;
        }
        value["component"] = json!(component.to_string_lossy());
        if let Some(metadata) = value.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            metadata.remove("signature");
            metadata.remove("signature_format");
        }
        std::fs::write(
            manifests.join(format!("{name}.manifest.json")),
            serde_json::to_vec_pretty(&value).ok()?,
        )
        .ok()?;
    }
    Some(root)
}

async fn start_host(root: &Path) -> (HostGuard, String) {
    let data = root.join("data");
    std::fs::create_dir_all(&data).expect("create data dir");
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve ephemeral port")
        .local_addr()
        .expect("read ephemeral port")
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_owasidea-host"))
        .arg(root.join("manifests"))
        .arg("--addr")
        .arg(format!("127.0.0.1:{port}"))
        .current_dir(root)
        .env("OWASIDEA_DATA_DIR", &data)
        .env_remove("OWASIDEA_PUBLIC_KEY")
        .env_remove("OWASIDEA_REQUIRE_SIGNATURES")
        .spawn()
        .expect("spawn host");
    let guard = HostGuard { child };
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if let Ok(response) = client.get(format!("{base}/health")).send().await {
            if response.status().is_success() {
                return (guard, base);
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("host did not become ready at {base}");
}

async fn call_ping(client: &reqwest::Client, base: &str) -> serde_json::Value {
    let response = client
        .post(format!("{base}/plugin/{LIBRARY}/call"))
        .json(&json!({"function": "ping", "args": []}))
        .send()
        .await
        .expect("call ping");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "call ping failed"
    );
    response.json().await.expect("call ping body")
}

async fn log_entries(client: &reqwest::Client, base: &str) -> Vec<serde_json::Value> {
    let body: serde_json::Value = client
        .get(format!("{base}/plugin/{LIBRARY}/logs"))
        .send()
        .await
        .expect("logs")
        .json()
        .await
        .expect("logs body");
    body["entries"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn reload_library_keeps_serving_and_does_not_duplicate_logs() {
    let Some(source) = manifests_dir() else {
        eprintln!("skipping: manifests not found");
        return;
    };
    let Some(root) = fixture_root(&source) else {
        eprintln!("skipping: {LIBRARY}/{SERVICE} components not built");
        return;
    };
    let (_guard, base) = start_host(root.path()).await;
    let client = reqwest::Client::new();

    assert_eq!(
        call_ping(&client, &base).await["result"],
        "shipyard core online"
    );
    let before = log_entries(&client, &base).await;
    assert_eq!(before.len(), 1, "ping logs exactly once: {before:?}");

    // Hot reload of a library plugin is allowed and re-reads the manifest.
    let response = client
        .post(format!("{base}/plugin/{LIBRARY}/reload"))
        .send()
        .await
        .expect("reload");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["reloaded"],
        LIBRARY
    );

    // The reloaded component answers the same call.
    assert_eq!(
        call_ping(&client, &base).await["result"],
        "shipyard core online"
    );

    // Reload keeps the same log buffer: history is appended, never replayed or
    // duplicated, and each entry carries its own request id.
    let after = log_entries(&client, &base).await;
    assert_eq!(after.len(), 2, "reload duplicated history: {after:?}");
    assert_eq!(after[0]["message"], PING_LOG);
    assert_eq!(after[1]["message"], PING_LOG);
    assert_eq!(after[0]["plugin"], LIBRARY);
    assert_ne!(
        after[0]["request_id"], after[1]["request_id"],
        "each call gets its own request id: {after:?}"
    );
}

#[tokio::test]
async fn reload_running_service_conflicts_until_stopped() {
    let Some(source) = manifests_dir() else {
        eprintln!("skipping: manifests not found");
        return;
    };
    let Some(root) = fixture_root(&source) else {
        eprintln!("skipping: {LIBRARY}/{SERVICE} components not built");
        return;
    };
    let (_guard, base) = start_host(root.path()).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/plugin/{SERVICE}/service/start"))
        .json(&json!({}))
        .send()
        .await
        .expect("start service");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let status: serde_json::Value = client
        .get(format!("{base}/plugin/{SERVICE}/service/status"))
        .send()
        .await
        .expect("service status")
        .json()
        .await
        .expect("service status body");
    assert_eq!(status["running"], true);

    // Hot reload while the service holds an instance is a conflict.
    let response = client
        .post(format!("{base}/plugin/{SERVICE}/reload"))
        .send()
        .await
        .expect("reload service");
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body = response.text().await.unwrap();
    assert!(body.contains("stop service"), "unexpected 409 body: {body}");

    // The rejected reload must leave the service untouched.
    let status: serde_json::Value = client
        .get(format!("{base}/plugin/{SERVICE}/service/status"))
        .send()
        .await
        .expect("service status")
        .json()
        .await
        .expect("service status body");
    assert_eq!(status["running"], true);
    assert_eq!(status["last_error"], serde_json::Value::Null);

    // Stop, reload, and start again from the reloaded component.
    let response = client
        .post(format!("{base}/plugin/{SERVICE}/service/stop"))
        .send()
        .await
        .expect("stop service");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let response = client
        .post(format!("{base}/plugin/{SERVICE}/reload"))
        .send()
        .await
        .expect("reload stopped service");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "reload after stop should succeed"
    );

    let response = client
        .post(format!("{base}/plugin/{SERVICE}/service/start"))
        .json(&json!({}))
        .send()
        .await
        .expect("restart service");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["output"],
        "watchtower armed; lifecycle instance is persistent"
    );

    let response = client
        .post(format!("{base}/plugin/{SERVICE}/service/stop"))
        .send()
        .await
        .expect("stop service");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[derive(Debug)]
struct SseEvent {
    event: Option<String>,
    id: Option<String>,
    data: String,
}

fn parse_sse(block: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut id = None;
    let mut data = String::new();
    let mut has_data = false;
    for line in block.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = Some(value.to_string()),
            "id" => id = Some(value.to_string()),
            "data" => {
                if has_data {
                    data.push('\n');
                }
                data.push_str(value);
                has_data = true;
            }
            _ => {}
        }
    }
    has_data.then_some(SseEvent { event, id, data })
}

/// Split complete SSE blocks out of the byte buffer, keeping any partial tail.
fn drain_events(buf: &mut Vec<u8>, events: &mut Vec<SseEvent>) {
    while let Some(pos) = buf.windows(2).position(|window| window == b"\n\n") {
        let block: Vec<u8> = buf.drain(..pos + 2).collect();
        if let Some(event) = parse_sse(&String::from_utf8_lossy(&block)) {
            events.push(event);
        }
    }
}

#[tokio::test]
async fn log_stream_replays_history_once_and_streams_new_entries() {
    let Some(source) = manifests_dir() else {
        eprintln!("skipping: manifests not found");
        return;
    };
    let Some(root) = fixture_root(&source) else {
        eprintln!("skipping: {LIBRARY}/{SERVICE} components not built");
        return;
    };
    let (_guard, base) = start_host(root.path()).await;
    let client = reqwest::Client::new();

    // One log entry exists before the stream opens: it must be replayed once.
    assert_eq!(
        call_ping(&client, &base).await["result"],
        "shipyard core online"
    );

    let mut response = client
        .get(format!("{base}/plugin/{LIBRARY}/logs/stream"))
        .header("accept", "text/event-stream")
        .send()
        .await
        .expect("open log stream");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content type: {content_type}"
    );

    let mut buf: Vec<u8> = Vec::new();
    let mut events: Vec<SseEvent> = Vec::new();
    let mut triggered_second = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while events.len() < 2 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for SSE log events; got {events:?}"
        );
        let chunk = tokio::time::timeout(remaining, response.chunk())
            .await
            .expect("SSE chunk timeout")
            .expect("SSE read error")
            .expect("SSE stream ended early");
        buf.extend_from_slice(&chunk);
        drain_events(&mut buf, &mut events);
        if events.len() == 1 && !triggered_second {
            // A call made after the stream opened must arrive as a new event,
            // not as a second copy of the replayed history.
            triggered_second = true;
            assert_eq!(
                call_ping(&client, &base).await["result"],
                "shipyard core online"
            );
        }
    }

    assert_eq!(events.len(), 2, "one event per log entry: {events:?}");
    let ids: Vec<u64> = events
        .iter()
        .map(|event| {
            event
                .id
                .as_deref()
                .expect("SSE event id")
                .parse()
                .expect("numeric SSE event id")
        })
        .collect();
    assert!(
        ids[0] < ids[1],
        "SSE must replay history once, then stream new ids: {ids:?}"
    );

    let mut request_ids = Vec::new();
    for event in &events {
        assert_eq!(event.event.as_deref(), Some("log"));
        let entry: serde_json::Value =
            serde_json::from_str(&event.data).expect("SSE data is structured JSON");
        assert_eq!(entry["plugin"], LIBRARY);
        assert_eq!(entry["level"], "info");
        assert_eq!(entry["message"], PING_LOG);
        assert!(entry["ts"].as_u64().unwrap_or(0) > 0, "ts: {entry:?}");
        let request_id = entry["request_id"].as_u64().expect("request_id");
        assert!(request_id > 0, "request_id: {entry:?}");
        request_ids.push(request_id);
    }
    assert_ne!(
        request_ids[0], request_ids[1],
        "both events belong to different calls: {request_ids:?}"
    );

    // Quiet window: the stream must not resend the ids it already delivered.
    let quiet_until = tokio::time::Instant::now() + Duration::from_millis(1_200);
    loop {
        let remaining = quiet_until.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, response.chunk()).await {
            Err(_) => break,
            Ok(Ok(Some(chunk))) => {
                buf.extend_from_slice(&chunk);
                let mut replayed = Vec::new();
                drain_events(&mut buf, &mut replayed);
                assert!(
                    replayed.is_empty(),
                    "SSE replayed entries it already sent: {replayed:?}"
                );
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => panic!("SSE stream error: {error}"),
        }
    }
}
