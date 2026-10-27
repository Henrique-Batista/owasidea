use std::path::{Path, PathBuf};
use std::process::Child;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TauriDevInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

/// Pure helper describing how `cargo tauri dev` must be invoked for DL-01.
pub fn tauri_dev_invocation(
    desktop_dir: &Path,
    tauri_config: &Path,
    host_url: &str,
    http_addr: &str,
    manifest_dir: &Path,
    runtime_path: Option<&Path>,
) -> TauriDevInvocation {
    let args = vec![
        "tauri".to_string(),
        "dev".to_string(),
        "--config".to_string(),
        tauri_config.display().to_string(),
    ];
    let mut env = vec![
        ("OWASIDEA_HOST".to_string(), host_url.to_string()),
        ("HOST_ADDR".to_string(), http_addr.to_string()),
        (
            "OWASIDEA_MANIFESTS".to_string(),
            manifest_dir.display().to_string(),
        ),
    ];
    if let Some(path) = runtime_path {
        env.push(("OWASIDEA_RUNTIME".to_string(), path.display().to_string()));
    }
    env.push(("OWASIDEA_DEV_NO_HOST".to_string(), "1".to_string()));
    TauriDevInvocation {
        program: "cargo".to_string(),
        args,
        cwd: desktop_dir.to_path_buf(),
        env,
    }
}

pub fn cargo_tauri_available() -> bool {
    std::process::Command::new("cargo")
        .args(["tauri", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn spawn_prefixed(mut child: Child, prefix: &'static str) -> Child {
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                println!("[{}] {}", prefix, line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                eprintln!("[{}] {}", prefix, line);
            }
        });
    }
    child
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_shape() {
        let inv = tauri_dev_invocation(
            Path::new("/app/desktop"),
            Path::new("/app/desktop/tauri.dev.conf.json"),
            "http://127.0.0.1:5173",
            "127.0.0.1:8787",
            Path::new("/app/manifests"),
            Some(Path::new("/app/ui/.owasidea/runtime.json")),
        );
        assert_eq!(inv.program, "cargo");
        assert_eq!(inv.args[0], "tauri");
        assert_eq!(inv.args[1], "dev");
        assert!(inv.args.contains(&"--config".to_string()));
        assert_eq!(inv.cwd, PathBuf::from("/app/desktop"));
        let env_map: std::collections::HashMap<_, _> = inv.env.iter().cloned().collect();
        assert_eq!(
            env_map.get("OWASIDEA_HOST").unwrap(),
            "http://127.0.0.1:5173"
        );
        assert_eq!(env_map.get("HOST_ADDR").unwrap(), "127.0.0.1:8787");
        assert_eq!(env_map.get("OWASIDEA_MANIFESTS").unwrap(), "/app/manifests");
        assert_eq!(
            env_map.get("OWASIDEA_RUNTIME").unwrap(),
            "/app/ui/.owasidea/runtime.json"
        );
        assert_eq!(env_map.get("OWASIDEA_DEV_NO_HOST").unwrap(), "1");
    }

    #[test]
    fn invocation_omits_empty_runtime() {
        let inv = tauri_dev_invocation(
            Path::new("/app/desktop"),
            Path::new("/app/desktop/tauri.dev.conf.json"),
            "http://127.0.0.1:5173",
            "127.0.0.1:8787",
            Path::new("/app/manifests"),
            None,
        );
        assert!(!inv.env.iter().any(|(k, _)| k == "OWASIDEA_RUNTIME"));
        assert!(inv
            .env
            .iter()
            .any(|(k, v)| k == "OWASIDEA_DEV_NO_HOST" && v == "1"));
    }
}
