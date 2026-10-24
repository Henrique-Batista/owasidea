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
