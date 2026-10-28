//! Go plugin build helpers.
//!
//! `componentize-go` needs generated `wit_exports.go` glue for the WIT world.
//! The scaffold ships only the idiomatic implementation in
//! `export_wit_world/`, so the first build generates the glue and refreshes
//! `go.mod`/`go.sum`. Later builds reuse the generated file.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How `build` obtains a Go plugin's component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentSource {
    /// Compile the plugin's `wit/` world with `componentize-go`.
    Build,
    /// Build a Go module that carries its WIT world in the SDK
    /// (`componentize-go.toml`, e.g. go.wasmcloud.dev/component).
    SdkBuild,
    /// Reuse an existing `main.wasm` because the plugin has no WIT source.
    ///
    /// Executable `wasi:http` plugins (e.g. `webhook-go`) are plain Go
    /// programs whose world is implied by `go.wasmcloud.dev/component`; there
    /// is no `wit/` directory to compile, so `componentize-go` would fail with
    /// `failed to read path for WIT [wit]`.
    Prebuilt(PathBuf),
}

/// Decide whether to compile a Go plugin or reuse a prebuilt `main.wasm`.
///
/// A missing `wit/` is only acceptable when a component already exists; a
/// plugin with neither must fail with an actionable message instead of the
/// opaque `componentize-go` error.
pub fn component_source(plugin_dir: &Path, wit_dir: &Path, name: &str) -> Result<ComponentSource> {
    let has_wit = std::fs::read_dir(wit_dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("wit")
            })
        })
        .unwrap_or(false);
    if has_wit {
        return Ok(ComponentSource::Build);
    }
    let sdk_world = plugin_dir.join("componentize-go.toml").is_file()
        || std::fs::read_to_string(plugin_dir.join("go.mod"))
            .map(|manifest| manifest.contains("go.wasmcloud.dev/component"))
            .unwrap_or(false);
    if sdk_world {
        return Ok(ComponentSource::SdkBuild);
    }
    let prebuilt = plugin_dir.join("main.wasm");
    if prebuilt.is_file() {
        return Ok(ComponentSource::Prebuilt(prebuilt));
    }
    bail!(
        "go plugin {name} has no wit/ directory or .owasidea/wit ({}), no componentize-go.toml \
         SDK world, and no prebuilt main.wasm at {}; add marker comments, wit/*.wit, or build the component",
        wit_dir.display(),
        prebuilt.display()
    )
}

