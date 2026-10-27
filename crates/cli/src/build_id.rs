//! Deterministic build identifier (RF-07).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Compute a build id.
///
/// Precedence:
/// 1. `OWASIDEA_BUILD_ID` env var (explicit override).
/// 2. `SOURCE_DATE_EPOCH` env var -> `<git-short-sha>-<epoch>` (deterministic).
/// 3. Otherwise `<git-short-sha>-<utc-unix-timestamp>` (or `nogit-<ts>` outside git).
///
/// Two invocations with the same `SOURCE_DATE_EPOCH` (and same git HEAD) yield
/// the same id.
pub fn compute_build_id() -> String {
    if let Ok(v) = std::env::var("OWASIDEA_BUILD_ID") {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }

    let sha = git_short_sha().unwrap_or_else(|| "nogit".to_string());

    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(n) = epoch.trim().parse::<u64>() {
            return format!("{}-{}", sha, n);
        }
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}-{}", sha, ts)
}

fn git_short_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Resolve the build id recorded in manifests (if any), without mutating env.
pub fn resolve_from_manifests(manifest_dir: &std::path::Path) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(manifest_dir) {
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

/// Read the build id from the generated UI runtime bundle, if present.
pub fn read_runtime_build_id(root: &std::path::Path) -> Option<String> {
    let p = root.join("ui").join(".owasidea").join("runtime.json");
    let raw = std::fs::read_to_string(&p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v["build_id"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Try to populate `OWASIDEA_BUILD_ID` from manifests if not already set.
pub fn ensure_env_from_manifests(manifest_dir: &std::path::Path) {
    if std::env::var("OWASIDEA_BUILD_ID")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(manifest_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(s) = v["metadata"]["build_id"].as_str() {
                        if !s.trim().is_empty() {
                            std::env::set_var("OWASIDEA_BUILD_ID", s);
                            return;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK as LOCK;

    // Env vars are process-global; the crate-wide lock serializes every test
    // that mutates or reads them, including `config_render`.

    #[test]
    fn override_takes_precedence() {
        let _g = LOCK.lock().unwrap();
        std::env::set_var("OWASIDEA_BUILD_ID", "override-id");
        std::env::remove_var("SOURCE_DATE_EPOCH");
        assert_eq!(compute_build_id(), "override-id");
        std::env::remove_var("OWASIDEA_BUILD_ID");
    }

    #[test]
    fn source_date_epoch_deterministic() {
        let _g = LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_BUILD_ID");
        std::env::set_var("SOURCE_DATE_EPOCH", "1700000000");
        let a = compute_build_id();
        let b = compute_build_id();
        assert_eq!(a, b);
        assert!(a.ends_with("-1700000000"));
        std::env::remove_var("SOURCE_DATE_EPOCH");
    }

    #[test]
    fn format_has_sha_and_epoch() {
        let _g = LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_BUILD_ID");
        std::env::set_var("SOURCE_DATE_EPOCH", "12345");
        let id = compute_build_id();
        assert!(id.contains('-'));
        assert!(id.ends_with("12345"));
        std::env::remove_var("SOURCE_DATE_EPOCH");
    }
}
