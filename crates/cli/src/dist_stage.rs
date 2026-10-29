//! Stage the runtime payload into the bundle output directory.
//!
//! `owasidea bundle` writes installer artifacts (`.deb`, `.rpm`, `.AppImage`,
//! ...). Verification, however, needs the manifests, the component bytes and
//! the UI assets in the same tree, otherwise `owasidea verify dist/` has
//! nothing to check and reports a misleading success.
//!
//! This module copies that payload next to the artifacts so a release directory
//! is self-describing and verifiable (FIX-05).

use anyhow::Context;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StageReport {
    pub manifests: usize,
    pub components: usize,
    pub ui: bool,
}

impl StageReport {
    pub fn summary(&self) -> String {
        format!(
            "{} manifest(s), {} component(s), UI {}",
            self.manifests,
            self.components,
            if self.ui { "staged" } else { "absent" }
        )
    }
}

/// Copies manifests into a runtime layout and rewrites `component` to the
/// staged `plugins/<name>/<name>.component.wasm` path. The app tree keeps
/// `.owasidea/build/...`; bundles are self-contained and use this layout.
pub fn write_runtime_manifests(source: &Path, destination: &Path) -> anyhow::Result<usize> {
    reset_dir(destination)?;
    let mut count = 0;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let is_manifest = file_name
            .to_str()
            .is_some_and(|name| name.ends_with(".manifest.json"));
        if !is_manifest {
            std::fs::copy(&path, destination.join(&file_name))?;
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        let Ok(mut manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
            std::fs::copy(&path, destination.join(&file_name))?;
            count += 1;
            continue;
        };
        if manifest.get("component").is_some() {
            let fallback = file_name
                .to_str()
                .and_then(|name| name.strip_suffix(".manifest.json"))
                .unwrap_or_default();
            let plugin = manifest["name"].as_str().unwrap_or(fallback);
            manifest["component"] =
                serde_json::json!(format!("../plugins/{plugin}/{plugin}.component.wasm"));
            std::fs::write(
                destination.join(&file_name),
                serde_json::to_string_pretty(&manifest)? + "\n",
            )?;
            count += 1;
        } else {
            std::fs::copy(&path, destination.join(&file_name))?;
        }
    }
    Ok(count)
}

/// Copy `manifests/`, the components they reference and `ui/public/` into
/// `output`. Existing staged copies are replaced so a removed plugin does not
/// leave a stale manifest behind.
pub fn stage_dist(root: &Path, output: &Path) -> anyhow::Result<StageReport> {
    let manifests_src = root.join("manifests");
    if !manifests_src.is_dir() {
        // `bundle --no-build` may run before the first build. Keep the previous
        // tolerant behaviour: the installers are still produced, but there is
        // nothing to stage and `verify` will report NO_MANIFESTS. Drop any
        // payload staged by an earlier run so verification cannot pass on stale
        // files.
        eprintln!(
            "warning: manifests directory not found under {}; skipping dist staging (run `owasidea build` first)",
            root.display()
        );
        clear_staged(output);
        return Ok(StageReport::default());
    }
    let mut report = StageReport::default();

    let manifests_dst = output.join("manifests");
    write_runtime_manifests(&manifests_src, &manifests_dst)?;
    report.manifests = std::fs::read_dir(&manifests_src)?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".manifest.json"))
        })
        .count();

    let plugins_dst = output.join("plugins");
    reset_dir(&plugins_dst)?;
    for entry in std::fs::read_dir(&manifests_src)? {
        let path = entry?.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".manifest.json"))
        {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let fallback_name = path
            .file_name()
            .and_then(|file| file.to_str())
            .and_then(|file| file.strip_suffix(".manifest.json"))
            .unwrap_or_default();
        let name = manifest["name"].as_str().unwrap_or(fallback_name);
        let component = manifest["component"]
            .as_str()
            .map(|relative| manifests_src.join(relative))
            .filter(|component| component.is_file())
            .unwrap_or_else(|| {
                root.join("plugins")
                    .join(name)
                    .join(format!("{name}.component.wasm"))
            });
        if !component.is_file() {
            continue;
        }
        let target = plugins_dst.join(name);
        std::fs::create_dir_all(&target)?;
        std::fs::copy(&component, target.join(component.file_name().unwrap()))?;
        report.components += 1;
    }

    let ui_src = root.join("ui").join("public");
    if ui_src.is_dir() {
        let ui_dst = output.join("ui").join("public");
        reset_dir(&ui_dst)?;
        crate::copy_dir_all(&ui_src, &ui_dst)?;
        report.ui = true;
    }

    Ok(report)
}

fn reset_dir(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("clear staged directory {}", path.display()))?;
    }
    std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    Ok(())
}

fn clear_staged(output: &Path) {
    for path in [
        output.join("manifests"),
        output.join("plugins"),
        output.join("ui").join("public"),
    ] {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "owasidea-stage-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn stages_manifests_components_ui_and_trust_key() {
        let root = temp_root("full");
        std::fs::create_dir_all(root.join("manifests")).unwrap();
        std::fs::create_dir_all(root.join("plugins/demo")).unwrap();
        std::fs::create_dir_all(root.join("ui/public/plugins/panel")).unwrap();
        std::fs::create_dir_all(root.join("keys")).unwrap();
        std::fs::write(root.join("manifests/demo.manifest.json"), b"{}").unwrap();
        std::fs::write(root.join("plugins/demo/demo.component.wasm"), b"component").unwrap();
        std::fs::write(root.join("ui/public/ui-plugins.json"), b"[]").unwrap();
        std::fs::write(root.join("ui/public/plugins/panel/index.html"), b"<html/>").unwrap();
        std::fs::write(root.join("keys/owasidea.pub"), b"key").unwrap();

        let dist = root.join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        let report = stage_dist(&root, &dist).unwrap();

        assert_eq!(
            report,
            StageReport {
                manifests: 1,
                components: 1,
                ui: true,
            }
        );
        assert!(dist.join("manifests/demo.manifest.json").is_file());
        assert!(dist.join("plugins/demo/demo.component.wasm").is_file());
        assert!(dist.join("ui/public/ui-plugins.json").is_file());
    }

    #[test]
    fn restage_drops_stale_plugin_directories() {
        let root = temp_root("restage");
        std::fs::create_dir_all(root.join("manifests")).unwrap();
        std::fs::create_dir_all(root.join("plugins/demo")).unwrap();
        std::fs::write(root.join("manifests/demo.manifest.json"), b"{}").unwrap();
        std::fs::write(root.join("plugins/demo/demo.component.wasm"), b"c").unwrap();
        let dist = root.join("dist");
        std::fs::create_dir_all(dist.join("plugins/stale")).unwrap();
        std::fs::write(dist.join("plugins/stale/stale.component.wasm"), b"old").unwrap();

        stage_dist(&root, &dist).unwrap();

        assert!(!dist.join("plugins/stale").exists());
        assert!(dist.join("plugins/demo/demo.component.wasm").is_file());
    }

    #[test]
    fn missing_manifests_skips_staging_and_clears_stale_payload() {
        let root = temp_root("empty");
        let dist = root.join("dist");
        std::fs::create_dir_all(dist.join("manifests")).unwrap();
        std::fs::write(dist.join("manifests/old.manifest.json"), b"{}").unwrap();
        let report = stage_dist(&root, &dist).unwrap();
        assert_eq!(report, StageReport::default());
        assert!(!dist.join("manifests").exists());
    }
}