/// Resolve the WIT directory and world declared by a Go SDK module
/// (`go.wasmcloud.dev/component`). The SDK's `componentize-go.toml` uses
/// `wit_paths = ["wit"]` relative to the module, so the CLI must pass the
/// absolute module path; relying on the bare `componentize-go build` defaults
/// to `./wit` and fails on a clean current working directory.
pub fn sdk_world(plugin_dir: &Path) -> Result<(PathBuf, String)> {
    // `go list -m` does not download modules; on a clean CI runner `.Dir` is
    // empty until the SDK module is present in the module cache.
    let download = std::process::Command::new("go")
        .args(["mod", "download", "go.wasmcloud.dev/component"])
        .current_dir(plugin_dir)
        .output()
        .context("failed to download go.wasmcloud.dev/component")?;
    if !download.status.success() {
        bail!(
            "go mod download failed for go.wasmcloud.dev/component: {}",
            String::from_utf8_lossy(&download.stderr).trim()
        );
    }
    let output = std::process::Command::new("go")
        .args(["list", "-m", "-f", "{{.Dir}}", "go.wasmcloud.dev/component"])
        .current_dir(plugin_dir)
        .output()
        .context("failed to resolve go.wasmcloud.dev/component")?;
    if !output.status.success() {
        bail!(
            "go list -m failed for go.wasmcloud.dev/component: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let module_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let wit_dir = module_dir.join("wit");
    if !wit_dir.is_dir() {
        bail!("SDK module {} has no wit directory", module_dir.display());
    }
    let config =
        std::fs::read_to_string(module_dir.join("componentize-go.toml")).unwrap_or_default();
    let world_re = regex::Regex::new(r#"worlds\s*=\s*\[\s*"([^"]+)"\s*\]"#).unwrap();
    let world = world_re
        .captures(&config)
        .map(|captures| captures[1].to_string())
        .unwrap_or_else(|| "wasmcloud:component-go/wasip2@0.2.0".to_string());
    Ok((wit_dir, world))
}

/// Derive `<package>/<world>` from the plugin's `wit/*.wit`.
pub fn world_arg(wit_dir: &Path) -> Option<String> {
    let world_re = regex::Regex::new(r"world\s+([A-Za-z0-9_-]+)").unwrap();
    let pkg_re = regex::Regex::new(r"package\s+([A-Za-z0-9_-]+:[A-Za-z0-9_-]+)").unwrap();
    let entries = std::fs::read_dir(wit_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("wit") {
            continue;
        }
        let content = std::fs::read_to_string(path).ok()?;
        let world = world_re.captures(&content).map(|c| c[1].to_string());
        let package = pkg_re.captures(&content).map(|c| c[1].to_string());
        if let (Some(world), Some(package)) = (world, package) {
            return Some(format!("{package}/{world}"));
        }
        break;
    }
    None
}

/// Generate `wit_exports.go` when the scaffold does not have it yet.
///
/// The generated bindings hard-code the `wit_component` module path, which is
/// why the Go template uses `module wit_component`.
pub fn ensure_bindings(plugin_dir: &Path, wit_dir: &Path, world: Option<&str>) -> Result<()> {
    if plugin_dir.join("wit_exports.go").is_file() {
        return Ok(());
    }
    if !plugin_dir.join("go.mod").is_file() {
        return Ok(());
    }
    let Some(world) = world else {
        // No WIT world to generate glue for (e.g. componentize-go.toml setups).
        return Ok(());
    };

    // The tool directive alone does not write the transitive checksums that
    // `go tool componentize-go` needs on a fresh scaffold.
    run_go(plugin_dir, &["mod", "tidy"])
        .context("go mod tidy failed before generating componentize-go bindings")?;

    let output_dir = bindings_dir();
    let mut command = std::process::Command::new("go");
    command
        .current_dir(plugin_dir)
        .args(["tool", "componentize-go", "-d"])
        .arg(wit_dir)
        .args(["-w", world]);
    command
        .args(["bindings", "--format", "-o"])
        .arg(&output_dir);

    let output = command
        .output()
        .context("failed to run go tool componentize-go bindings")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&output_dir);
        bail!(
            "go bindings generation failed for {}: {}",
            plugin_dir.display(),
            stderr.trim()
        );
    }

    let generated = output_dir.join("wit_exports.go");
    if !generated.is_file() {
        let _ = std::fs::remove_dir_all(&output_dir);
        bail!(
            "componentize-go did not generate wit_exports.go for {}",
            plugin_dir.display()
        );
    }
    std::fs::copy(&generated, plugin_dir.join("wit_exports.go"))
        .context("failed to copy generated Go bindings")?;
    let _ = std::fs::remove_dir_all(&output_dir);

    run_go(plugin_dir, &["mod", "tidy"])
        .context("go mod tidy failed after generating componentize-go bindings")?;
    Ok(())
}

fn run_go(plugin_dir: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("go")
        .args(args)
        .current_dir(plugin_dir)
        .output()
        .with_context(|| format!("failed to run go {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "go {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn bindings_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "owasidea-go-bindings-{}-{nanos}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prebuilt_component_is_reused_without_wit() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("webhook-go");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("main.wasm"), b"\0asm").unwrap();

        assert_eq!(
            component_source(&plugin, &plugin.join("wit"), "webhook-go").unwrap(),
            ComponentSource::Prebuilt(plugin.join("main.wasm"))
        );
    }

    #[test]
    fn wit_source_wins_over_a_stale_prebuilt_component() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("notes");
        std::fs::create_dir_all(plugin.join("wit")).unwrap();
        std::fs::write(plugin.join("wit/notes.wit"), "package poc:notes;\n").unwrap();
        std::fs::write(plugin.join("main.wasm"), b"\0asm").unwrap();

        assert_eq!(
            component_source(&plugin, &plugin.join("wit"), "notes").unwrap(),
            ComponentSource::Build
        );
    }

    #[test]
    fn sdk_world_uses_componentize_go_without_a_wit_directory() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("webhook-go");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("componentize-go.toml"), "").unwrap();

        assert_eq!(
            component_source(&plugin, &plugin.join("wit"), "webhook-go").unwrap(),
            ComponentSource::SdkBuild
        );
    }

    #[test]
    fn missing_wit_and_component_fails_with_actionable_message() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("broken");
        std::fs::create_dir_all(&plugin).unwrap();

        let error = component_source(&plugin, &plugin.join("wit"), "broken")
            .unwrap_err()
            .to_string();
        assert!(error.contains("wit/"), "{error}");
        assert!(error.contains("main.wasm"), "{error}");
        assert!(error.contains("broken"), "{error}");
    }
}
