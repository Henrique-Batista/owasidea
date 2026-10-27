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
    assert!(stdout.contains("--provenance"));
    assert!(stdout.contains("--no-sbom"));
}

#[test]
fn signing_plan_skips_without_secrets() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    cmd.args(["signing", "plan", "--platform", "windows", "--json"]);
    for key in [
        "WINDOWS_CERTIFICATE",
        "WINDOWS_CERTIFICATE_PASSWORD",
        "OWASIDEA_WINDOWS_SIGN_COMMAND",
        "OWASIDEA_WINDOWS_CERT_THUMBPRINT",
    ] {
        cmd.env_remove(key);
    }
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["mode"], "skip");
    assert_eq!(value["sign"], false);
    assert_eq!(value["notarize"], false);
}

#[test]
fn signing_plan_rejects_partial_windows_certificate() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    cmd.args(["signing", "plan", "--platform", "windows", "--json"])
        .env("WINDOWS_CERTIFICATE", "not-a-real-certificate")
        .env_remove("WINDOWS_CERTIFICATE_PASSWORD")
        .env_remove("OWASIDEA_WINDOWS_SIGN_COMMAND")
        .env_remove("OWASIDEA_WINDOWS_CERT_THUMBPRINT");
    let output = cmd.output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("WINDOWS_CERTIFICATE_PASSWORD"));
    // The helper must never echo the secret value.
    assert!(!stderr.contains("not-a-real-certificate"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("not-a-real-certificate"));
}

