//! Bundle profiles (RL-01).
//!
//! A profile is a named overlay applied on top of `wasm-app.toml`:
//! `wasm-app.<profile>.toml` next to the base config. The merge is a
//! deterministic deep merge: tables merge recursively, every other value
//! (scalars and arrays) from the overlay replaces the base value. The base
//! file is only read, never rewritten.
//!
//! `dev`, `staging` and `prod` are the supported profiles. A profile without an
//! overlay file simply uses the base config.

use std::path::{Path, PathBuf};

use crate::config::{config_dir, read_app_config, AppConfig};

/// Profiles supported by `owasidea bundle --profile`.
pub const PROFILES: [&str; 3] = ["dev", "staging", "prod"];

/// Validate a profile name. Unknown or malformed names fail loudly.
pub fn validate_profile(profile: &str) -> anyhow::Result<&str> {
    let profile = profile.trim();
    if profile.is_empty() {
        anyhow::bail!("--profile requires a name (expected one of: dev, staging, prod)");
    }
    if !PROFILES.contains(&profile) {
        anyhow::bail!(
            "unknown bundle profile {:?}; expected one of: {}",
            profile,
            PROFILES.join(", ")
        );
    }
    Ok(profile)
}

/// Overlay path for `profile`: `wasm-app.<profile>.toml` beside the base config.
pub fn overlay_path(config_path: &Path, profile: &str) -> PathBuf {
    config_dir(config_path).join(format!("wasm-app.{profile}.toml"))
}

/// Overlay file actually used for `profile`, or `None` when the profile has no
/// overlay (the merged config then equals the base config).
pub fn active_overlay(config_path: &Path, profile: &str) -> Option<PathBuf> {
    let path = overlay_path(config_path, profile);
    path.is_file().then_some(path)
}

/// Deep merge two TOML values. Tables merge key by key; anything else in the
/// overlay replaces the base value.
pub fn merge_values(base: toml::Value, overlay: toml::Value) -> toml::Value {
    match (base, overlay) {
        (toml::Value::Table(mut base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                let merged = match base.remove(&key) {
                    Some(base_value) => merge_values(base_value, value),
                    None => value,
                };
                base.insert(key, merged);
            }
            toml::Value::Table(base)
        }
        (_, overlay) => overlay,
    }
}

/// Parse and merge base + overlay TOML text.
pub fn merge_toml_str(base: &str, overlay: &str) -> anyhow::Result<toml::Value> {
    let base: toml::Value =
        toml::from_str(base).map_err(|e| anyhow::anyhow!("invalid base TOML: {e}"))?;
    let overlay: toml::Value =
        toml::from_str(overlay).map_err(|e| anyhow::anyhow!("invalid overlay TOML: {e}"))?;
    Ok(merge_values(base, overlay))
}

