use std::path::{Path, PathBuf};

use crate::config::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderTarget {
    Runtime,
    Tauri,
    Playwright,
}

// RenderTarget::All is a pseudo value; we use a separate function for "all".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderScope {
    Single(RenderTarget),
    All,
}

pub fn parse_target(s: &str) -> anyhow::Result<RenderScope> {
    match s {
        "runtime" => Ok(RenderScope::Single(RenderTarget::Runtime)),
        "tauri" => Ok(RenderScope::Single(RenderTarget::Tauri)),
        "playwright" => Ok(RenderScope::Single(RenderTarget::Playwright)),
        "all" => Ok(RenderScope::All),
        other => {
            anyhow::bail!("unknown render target: {other} (expected runtime|tauri|playwright|all)")
        }
    }
}

fn host_url(cfg: &AppConfig) -> String {
    let addr = cfg
        .host
        .as_ref()
        .and_then(|h| h.addr.clone())
        .unwrap_or_else(|| "127.0.0.1:8787".to_string());
    format!("http://{}", addr.trim_end_matches('/'))
}

fn ui_url(cfg: &AppConfig) -> String {
    let addr = cfg
        .dev
        .as_ref()
        .and_then(|d| d.ui_addr.clone())
        .unwrap_or_else(|| "127.0.0.1:5173".to_string());
    format!("http://{}", addr.trim_end_matches('/'))
}

fn app_name(cfg: &AppConfig) -> String {
    cfg.app
        .as_ref()
        .and_then(|a| a.name.clone())
        .unwrap_or_else(|| "Owasidea".to_string())
}

fn app_version(cfg: &AppConfig) -> String {
    cfg.app
        .as_ref()
        .and_then(|a| a.version.clone())
        .unwrap_or_else(|| "0.1.0".to_string())
}

fn ui_dir(root: &Path, cfg: &AppConfig) -> PathBuf {
    let ui = cfg
        .desktop
        .as_ref()
        .and_then(|d| d.ui.clone())
        .unwrap_or_else(|| "ui".to_string());
    if Path::new(&ui).is_absolute() {
        PathBuf::from(ui)
    } else {
        root.join(ui)
    }
}

fn desktop_dir(root: &Path, cfg: &AppConfig) -> PathBuf {
    let path = cfg.desktop.as_ref().and_then(|d| d.path.clone());
    match path {
        Some(p) => {
            let pb = PathBuf::from(&p);
            if pb.is_absolute() {
                pb
            } else {
                root.join(pb)
            }
        }
        None => {
            if root.join("desktop/Cargo.toml").is_file() {
                root.join("desktop")
            } else {
                root.join("crates/desktop")
            }
        }
    }
}

fn add_origin_to_csp(csp: &str, origin: &str) -> String {
    let mut directives: Vec<String> = csp.split(';').map(str::trim).map(String::from).collect();
    let mut found = false;
    for d in directives.iter_mut() {
        if let Some(rest) = d.strip_prefix("connect-src") {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                if !tokens.iter().any(|t| t.trim_end_matches('/') == origin) {
                    if rest.trim().is_empty() {
                        *d = format!("connect-src {}", origin);
                    } else {
                        *d = format!("connect-src {} {}", rest.trim(), origin);
                    }
                }
                found = true;
            }
        }
    }
    if !found {
        directives.push(format!("connect-src {}", origin));
    }
    directives.join("; ")
}