#[test]
fn signing_help_documents_require() {
    let mut cmd = Command::cargo_bin("owasidea").unwrap();
    let output = cmd.args(["signing", "plan", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--platform"));
    assert!(stdout.contains("--require"));
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
fn build_fails_when_the_runtime_descriptor_cannot_be_rendered() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-build-render-{}-{}",
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

    // A regular file where the generated descriptor directory belongs makes
    // `render_targets` fail after the plugins have been built.
    std::fs::create_dir_all(root.join("ui")).unwrap();
    std::fs::write(root.join("ui/.owasidea"), "not a directory").unwrap();

    let output = Command::cargo_bin("owasidea")
        .unwrap()
        .args([
            "--config",
            "wasm-app.toml",
            "--manifest-dir",
            "manifests",
            "build",
        ])
        .current_dir(&root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !output.status.success(),
        "build must fail when the runtime descriptor cannot be written; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("Build complete"),
        "a stale runtime descriptor must not be reported as a successful build: {stdout}"
    );
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

// FIX-03: the scaffolds must compile, not only register their files.
// Heavy by design: opt in with OWASIDEA_TEMPLATE_BUILD=1 (CI runs the same
// script in the fast job). Missing toolchains skip the test with a message.
#[test]
fn templates_build() {
    if std::env::var("OWASIDEA_TEMPLATE_BUILD").as_deref() != Ok("1") {
        eprintln!(
            "skipping templates_build: set OWASIDEA_TEMPLATE_BUILD=1 to compile scaffold templates"
        );
        return;
    }

    fn tool(program: &str, args: &[&str]) -> bool {
        std::process::Command::new(program)
            .args(args)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    let mut missing = Vec::new();
    if !tool("cargo", &["component", "--version"]) {
        missing.push("cargo-component");
    }
    if !tool("go", &["version"]) {
        missing.push("go");
    }
    if !tool("node", &["--version"]) || !tool("npm", &["--version"]) {
        // The TypeScript scaffold installs jco from its npm devDependencies.
        missing.push("node/npm (jco)");
    }
    if !missing.is_empty() {
        eprintln!(
            "skipping templates_build: missing toolchains: {}",
            missing.join(", ")
        );
        return;
    }

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = repo.join("scripts/test-templates.sh");
    let cargo = Command::cargo_bin("owasidea").unwrap();
    let bin = cargo.get_program().to_owned();
    let status = std::process::Command::new("bash")
        .arg(&script)
        .env("OWASIDEA_BIN", bin)
        .env("OWASIDEA_TEMPLATE_STRICT", "1")
        .env("OWASIDEA_TEMPLATE_LANGS", "rust,go,typescript")
        .status()
        .expect("run scripts/test-templates.sh");
    assert!(status.success(), "scripts/test-templates.sh failed");
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
        .args(["--public-key"])
        .arg(dist.join("keys/owasidea.pub"))
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
    verify2
        .args(["verify"])
        .arg(&dist)
        .args(["--public-key"])
        .arg(dist.join("keys/owasidea.pub"))
        .assert()
        .failure();
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
    let out = verify
        .args(["verify"])
        .arg(&dist)
        .args(["--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["errors"][0]["code"], "TRUST_ROOT_MISSING");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn verify_dist_rejects_key_staged_inside_dist() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-verify-unpinned-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dist = root.join("dist");
    std::fs::create_dir_all(dist.join("manifests")).unwrap();
    std::fs::create_dir_all(dist.join("trust")).unwrap();
    // base64 of b"k"
    std::fs::write(dist.join("trust/owasidea.pub"), b"aw==\n").unwrap();
    std::fs::write(
        dist.join("manifests/demo.manifest.json"),
        br#"{"name":"demo","version":"1.0.0","runtime":"wasi-component","type":"library","component":"demo.wasm","metadata":{"sha256":""}}"#,
    )
    .unwrap();

    let mut verify = Command::cargo_bin("owasidea").unwrap();
    verify.env_remove("OWASIDEA_PUBLIC_KEY");
    let out = verify
        .args(["verify"])
        .arg(&dist)
        .args(["--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    let codes: Vec<&str> = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|error| error["code"].as_str())
        .collect();
    assert!(codes.contains(&"TRUST_ROOT_UNPINNED"), "codes: {codes:?}");

    // Explicit opt-in accepts the staged key for local checks.
    let mut allowed = Command::cargo_bin("owasidea").unwrap();
    allowed.env_remove("OWASIDEA_PUBLIC_KEY");
    let out = allowed
        .args(["verify"])
        .arg(&dist)
        .args(["--allow-dist-trust", "--json"])
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let codes: Vec<&str> = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|error| error["code"].as_str())
        .collect();
    assert!(!codes.contains(&"TRUST_ROOT_UNPINNED"), "codes: {codes:?}");

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
    verify
        .args(["verify"])
        .arg(&dist)
        .args(["--public-key"])
        .arg(dist.join("keys/owasidea.pub"))
        .assert()
        .success();

    // tamper asset
    std::fs::write(
        dist.join("ui/public/plugins/ui/index.html"),
        b"<html>tampered</html>",
    )
    .unwrap();
    let mut verify2 = Command::cargo_bin("owasidea").unwrap();
    verify2
        .args(["verify"])
        .arg(&dist)
        .args(["--public-key"])
        .arg(dist.join("keys/owasidea.pub"))
        .assert()
        .failure();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn verify_dist_without_manifests_fails_instead_of_passing_vacuously() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-verify-empty-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("Shipyard_0.1.0_amd64.deb"), b"deb").unwrap();

    let mut verify = Command::cargo_bin("owasidea").unwrap();
    let out = verify
        .args(["verify"])
        .arg(&dist)
        .args(["--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["errors"][0]["code"], "NO_MANIFESTS");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn latest_generates_and_validates_tauri_updater_manifest() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-latest-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let dist = root.join("dist");
    std::fs::create_dir_all(root.join("keys")).unwrap();
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(
        root.join("keys/owasidea-updater.pub"),
        "untrusted comment: owasidea updater public key\nRWQfakekeyforunittests\n",
    )
    .unwrap();
    std::fs::write(
        root.join("wasm-app.toml"),
        r#"
[app]
name = "shipyard"
version = "1.2.3"
identifier = "dev.owasidea.shipyard"

[updater]
channel = "beta"
endpoint = "https://updates.example.com/{{channel}}/latest.json"
pubkey = "keys/owasidea-updater.pub"
notes = "beta build"
"#,
    )
    .unwrap();
    for (artifact, signature) in [
        ("Shipyard_1.2.3_amd64.AppImage", "sig-appimage"),
        ("Shipyard_1.2.3_x64-setup.exe", "sig-nsis"),
        ("Shipyard_1.2.3_amd64.deb", "sig-deb"),
    ] {
        std::fs::write(dist.join(artifact), b"artifact").unwrap();
        std::fs::write(dist.join(format!("{artifact}.sig")), signature).unwrap();
    }

    let mut latest = Command::cargo_bin("owasidea").unwrap();
    latest
        .args(["--config"])
        .arg(root.join("wasm-app.toml"))
        .args(["latest", "--artifacts"])
        .arg(&dist)
        .args(["--pub-date", "2026-10-27T00:00:00Z", "--json"])
        .assert()
        .success();

    let raw = std::fs::read_to_string(dist.join("latest.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["version"], "1.2.3");
    assert_eq!(value["channel"], "beta");
    assert_eq!(value["notes"], "beta build");
    assert_eq!(value["pub_date"], "2026-10-27T00:00:00Z");
    assert_eq!(
        value["platforms"]["linux-x86_64"]["url"],
        "https://updates.example.com/beta/Shipyard_1.2.3_amd64.AppImage"
    );
    assert_eq!(
        value["platforms"]["linux-x86_64"]["signature"],
        "sig-appimage"
    );
    assert_eq!(
        value["platforms"]["windows-x86_64"]["url"],
        "https://updates.example.com/beta/Shipyard_1.2.3_x64-setup.exe"
    );
    // .deb is not an updater target.
    assert!(value["platforms"].get("linux-x86_64-deb").is_none());

    let mut check = Command::cargo_bin("owasidea").unwrap();
    check
        .args(["--config"])
        .arg(root.join("wasm-app.toml"))
        .args(["latest", "--artifacts"])
        .arg(&dist)
        .arg("--check")
        .assert()
        .success();

    // A malformed manifest fails --check instead of silently passing.
    let mut broken = value.clone();
    broken["platforms"]["linux-x86_64"]
        .as_object_mut()
        .unwrap()
        .remove("signature");
    std::fs::write(
        dist.join("latest.json"),
        serde_json::to_string_pretty(&broken).unwrap(),
    )
    .unwrap();
    let mut check_broken = Command::cargo_bin("owasidea").unwrap();
    check_broken
        .args(["--config"])
        .arg(root.join("wasm-app.toml"))
        .args(["latest", "--artifacts"])
        .arg(&dist)
        .arg("--check")
        .assert()
        .failure();

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn latest_rejects_unconfigured_updater() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-latest-unconfigured-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("wasm-app.toml"),
        "[app]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let mut latest = Command::cargo_bin("owasidea").unwrap();
    latest
        .args(["--config"])
        .arg(root.join("wasm-app.toml"))
        .arg("latest")
        .assert()
        .failure()
        .stderr(predicates::str::contains("updater is not configured"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sbom_is_valid_deterministic_and_checkable() {
    use std::path::Path;

    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("wasm-app.toml");
    let dist = dir.path().join("dist");
    let output = dist.join("sbom.cdx.json");
    std::fs::write(&config, "[app]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(
        dir.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("package-lock.json"),
        r#"{"name":"demo","lockfileVersion":3,"packages":{"":{"name":"demo","version":"0.1.0"},"node_modules/plain":{"version":"1.2.3"}}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("go.mod"),
        "module example.com/demo\n\ngo 1.27.1\n\nrequire golang.org/x/sys v0.37.0\n",
    )
    .unwrap();

    let run = |extra: &[&str]| {
        let mut cmd = Command::cargo_bin("owasidea").unwrap();
        cmd.args(["--config"])
            .arg(&config)
            .args(["sbom", "--output"])
            .arg(&output)
            .args(extra)
            .env("SOURCE_DATE_EPOCH", "1700000000");
        cmd.output().unwrap()
    };

    let first = run(&[]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_bytes = std::fs::read(&output).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
    assert_eq!(value["bomFormat"], "CycloneDX");
    assert_eq!(value["specVersion"], "1.5");
    assert_eq!(value["metadata"]["timestamp"], "2023-11-14T22:13:20Z");
    assert_eq!(value["components"].as_array().unwrap().len(), 3);
    assert_eq!(value["metadata"]["component"]["name"], "demo");

    let second = run(&[]);
    assert!(second.status.success());
    assert_eq!(first_bytes, std::fs::read(&output).unwrap());

    let check = run(&["--check"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let mut tampered = first_bytes.clone();
    tampered.push(b' ');
    std::fs::write(&output, tampered).unwrap();
    let stale = run(&["--check"]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale"));

    std::fs::write(dist.join("app.deb"), b"deb").unwrap();
    let artifacts = dist.to_string_lossy().to_string();
    let provenance = run(&["--provenance", "--artifacts", &artifacts]);
    assert!(
        provenance.status.success(),
        "{}",
        String::from_utf8_lossy(&provenance.stderr)
    );
    let raw = std::fs::read(dist.join("provenance.intoto.json")).unwrap();
    let statement: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v1");
    assert_eq!(
        statement["predicate"]["buildDefinition"]["externalParameters"]["sourceDateEpoch"],
        1_700_000_000u64
    );
    let names: Vec<&str> = statement["subject"]
        .as_array()
        .unwrap()
        .iter()
        .map(|subject| subject["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"app.deb"));
    assert!(names.contains(&"sbom.cdx.json"));
    assert!(Path::new(&output).is_file());
}

// --- RL-07: ABI/deprecation warnings -------------------------------------

fn write_validate_fixture(
    label: &str,
    manifest: impl FnOnce(&str) -> serde_json::Value,
) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};
    let root = std::env::temp_dir().join(format!(
        "owasidea-validate-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("manifests")).unwrap();
    std::fs::create_dir_all(root.join("plugins/legacy")).unwrap();
    std::fs::write(
        root.join("wasm-app.toml"),
        "[app]\nname = \"legacy\"\nversion = \"0.1.0\"\n\n\
         [plugins.legacy]\npath = \"plugins/legacy\"\nlanguage = \"rust\"\ntype = \"library\"\n",
    )
    .unwrap();
    let component = b"legacy component bytes";
    std::fs::write(root.join("plugins/legacy/legacy.component.wasm"), component).unwrap();
    let sha = format!("{:x}", Sha256::digest(component));
    let manifest = manifest(&sha);
    std::fs::write(
        root.join("manifests/legacy.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    root
}

fn validate_fixture(root: &std::path::Path) -> std::process::Output {
    Command::cargo_bin("owasidea")
        .unwrap()
        .args(["--config"])
        .arg(root.join("wasm-app.toml"))
        .arg("validate")
        .args(["--manifest-dir"])
        .arg(root.join("manifests"))
        .output()
        .unwrap()
}

fn validate_fixture_json(root: &std::path::Path) -> std::process::Output {
    Command::cargo_bin("owasidea")
        .unwrap()
        .args(["--config"])
        .arg(root.join("wasm-app.toml"))
        .args(["validate", "--json"])
        .args(["--manifest-dir"])
        .arg(root.join("manifests"))
        .output()
        .unwrap()
}

fn base_manifest(sha: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "legacy", "version": "0.1.0", "language": "rust", "abi": "wasi-0.2",
        "runtime": "wasi-component", "type": "library",
        "component": "legacy.component.wasm",
        "exports": [], "imports": {"wasi": [], "host": [], "plugins": []},
        "permissions": {
            "filesystem": [],
            "http": {"outbound": [], "inbound": false},
            "kv": null
        },
        "state": {}, "composition": {},
        "metadata": {"sha256": sha}
    })
}

#[test]
fn validate_warns_about_legacy_manifest_without_failing() {
    let root = write_validate_fixture("legacy", base_manifest);
    let out = validate_fixture(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains("warning: legacy: ABI_LEGACY_MANIFEST"),
        "{stdout}"
    );
    assert!(stdout.contains("ABI_MISSING_HOST_FIELDS"), "{stdout}");
    assert!(stdout.contains("docs/ABI_POLICY.md"), "{stdout}");
    assert!(stdout.contains("deprecation warning(s)"), "{stdout}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_is_quiet_for_current_manifest() {
    let root = write_validate_fixture("current", |sha| {
        let mut manifest = base_manifest(sha);
        let object = manifest.as_object_mut().unwrap();
        object.insert("schema_version".to_string(), serde_json::json!(1));
        object.insert(
            "host_abi".to_string(),
            serde_json::json!(owasidea_host::HOST_ABI),
        );
        object.insert(
            "min_host".to_string(),
            serde_json::json!(owasidea_host::HOST_VERSION),
        );
        manifest
    });
    let out = validate_fixture(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(!stdout.contains("warning:"), "{stdout}");
    assert!(stdout.contains("Validation OK."), "{stdout}");
    std::fs::remove_dir_all(root).unwrap();
}

fn current_manifest(sha: &str) -> serde_json::Value {
    let mut manifest = base_manifest(sha);
    let object = manifest.as_object_mut().unwrap();
    object.insert("schema_version".to_string(), serde_json::json!(1));
    object.insert(
        "host_abi".to_string(),
        serde_json::json!(owasidea_host::HOST_ABI),
    );
    object.insert(
        "min_host".to_string(),
        serde_json::json!(owasidea_host::HOST_VERSION),
    );
    manifest
}

// --- FIX-02: `validate --json` -------------------------------------------

#[test]
fn validate_json_reports_ok_for_current_manifest() {
    let root = write_validate_fixture("json-ok", current_manifest);
    let out = validate_fixture_json(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("validate --json must emit JSON: {e}: {stdout}"));
    assert_eq!(value["ok"], true);
    assert_eq!(value["errors"], serde_json::json!([]));
    assert_eq!(value["warnings"], serde_json::json!([]));
    let plugins = value["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1, "{value}");
    assert_eq!(plugins[0]["name"], "legacy");
    assert_eq!(plugins[0]["ok"], true);
    assert_eq!(plugins[0]["errors"], serde_json::json!([]));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_json_reports_tampered_manifest_as_not_ok() {
    let root = write_validate_fixture("json-tampered", |sha| {
        // Current ABI so the only failure is the checksum drift.
        let _ = sha;
        current_manifest("deadbeef")
    });
    let out = validate_fixture_json(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{stdout}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("validate --json must emit JSON on failure: {e}: {stdout}"));
    assert_eq!(value["ok"], false);
    let errors = value["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e.as_str().unwrap_or_default().contains("CHECKSUM MISMATCH")),
        "{value}"
    );
    assert_eq!(value["plugins"][0]["ok"], false);
    assert!(
        value["plugins"][0]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap_or_default().contains("CHECKSUM MISMATCH")),
        "{value}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_json_includes_abi_warnings_without_failing() {
    let root = write_validate_fixture("json-legacy", base_manifest);
    let out = validate_fixture_json(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["ok"], true);
    let warnings = value["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap_or_default()
            .contains("ABI_LEGACY_MANIFEST")),
        "{value}"
    );
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap_or_default()
            .contains("ABI_MISSING_HOST_FIELDS")),
        "{value}"
    );
    assert!(
        value["plugins"][0]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w
                .as_str()
                .unwrap_or_default()
                .contains("ABI_LEGACY_MANIFEST")),
        "{value}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_json_reports_missing_manifest() {
    let root = write_validate_fixture("json-missing", current_manifest);
    std::fs::remove_file(root.join("manifests/legacy.manifest.json")).unwrap();
    let out = validate_fixture_json(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{stdout}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert!(
        value["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap_or_default().contains("MISSING manifest")),
        "{value}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_without_json_keeps_text_output() {
    let root = write_validate_fixture("text", current_manifest);
    let out = validate_fixture(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).is_err(),
        "text mode must not emit JSON: {stdout}"
    );
    assert!(stdout.contains("Validation OK."), "{stdout}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_text_failure_still_prints_lines_and_exits_non_zero() {
    let root = write_validate_fixture("text-bad", |sha| {
        let _ = sha;
        current_manifest("deadbeef")
    });
    let out = validate_fixture(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stdout}");
    assert!(stdout.contains("legacy: CHECKSUM MISMATCH"), "{stdout}");
    assert!(stderr.contains("validation failed"), "{stderr}");
    assert!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).is_err(),
        "text mode must not emit JSON on failure: {stdout}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_json_reports_config_failure_as_json() {
    let root = write_validate_fixture("json-config", current_manifest);
    std::fs::write(
        root.join("wasm-app.toml"),
        "[app]\nname = \"\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let out = validate_fixture_json(&root);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{stdout}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("config failure must still emit JSON: {e}: {stdout}"));
    assert_eq!(value["ok"], false);
    assert_eq!(value["plugins"], serde_json::json!([]));
    assert!(!value["errors"].as_array().unwrap().is_empty(), "{value}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn abi_policy_docs_links_resolve() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let docs = [
        "docs/ABI_POLICY.md",
        "docs/MIGRATION_0_X_TO_1_0.md",
        "docs/ABI.md",
        "CHANGELOG.md",
    ];
    let mut broken = Vec::new();
    for doc in docs {
        let path = repo.join(doc);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{doc}: {e}"));
        let dir = path.parent().unwrap();
        let mut rest = text.as_str();
        while let Some(start) = rest.find("](") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find(')') else { break };
            let target = rest[..end].trim();
            rest = &rest[end..];
            if target.is_empty() || target.starts_with('#') || target.starts_with("http") {
                continue;
            }
            let file = target.split('#').next().unwrap_or("");
            if file.is_empty() {
                continue;
            }
            if !dir.join(file).exists() {
                broken.push(format!("{doc}: {target}"));
            }
        }
    }
    assert!(broken.is_empty(), "broken doc links: {broken:?}");
}

#[test]
fn changelog_and_policy_document_1_0_abi_rules() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let changelog = std::fs::read_to_string(repo.join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("## [1.0.0]"), "{changelog}");
    assert!(changelog.contains("ABI_LEGACY_MANIFEST"), "{changelog}");
    assert!(changelog.contains("docs/ABI_POLICY.md"), "{changelog}");

    let policy = std::fs::read_to_string(repo.join("docs/ABI_POLICY.md")).unwrap();
    for needle in [
        "schema_version",
        "host_abi",
        "min_host",
        "Deprecation window",
        "Error vs warning policy",
        "ABI_LEGACY_MANIFEST",
    ] {
        assert!(policy.contains(needle), "ABI_POLICY.md missing {needle}");
    }

    let migration = std::fs::read_to_string(repo.join("docs/MIGRATION_0_X_TO_1_0.md")).unwrap();
    for needle in [
        "wasm-app.toml",
        "owasidea build",
        "bridge",
        "[updater]",
        "host_abi",
    ] {
        assert!(
            migration.contains(needle),
            "MIGRATION_0_X_TO_1_0.md missing {needle}"
        );
    }
}
