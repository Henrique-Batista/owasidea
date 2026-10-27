use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn plugin(target: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"demo\"\nversion = \"0.3.0\"\n\n\
             [package.metadata.component]\npackage = \"poc:demo\"\n\n\
             [package.metadata.component.target]\npath = \"{target}\"\nworld = \"demo\"\n"
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "wit_bindgen::generate!({ world: \"demo\", path: \".owasidea/wit\" });\n\
         #[owasidea::world(\"demo\")]\nstruct Component;\n\
         impl Guest for Component {\n\
         \x20   #[owasidea::export]\n\
         \x20   fn start_service() -> bool { true }\n\
         \x20   #[owasidea::export]\n\
         \x20   fn greet(name: String) -> String { name }\n\
         }\n",
    )
    .unwrap();
    dir
}

fn generate(dir: &Path) -> serde_json::Value {
    let output = Command::cargo_bin("owasidea")
        .unwrap()
        .args(["wit", "generate"])
        .arg(dir)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn wit_generate_writes_an_artifact_not_source() {
    let dir = plugin(".owasidea/wit");
    let value = generate(dir.path());
    assert_eq!(value["mode"], "attributes");
    assert_eq!(value["world"], "demo");
    assert_eq!(value["exports"], 2);
    assert_eq!(value["changed"], true);
    let generated = fs::read_to_string(dir.path().join(".owasidea/wit/demo.wit")).unwrap();
    assert!(generated.contains("package poc:demo@0.3.0;"));
    assert!(generated.contains("export start-service: func() -> bool;"));
    assert!(generated.contains("export greet: func(name: string) -> string;"));
    assert!(!dir.path().join("wit").exists());

    // A second run is stable and reports no content change.
    let value = generate(dir.path());
    assert_eq!(value["changed"], false);
}

#[test]
fn wit_generate_refuses_to_write_into_source() {
    let dir = plugin("wit");
    let output = Command::cargo_bin("owasidea")
        .unwrap()
        .args(["wit", "generate"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(".owasidea/wit"), "stderr: {stderr}");
}

#[test]
fn rust_scaffold_has_no_wit_file() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("wasm-app.toml");
    let output = Command::cargo_bin("owasidea")
        .unwrap()
        .args(["init"])
        .arg(dir.path())
        .args(["--preset", "minimal"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = Command::cargo_bin("owasidea")
        .unwrap()
        .args(["--config"])
        .arg(&config)
        .args(["plugin", "new", "demo", "--lang", "rust"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let plugin = dir.path().join("plugins/demo");
    assert!(!plugin.join("wit").exists());
    assert!(plugin.join(".owasidea/wit/demo.wit").is_file());
    let generated = fs::read_to_string(plugin.join(".owasidea/wit/demo.wit")).unwrap();
    assert!(generated.contains("export greet: func(name: string) -> string;"));
    let config_raw = fs::read_to_string(&config).unwrap();
    assert!(config_raw.contains("wit_source = \"attributes\""));
}