/// Load the effective config for `profile`. Without a profile (or without an
/// overlay file) this is exactly `read_app_config`.
pub fn merged_config(config_path: &Path, profile: Option<&str>) -> anyhow::Result<AppConfig> {
    let Some(profile) = profile else {
        return read_app_config(config_path);
    };
    let profile = validate_profile(profile)?;
    let base_raw = std::fs::read_to_string(config_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", config_path.display()))?;
    let Some(overlay) = active_overlay(config_path, profile) else {
        return read_app_config(config_path);
    };
    let overlay_raw = std::fs::read_to_string(&overlay)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", overlay.display()))?;
    let merged = merge_toml_str(&base_raw, &overlay_raw)
        .map_err(|e| anyhow::anyhow!("{}: {e}", overlay.display()))?;
    let cfg: AppConfig = merged
        .try_into()
        .map_err(|e| anyhow::anyhow!("{}: {e}", overlay.display()))?;
    cfg.validate()?;
    Ok(cfg)
}

/// Metadata written to `dist/<profile>/owasidea.bundle.json` so a profile
/// bundle is auditable (effective host, identity, plugin set, overlay used).
pub fn bundle_metadata(
    cfg: &AppConfig,
    profile: &str,
    overlay: Option<&Path>,
) -> serde_json::Value {
    let app = cfg.app.as_ref();
    serde_json::json!({
        "profile": profile,
        "overlay": overlay.map(|path| path.to_string_lossy().to_string()),
        "host": crate::config_render::host_url(cfg),
        "product_name": app.and_then(|a| a.name.clone()),
        "version": app.and_then(|a| a.version.clone()),
        "identifier": app.and_then(|a| a.identifier.clone()),
        "plugins": cfg
            .plugins
            .as_ref()
            .map(|plugins| plugins.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

/// Artifact directory for a bundle. Profiles always get their own subdirectory
/// (`dist/<profile>` by default) so two profiles never overwrite each other.
pub fn output_dir(root: &Path, cfg: &AppConfig, profile: Option<&str>) -> anyhow::Result<PathBuf> {
    let configured = cfg
        .desktop
        .as_ref()
        .and_then(|d| d.output.as_deref())
        .unwrap_or("dist");
    let base = crate::config_render::safe_resolve(root, configured)?;
    Ok(match profile {
        Some(profile) => base.join(profile),
        None => base,
    })
}

/// Path of the profile-specific runtime descriptor. The base
/// `ui/.owasidea/runtime.json` (used by `dev`/`run`) is never overwritten.
pub fn runtime_path(root: &Path, cfg: &AppConfig, profile: &str) -> anyhow::Result<PathBuf> {
    Ok(crate::config_render::ui_dir(root, cfg)?
        .join(".owasidea")
        .join(format!("runtime.{profile}.json")))
}

/// Write the profile runtime descriptor from the merged config and return it.
pub fn write_runtime(root: &Path, cfg: &AppConfig, profile: &str) -> anyhow::Result<PathBuf> {
    let path = runtime_path(root, cfg, profile)?;
    let build_id = crate::config_render::resolve_build_id(root);
    let mut bytes =
        serde_json::to_vec_pretty(&crate::config_render::runtime_content(cfg, build_id))?;
    bytes.push(b'\n');
    crate::config_render::atomic_write(&path, &bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "owasidea-profile-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const BASE: &str = r#"
[app]
name = "shipyard"
version = "0.1.0"
identifier = "dev.owasidea.shipyard"

[host]
addr = "127.0.0.1:8790"

[desktop]
path = "desktop"
ui = "ui"
output = "dist"

[plugins.core]
path = "plugins/core"
language = "rust"
type = "library"

[plugins.webhook]
path = "plugins/webhook"
language = "go"
type = "executable"
"#;

    #[test]
    fn overlay_merges_host_and_plugins() {
        let dir = temp_root("merge");
        let config = dir.join("wasm-app.toml");
        std::fs::write(&config, BASE).unwrap();
        std::fs::write(
            dir.join("wasm-app.staging.toml"),
            "[host]\naddr = \"127.0.0.1:9100\"\n\n[plugins.webhook]\nhttp_outbound = [\"https://staging.example\"]\n",
        )
        .unwrap();

        let cfg = merged_config(&config, Some("staging")).unwrap();
        assert_eq!(cfg.host.unwrap().addr.unwrap(), "127.0.0.1:9100");
        let plugins = cfg.plugins.unwrap();
        assert_eq!(plugins.len(), 2, "overlay must not drop base plugins");
        assert_eq!(plugins["core"].path, "plugins/core");
        assert_eq!(plugins["webhook"].path, "plugins/webhook");
        assert_eq!(plugins["webhook"].language, "go");
        assert_eq!(
            plugins["webhook"].http_outbound,
            vec!["https://staging.example".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_overlay_uses_base_config() {
        let dir = temp_root("no-overlay");
        let config = dir.join("wasm-app.toml");
        std::fs::write(&config, BASE).unwrap();
        let cfg = merged_config(&config, Some("prod")).unwrap();
        assert_eq!(cfg.host.unwrap().addr.unwrap(), "127.0.0.1:8790");
        assert!(active_overlay(&config, "prod").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base_config_is_never_modified() {
        let dir = temp_root("immutable");
        let config = dir.join("wasm-app.toml");
        std::fs::write(&config, BASE).unwrap();
        std::fs::write(
            dir.join("wasm-app.prod.toml"),
            "[app]\nversion = \"1.2.3\"\n\n[host]\naddr = \"0.0.0.0:9200\"\n",
        )
        .unwrap();
        let before = std::fs::read_to_string(&config).unwrap();
        let cfg = merged_config(&config, Some("prod")).unwrap();
        assert_eq!(cfg.app.unwrap().version.unwrap(), "1.2.3");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_profile_fails() {
        let err = validate_profile("qa").unwrap_err().to_string();
        assert!(err.contains("unknown bundle profile"), "msg: {err}");
        assert!(err.contains("dev, staging, prod"), "msg: {err}");
    }

    #[test]
    fn empty_profile_fails() {
        assert!(validate_profile("   ").is_err());
    }

    #[test]
    fn profiles_get_distinct_output_dirs() {
        let dir = temp_root("output");
        let config = dir.join("wasm-app.toml");
        std::fs::write(&config, BASE).unwrap();
        let cfg = merged_config(&config, None).unwrap();
        let dev = output_dir(&dir, &cfg, Some("dev")).unwrap();
        let prod = output_dir(&dir, &cfg, Some("prod")).unwrap();
        // safe_resolve canonicalizes the root, which on Windows expands 8.3
        // short names; compare against the canonical root.
        let root = dir.canonicalize().unwrap();
        assert_eq!(dev, root.join("dist/dev"));
        assert_eq!(prod, root.join("dist/prod"));
        assert_ne!(dev, prod);
        assert_eq!(output_dir(&dir, &cfg, None).unwrap(), root.join("dist"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overlay_unknown_field_fails() {
        let dir = temp_root("unknown-field");
        let config = dir.join("wasm-app.toml");
        std::fs::write(&config, BASE).unwrap();
        std::fs::write(dir.join("wasm-app.dev.toml"), "[bogus]\nvalue = 1\n").unwrap();
        let err = merged_config(&config, Some("dev")).unwrap_err().to_string();
        assert!(err.contains("bogus"), "msg: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn arrays_replace_and_tables_merge() {
        let merged = merge_toml_str(
            "[smoke]\ncalls = [\"a\"]\n\n[smoke.http]\nx = 1\n",
            "[smoke]\ncalls = [\"b\"]\n",
        )
        .unwrap();
        assert_eq!(merged["smoke"]["calls"].as_array().unwrap().len(), 1);
        assert_eq!(merged["smoke"]["calls"][0].as_str(), Some("b"));
        assert_eq!(merged["smoke"]["http"]["x"].as_integer(), Some(1));
    }
}