fn atomic_write(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp.owasidea");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn resolve_build_id(root: &Path) -> Option<String> {
    if let Ok(v) = std::env::var("OWASIDEA_BUILD_ID") {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let dir = root.join("manifests");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(s) = v["metadata"]["build_id"].as_str() {
                        if !s.trim().is_empty() {
                            return Some(s.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn runtime_content(cfg: &AppConfig, build_id: Option<String>) -> serde_json::Value {
    let build_id = build_id.or_else(|| {
        std::env::var("OWASIDEA_BUILD_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
    });
    serde_json::json!({
        "host": host_url(cfg),
        "app": app_name(cfg),
        "version": app_version(cfg),
        "build_id": build_id,
        "manifests_dir": "manifests",
    })
}

fn playwright_content(cfg: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "baseURL": ui_url(cfg),
        "hostURL": host_url(cfg),
    })
}

fn tauri_content(root: &Path, cfg: &AppConfig) -> anyhow::Result<serde_json::Value> {
    let dd = desktop_dir(root, cfg);
    let base_path = dd.join("tauri.conf.json");
    let raw = std::fs::read_to_string(&base_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {}", base_path.display(), e))?;
    let mut value: serde_json::Value = serde_json::from_str(&raw)?;
    let dev_url = ui_url(cfg);
    if let Some(build) = value.get_mut("build").and_then(|v| v.as_object_mut()) {
        build.insert(
            "devUrl".to_string(),
            serde_json::Value::String(dev_url.clone()),
        );
    } else {
        let mut build = serde_json::Map::new();
        build.insert("devUrl".to_string(), serde_json::Value::String(dev_url));
        value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("tauri.conf.json root must be object"))?
            .insert("build".to_string(), serde_json::Value::Object(build));
    }
    let origin = host_url(cfg);
    let origin = origin.trim_end_matches('/');
    if let Some(csp) = value
        .get("app")
        .and_then(|a| a.get("security"))
        .and_then(|s| s.get("csp"))
        .and_then(|c| c.as_str())
    {
        let new_csp = add_origin_to_csp(csp, origin);
        if let Some(csp_slot) = value
            .get_mut("app")
            .and_then(|a| a.get_mut("security"))
            .and_then(|s| s.as_object_mut())
            .and_then(|o| o.get_mut("csp"))
        {
            *csp_slot = serde_json::Value::String(new_csp);
        }
    }
    Ok(value)
}

/// Merge-based content for `tauri.dev.conf.json`: clone the base `tauri.conf.json`
/// and overwrite only `build.devUrl` with the reserved UI URL.
pub fn tauri_dev_content(
    root: &Path,
    cfg: &AppConfig,
    dev_url: &str,
) -> anyhow::Result<serde_json::Value> {
    let dd = desktop_dir(root, cfg);
    let base_path = dd.join("tauri.conf.json");
    let raw = std::fs::read_to_string(&base_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {}", base_path.display(), e))?;
    let mut value: serde_json::Value = serde_json::from_str(&raw)?;
    if let Some(build) = value.get_mut("build").and_then(|v| v.as_object_mut()) {
        build.insert(
            "devUrl".to_string(),
            serde_json::Value::String(dev_url.to_string()),
        );
    } else {
        let mut build = serde_json::Map::new();
        build.insert(
            "devUrl".to_string(),
            serde_json::Value::String(dev_url.to_string()),
        );
        value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("tauri.conf.json root must be object"))?
            .insert("build".to_string(), serde_json::Value::Object(build));
    }
    Ok(value)
}

pub fn write_tauri_dev_config(config_path: &Path, dev_url: &str) -> anyhow::Result<PathBuf> {
    let cfg = read_app_config(config_path)?;
    cfg.validate()?;
    let root = config_dir(config_path);
    let dd = desktop_dir(&root, &cfg);
    let value = tauri_dev_content(&root, &cfg, dev_url)?;
    let path = dd.join("tauri.dev.conf.json");
    let bytes = serde_json::to_vec_pretty(&value)?;
    let mut b = bytes;
    b.push(b'\n');
    atomic_write(&path, &b)?;
    Ok(path)
}

pub fn render_targets(
    config_path: &Path,
    scope: RenderScope,
    check: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    let cfg = read_app_config(config_path)?;
    cfg.validate()?;
    let root = config_dir(config_path);

    let mut targets: Vec<RenderTarget> = match scope {
        RenderScope::All => vec![
            RenderTarget::Runtime,
            RenderTarget::Tauri,
            RenderTarget::Playwright,
        ],
        RenderScope::Single(t) => vec![t],
    };

    let mut outputs: Vec<PathBuf> = Vec::new();
    let mut stale: Vec<PathBuf> = Vec::new();

    for t in targets.drain(..) {
        let (path, content_bytes) = match t {
            RenderTarget::Runtime => {
                let p = ui_dir(&root, &cfg).join(".owasidea").join("runtime.json");
                let build_id = resolve_build_id(&root);
                let bytes = serde_json::to_vec_pretty(&runtime_content(&cfg, build_id))?;
                let mut b = bytes;
                b.push(b'\n');
                (p, b)
            }
            RenderTarget::Playwright => {
                let p = ui_dir(&root, &cfg)
                    .join(".owasidea")
                    .join("playwright.json");
                let bytes = serde_json::to_vec_pretty(&playwright_content(&cfg))?;
                let mut b = bytes;
                b.push(b'\n');
                (p, b)
            }
            RenderTarget::Tauri => {
                let dd = desktop_dir(&root, &cfg);
                let p = dd.join("tauri.generated.conf.json");
                let value = tauri_content(&root, &cfg)?;
                let bytes = serde_json::to_vec_pretty(&value)?;
                let mut b = bytes;
                b.push(b'\n');
                (p, b)
            }
        };

        outputs.push(path.clone());

        if check {
            let existing = std::fs::read(&path).ok();
            if existing.as_deref() != Some(content_bytes.as_slice()) {
                stale.push(path);
            }
        } else {
            atomic_write(&path, &content_bytes)?;
        }
    }

    if check && !stale.is_empty() {
        for p in &stale {
            println!("{}", p.display());
        }
        anyhow::bail!("config render --check failed: stale or missing files");
    }

    for p in &outputs {
        println!("{}", p.display());
    }
    Ok(outputs)
}

pub fn cmd_config_render(config_path: &Path, target: &str, check: bool) -> anyhow::Result<()> {
    let scope = parse_target(target)?;
    render_targets(config_path, scope, check)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "owasidea-config-render-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("manifests")).unwrap();
        dir
    }

    #[test]
    fn resolve_build_id_from_manifest() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_BUILD_ID");
        let root = temp_root();
        std::fs::write(
            root.join("manifests").join("core.manifest.json"),
            r#"{"metadata":{"build_id":"abc-123"}}"#,
        )
        .unwrap();
        assert_eq!(resolve_build_id(&root), Some("abc-123".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_build_id_env_precedence() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("OWASIDEA_BUILD_ID", "env-id");
        let root = temp_root();
        std::fs::write(
            root.join("manifests").join("core.manifest.json"),
            r#"{"metadata":{"build_id":"abc-123"}}"#,
        )
        .unwrap();
        assert_eq!(resolve_build_id(&root), Some("env-id".to_string()));
        std::env::remove_var("OWASIDEA_BUILD_ID");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_content_uses_manifest_id() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_BUILD_ID");
        let root = temp_root();
        std::fs::write(
            root.join("manifests").join("core.manifest.json"),
            r#"{"metadata":{"build_id":"abc-123"}}"#,
        )
        .unwrap();
        let cfg = AppConfig::default();
        let v = runtime_content(&cfg, resolve_build_id(&root));
        assert_eq!(v["build_id"], "abc-123");
        let _ = std::fs::remove_dir_all(&root);
    }
}
