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

/// Copy `manifests/`, `plugins/*/<name>.component.wasm` and `ui/public/` into
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
    reset_dir(&manifests_dst)?;
    for entry in std::fs::read_dir(&manifests_src)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        std::fs::copy(&path, manifests_dst.join(entry.file_name()))?;
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".manifest.json"))
        {
            report.manifests += 1;
        }
    }

    let plugins_src = root.join("plugins");
    let plugins_dst = output.join("plugins");
    reset_dir(&plugins_dst)?;
    if plugins_src.is_dir() {
        for entry in std::fs::read_dir(&plugins_src)? {
            let entry = entry?;
            let plugin_dir = entry.path();
            if !plugin_dir.is_dir() {
                continue;
            }
            let Some(name) = plugin_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let component = plugin_dir.join(format!("{name}.component.wasm"));
            if !component.is_file() {
                continue;
            }
            let target = plugins_dst.join(name);
            std::fs::create_dir_all(&target)?;
            std::fs::copy(&component, target.join(component.file_name().unwrap()))?;
            report.components += 1;
        }
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
