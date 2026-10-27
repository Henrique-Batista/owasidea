use assert_cmd::Command;

#[test]
fn doctor_json() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    cmd.args(["doctor", "--json"]);
    // doctor may exit non-zero if a tool missing; just ensure runs
    let _ = cmd.output().unwrap();
}

#[test]
fn inspect_missing() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    cmd.args(["inspect", "nope", "--manifest-dir", "manifests"]);
    cmd.assert().failure();
}

#[test]
fn bundle_help_documents_host_lib_isolation() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    let output = cmd.args(["bundle", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--isolate-host-libs"));
    assert!(stdout.contains("--mask-path"));
    assert!(stdout.contains("--profile"));
}

#[test]
fn bundle_rejects_unknown_profile() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    cmd.args(["bundle", "--no-build", "--profile", "qa"]);
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("unknown bundle profile"));
}

#[test]
fn ui_plugin_scaffold_builds_a_manifest_and_static_bundle() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-ui-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();

    let mut scaffold = Command::cargo_bin("owasidea").unwrap();
    scaffold
        .args(["plugin", "new", "demo-ui", "--lang", "yew", "--type", "ui"])
        .current_dir(&root)
        .assert()
        .success();
    std::fs::write(
        root.join("wasm-app.toml"),
        "[app]\nversion = \"1.2.3\"\n\n[plugins.demo-ui]\npath = \"plugins/demo-ui\"\nlanguage = \"yew\"\ntype = \"ui\"\nruntime = \"js-wasm\"\nui_entry = \"index.html\"\n",
    )
    .unwrap();

    let mut build = Command::cargo_bin("owasidea").unwrap();
    build
        .args([
            "--config",
            "wasm-app.toml",
            "--manifest-dir",
            "manifests",
            "build",
        ])
        .current_dir(&root)
        .assert()
        .success();

    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("manifests/demo-ui.manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["runtime"], "js-wasm");
    assert_eq!(manifest["metadata"]["ui"]["entry"], "index.html");
    assert!(manifest["metadata"]["ui"]["files"]["index.html"].is_string());
    assert!(root.join("ui/public/plugins/demo-ui/index.html").exists());

    // The digest index must ship with the assets it describes.
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("ui/public/ui-plugins.json")).unwrap())
            .unwrap();
    assert_eq!(index[0]["name"], "demo-ui");
    assert_eq!(index[0]["entry"], "plugins/demo-ui/index.html");
    assert_eq!(index[0]["sha256"], manifest["metadata"]["ui"]["sha256"]);
    assert_eq!(index[0]["files"], manifest["metadata"]["ui"]["files"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn init_scaffolds_working_ui_and_plugin_new_registers_plugin() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-init-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut init = Command::cargo_bin("owasidea").unwrap();
    init.args(["init"]).arg(&root).assert().success();
    assert!(root.join("ui/index.html").is_file());
    assert!(root.join("ui/main.ts").is_file());

    let mut scaffold = Command::cargo_bin("owasidea").unwrap();
    scaffold
        .args(["plugin", "new", "notes", "--lang", "rust"])
        .current_dir(&root)
        .assert()
        .success();
    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join("wasm-app.toml")).unwrap()).unwrap();
    assert_eq!(
        config["plugins"]["notes"]["path"].as_str(),
        Some("plugins/notes")
    );

    let mut go_scaffold = Command::cargo_bin("owasidea").unwrap();
    go_scaffold
        .args([
            "plugin",
            "new",
            "chat",
            "--lang",
            "go",
            "--type",
            "executable",
        ])
        .current_dir(&root)
        .assert()
        .success();
    let go_mod = std::fs::read_to_string(root.join("plugins/chat/go.mod")).unwrap();
    assert!(go_mod.contains("tool github.com/bytecodealliance/componentize-go"));
    assert!(go_mod.contains("require github.com/bytecodealliance/componentize-go v0.5.1"));

    let mut c_scaffold = Command::cargo_bin("owasidea").unwrap();
    c_scaffold
        .args(["plugin", "new", "native", "--lang", "cpp"])
        .current_dir(&root)
        .assert()
        .success();
    let makefile = std::fs::read_to_string(root.join("plugins/native/Makefile")).unwrap();
    assert!(makefile.contains("WASI_PREVIEW1_ADAPTER"));
    assert!(!makefile.contains("/home/"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn build_resolves_paths_from_the_config_directory() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-relative-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let app = root.join("apps/demo");
    std::fs::create_dir_all(app.join("plugins/panel/dist")).unwrap();
    std::fs::write(
        app.join("plugins/panel/dist/index.html"),
        "<!doctype html><title>panel</title>",
    )
    .unwrap();
    std::fs::write(
        app.join("wasm-app.toml"),
        "[app]\nversion = \"2.0.0\"\n\n[plugins.panel]\npath = \"plugins/panel\"\nlanguage = \"svelte\"\ntype = \"ui\"\nruntime = \"js-wasm\"\nui_entry = \"index.html\"\n",
    )
    .unwrap();

    let mut build = Command::cargo_bin("owasidea").unwrap();
    build
        .args(["--config", "apps/demo/wasm-app.toml", "build"])
        .current_dir(&root)
        .assert()
        .success();

    assert!(app.join("ui/public/plugins/panel/index.html").is_file());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(app.join("manifests/panel.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["runtime"], "js-wasm");
    assert_eq!(manifest["version"], "2.0.0");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn build_generates_and_signs_when_signing_is_configured() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-sign-build-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let app = root.join("apps/signed");
    std::fs::create_dir_all(app.join("plugins/panel/dist")).unwrap();
    std::fs::write(
        app.join("plugins/panel/dist/index.html"),
        "<!doctype html><title>signed panel</title>",
    )
    .unwrap();
    std::fs::write(
        app.join("wasm-app.toml"),
        "[app]\nversion = \"1.0.0\"\n\n[signing]\ngenerate = true\n\n[plugins.panel]\npath = \"plugins/panel\"\nlanguage = \"svelte\"\ntype = \"ui\"\nruntime = \"js-wasm\"\nui_entry = \"index.html\"\n",
    )
    .unwrap();

    let mut build = Command::cargo_bin("owasidea").unwrap();
    build
        .args(["--config", "apps/signed/wasm-app.toml", "build"])
        .current_dir(&root)
        .assert()
        .success();

    assert!(app.join("keys/owasidea.key").is_file());
    assert!(app.join("keys/owasidea.pub").is_file());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(app.join("manifests/panel.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["metadata"]["signature_format"],
        "owasidea-manifest-v1"
    );
    assert!(manifest["metadata"]["signature"]
        .as_str()
        .is_some_and(|signature| !signature.is_empty()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn registry_rejects_non_https_remote_urls() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    cmd.args([
        "registry",
        "search",
        "notes",
        "--url",
        "http://example.com/index.json",
    ])
    .assert()
    .failure()
    .stderr(predicates::str::contains("must use HTTPS"));
}

#[test]
fn signing_binds_component_and_effective_manifest() {
    use base64::Engine;
    use ring::signature::{UnparsedPublicKey, ED25519};
    use std::time::{SystemTime, UNIX_EPOCH};

    let root = std::env::temp_dir().join(format!(
        "owasidea-sign-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let manifests = root.join("manifests");
    let keys = root.join("keys");
    std::fs::create_dir_all(&manifests).unwrap();
    std::fs::write(manifests.join("artifact.wasm"), b"component bytes").unwrap();
    std::fs::write(
        manifests.join("signed.manifest.json"),
        r#"{
          "name":"signed","version":"1.0.0","language":"rust","abi":"wasi-0.2",
          "runtime":"wasi-component","type":"library","component":"artifact.wasm",
          "exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},
          "permissions":{"filesystem":[],"http":{"outbound":[],"inbound":false},"kv":null,
            "resources":{"max_memory_mb":64,"max_fuel":1000000,"timeout_ms":5000,"max_instances":1}},
          "state":{"persistent":false,"storage":"none","root":null},
          "composition":{"strategy":"build-time","entry":false},"metadata":{"sha256":"unused"}
        }"#,
    )
    .unwrap();

    let mut keygen = Command::cargo_bin("owasidea").unwrap();
    keygen
        .args(["keygen", "--out"])
        .arg(&keys)
        .assert()
        .success();
    let mut sign = Command::cargo_bin("owasidea").unwrap();
    sign.args(["sign", "signed", "--manifest-dir"])
        .arg(&manifests)
        .args(["--key"])
        .arg(keys.join("owasidea.key"))
        .assert()
        .success();

    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifests.join("signed.manifest.json")).unwrap())
            .unwrap();
    let signature = base64::engine::general_purpose::STANDARD
        .decode(manifest["metadata"]["signature"].as_str().unwrap())
        .unwrap();
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(
            std::fs::read_to_string(keys.join("owasidea.pub"))
                .unwrap()
                .trim(),
        )
        .unwrap();
    let component = std::fs::read(manifests.join("artifact.wasm")).unwrap();
    let mut unsigned = manifest.clone();
    unsigned["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("signature");
    let mut payload = b"owasidea-plugin-signature-v1\0".to_vec();
    payload.extend_from_slice(&serde_json::to_vec(&unsigned).unwrap());
    payload.push(0);
    payload.extend_from_slice(&component);
    let key = UnparsedPublicKey::new(&ED25519, &public_key);
    assert!(key.verify(&payload, &signature).is_ok());

    manifest["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("signature");
    manifest["permissions"]["network"]["outbound"] = serde_json::json!(["evil.example"]);
    let mut altered = b"owasidea-plugin-signature-v1\0".to_vec();
    altered.extend_from_slice(&serde_json::to_vec(&manifest).unwrap());
    altered.push(0);
    altered.extend_from_slice(&component);
    assert!(key.verify(&altered, &signature).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn registry_install_downloads_only_after_checksum_and_signature_validation() {
    use base64::Engine;
    use sha2::Digest;
    use std::io::{Read, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    let root = std::env::temp_dir().join(format!(
        "owasidea-registry-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let packages = root.join("packages");
    let keys = root.join("keys");
    let install = root.join("installed");
    std::fs::create_dir_all(&packages).unwrap();
    let artifact = b"signed registry component";
    std::fs::write(packages.join("artifact.wasm"), artifact).unwrap();
    let sha256 = format!("{:x}", sha2::Sha256::digest(artifact));
    std::fs::write(
        packages.join("registry-demo.manifest.json"),
        format!(
            r#"{{
              "name":"registry-demo","version":"1.0.0","language":"rust","abi":"wasi-0.2",
              "runtime":"wasi-component","type":"library","component":"artifact.wasm",
              "exports":[],"imports":{{"wasi":[],"host":[],"plugins":[]}},
              "permissions":{{"filesystem":[],"http":{{"outbound":[],"inbound":false}},"network":{{"outbound":[],"inbound":[]}},"kv":null,
                "resources":{{"max_memory_mb":64,"max_fuel":1000000,"timeout_ms":5000,"max_instances":1}}}},
              "state":{{"persistent":false,"storage":"none","root":null}},
              "composition":{{"strategy":"build-time","entry":false}},"metadata":{{"sha256":"{sha256}"}}
            }}"#
        ),
    )
    .unwrap();

    let mut keygen = Command::cargo_bin("owasidea").unwrap();
    keygen
        .args(["keygen", "--out"])
        .arg(&keys)
        .assert()
        .success();
    let mut sign = Command::cargo_bin("owasidea").unwrap();
    sign.args(["sign", "registry-demo", "--manifest-dir"])
        .arg(&packages)
        .args(["--key"])
        .arg(keys.join("owasidea.key"))
        .assert()
        .success();
    std::fs::write(
        root.join("index.json"),
        r#"{"version":1,"plugins":[{"name":"registry-demo","version":"1.0.0","description":"test","manifest":"packages/registry-demo.manifest.json","artifact":"packages/artifact.wasm"}]}"#,
    )
    .unwrap();

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server_root = root.clone();
    let server = std::thread::spawn(move || {
        for incoming in listener.incoming().take(3) {
            let mut stream = incoming.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
            }
            let first_line = String::from_utf8_lossy(&request)
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            let relative = first_line.split_whitespace().nth(1).unwrap_or("/");
            let path = server_root.join(relative.trim_start_matches('/'));
            let (status, body) = match std::fs::read(path) {
                Ok(body) => ("200 OK", body),
                Err(_) => ("404 Not Found", Vec::new()),
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });

    let mut install_cmd = Command::cargo_bin("owasidea").unwrap();
    install_cmd
        .args([
            "registry",
            "install",
            "registry-demo",
            "--version",
            "1.0.0",
            "--url",
        ])
        .arg(format!("http://{address}/index.json"))
        .args(["--public-key"])
        .arg(keys.join("owasidea.pub"))
        .args(["--manifest-dir"])
        .arg(install.join("manifests"))
        .assert()
        .success();
    server.join().unwrap();

    assert_eq!(
        std::fs::read(install.join("manifests/artifact.wasm")).unwrap(),
        artifact
    );
    let installed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(install.join("manifests/registry-demo.manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        installed["metadata"]["signature_format"],
        "owasidea-manifest-v1"
    );
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(
            std::fs::read_to_string(keys.join("owasidea.pub"))
                .unwrap()
                .trim(),
        )
        .unwrap();
    assert_eq!(public_key.len(), 32);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dev_help_documents_desktop_flag() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    let output = cmd.args(["dev", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--desktop"));
}

#[test]
fn call_command_success_and_http_error() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let (status, body) = if req.contains("\"function\":\"fail\"")
                || req.contains("\"function\": \"fail\"")
            {
                ("400 Bad Request", r#"{"error":"boom"}"#)
            } else {
                ("200 OK", r#"{"result":42}"#)
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });

    // success
    let mut cmd = assert_cmd::Command::cargo_bin("owasidea").unwrap();
    cmd.args([
        "--http-addr",
        &addr,
        "call",
        "core-rust.ping",
        "--args",
        "[]",
        "--json",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("42"));

    // http error
    let mut cmd = assert_cmd::Command::cargo_bin("owasidea").unwrap();
    cmd.args([
        "--http-addr",
        &addr,
        "call",
        "core-rust.fail",
        "--args",
        "[]",
    ])
    .assert()
    .failure();
}

#[test]
fn call_command_reads_args_from_stdin() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = stream.read(&mut buf);
            let body = r#"{"result":"ok"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let mut cmd = assert_cmd::Command::cargo_bin("owasidea").unwrap();
    cmd.args([
        "--http-addr",
        &addr,
        "call",
        "core-rust.ping",
        "--args",
        "-",
    ])
    .write_stdin("[7]")
    .assert()
    .success()
    .stdout(predicates::str::contains("ok"));
}

#[test]
fn verify_dist_valid_signed_component() {
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-verify-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dist = root.join("dist");
    std::fs::create_dir_all(dist.join("manifests")).unwrap();
    std::fs::create_dir_all(dist.join("plugins/demo")).unwrap();
    let component = b"component bytes";
    std::fs::write(dist.join("plugins/demo/demo.component.wasm"), component).unwrap();
    let sha = format!("{:x}", Sha256::digest(component));
    let manifest = serde_json::json!({
        "name":"demo","version":"1.0.0","language":"rust","abi":"wasi-0.2",
        "schema_version":1,"host_abi":owasidea_host::HOST_ABI,"min_host":owasidea_host::HOST_VERSION,
        "runtime":"wasi-component","type":"library","component":"../plugins/demo/demo.component.wasm",
        "exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},
        "permissions":{"filesystem":[],"http":{"outbound":[],"inbound":false},"kv":null,
            "network":{"outbound":[],"inbound":[]},
            "resources":{"max_memory_mb":64,"max_fuel":1000000,"timeout_ms":5000,"max_instances":1}},
        "state":{"persistent":false,"storage":"none","root":null},
        "composition":{"strategy":"build-time","entry":false},
        "metadata":{"author":"","license":"MIT","description":"","sha256":sha}
    });
    std::fs::write(
        dist.join("manifests/demo.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    std::fs::create_dir_all(dist.join("keys")).unwrap();
    let mut keygen = Command::cargo_bin("owasidea").unwrap();
    keygen
        .args(["keygen", "--out"])
        .arg(dist.join("keys"))
        .assert()
        .success();

    let mut sign = Command::cargo_bin("owasidea").unwrap();
    sign.args(["sign", "demo", "--manifest-dir"])
        .arg(dist.join("manifests"))
        .args(["--key"])
        .arg(dist.join("keys/owasidea.key"))
        .assert()
        .success();

    // valid
    let mut verify = Command::cargo_bin("owasidea").unwrap();
    let out = verify
        .args(["verify"])
        .arg(&dist)
        .args(["--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], true);
    assert!(report["checked"].as_u64().unwrap() >= 1);
    assert!(report["errors"].as_array().unwrap().is_empty());

    // tampered component
    std::fs::write(dist.join("plugins/demo/demo.component.wasm"), b"tampered").unwrap();
    let mut verify2 = Command::cargo_bin("owasidea").unwrap();
    verify2.args(["verify"]).arg(&dist).assert().failure();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn verify_dist_unsigned_or_missing_key_fails() {
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-verify-unsigned-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dist = root.join("dist");
    std::fs::create_dir_all(dist.join("manifests")).unwrap();
    std::fs::create_dir_all(dist.join("plugins/demo")).unwrap();
    let component = b"component bytes";
    std::fs::write(dist.join("plugins/demo/demo.component.wasm"), component).unwrap();
    let sha = format!("{:x}", Sha256::digest(component));
    let manifest = serde_json::json!({
        "name":"demo","version":"1.0.0","language":"rust","abi":"wasi-0.2",
        "schema_version":1,"host_abi":owasidea_host::HOST_ABI,"min_host":owasidea_host::HOST_VERSION,
        "runtime":"wasi-component","type":"library","component":"../plugins/demo/demo.component.wasm",
        "exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},
        "permissions":{"filesystem":[],"http":{"outbound":[],"inbound":false},"kv":null,
            "network":{"outbound":[],"inbound":[]},
            "resources":{"max_memory_mb":64,"max_fuel":1000000,"timeout_ms":5000,"max_instances":1}},
        "state":{"persistent":false,"storage":"none","root":null},
        "composition":{"strategy":"build-time","entry":false},
        "metadata":{"author":"","license":"MIT","description":"","sha256":sha}
    });
    std::fs::write(
        dist.join("manifests/demo.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let mut verify = Command::cargo_bin("owasidea").unwrap();
    verify.args(["verify"]).arg(&dist).assert().failure();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn verify_dist_ui_asset_hash_mismatch_fails() {
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-verify-ui-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dist = root.join("dist");
    std::fs::create_dir_all(dist.join("manifests")).unwrap();
    std::fs::create_dir_all(dist.join("ui/public/plugins/ui")).unwrap();
    let asset = b"<html>hi</html>";
    std::fs::write(dist.join("ui/public/plugins/ui/index.html"), asset).unwrap();
    let file_digest = format!("{:x}", Sha256::digest(asset));
    let mut files = serde_json::Map::new();
    files.insert("index.html".to_string(), serde_json::json!(file_digest));
    let bundle_digest_input: std::collections::BTreeMap<String, String> =
        [("index.html".to_string(), file_digest.clone())]
            .into_iter()
            .collect();
    // compute bundle digest same as cli
    let mut h = Sha256::new();
    for (p, d) in &bundle_digest_input {
        h.update(p.as_bytes());
        h.update([0]);
        h.update(d.as_bytes());
        h.update(b"\n");
    }
    let bundle_digest = format!("{:x}", h.finalize());
    let manifest = serde_json::json!({
        "name":"ui","version":"1.0.0","language":"yew","abi":"wasi-0.2",
        "schema_version":1,"host_abi":owasidea_host::HOST_ABI,"min_host":owasidea_host::HOST_VERSION,
        "runtime":"js-wasm","type":"ui","component":"",
        "exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},
        "permissions":{"filesystem":[],"http":{"outbound":[],"inbound":false},"kv":null,
            "network":{"outbound":[],"inbound":[]},
            "resources":{"max_memory_mb":64,"max_fuel":1000000,"timeout_ms":5000,"max_instances":1}},
        "state":{"persistent":false,"storage":"none","root":null},
        "composition":{"strategy":"build-time","entry":false},
        "metadata":{"author":"","license":"MIT","description":"","sha256":"",
            "ui":{"entry":"index.html","sha256":bundle_digest,"files":files,"bridge":[]}}
    });
    std::fs::write(
        dist.join("manifests/ui.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    std::fs::create_dir_all(dist.join("keys")).unwrap();
    let mut keygen = Command::cargo_bin("owasidea").unwrap();
    keygen
        .args(["keygen", "--out"])
        .arg(dist.join("keys"))
        .assert()
        .success();
    let mut sign = Command::cargo_bin("owasidea").unwrap();
    sign.args(["sign", "ui", "--manifest-dir"])
        .arg(dist.join("manifests"))
        .args(["--key"])
        .arg(dist.join("keys/owasidea.key"))
        .assert()
        .success();

    // valid first
    let mut verify = Command::cargo_bin("owasidea").unwrap();
    verify.args(["verify"]).arg(&dist).assert().success();

    // tamper asset
    std::fs::write(
        dist.join("ui/public/plugins/ui/index.html"),
        b"<html>tampered</html>",
    )
    .unwrap();
    let mut verify2 = Command::cargo_bin("owasidea").unwrap();
    verify2.args(["verify"]).arg(&dist).assert().failure();
    std::fs::remove_dir_all(root).unwrap();
}
