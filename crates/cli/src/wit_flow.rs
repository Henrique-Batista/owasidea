//! Selects where a component contract comes from and keeps it honest.
//!
//! `auto` (the default) uses a WIT file when one exists and otherwise renders
//! the contract from `#[owasidea::world]` / `#[owasidea::export]` markers.
//! Attribute-generated WIT is an artifact: for Rust it lives in
//! `.owasidea/wit` (ignored by git) and the Cargo metadata target points there.
//! When both a contract file and markers exist, the build fails on drift
//! instead of silently compiling a contract the developer no longer owns.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitSource {
    Auto,
    Wit,
    Attributes,
}

impl WitSource {
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.unwrap_or("auto") {
            "auto" => Ok(Self::Auto),
            "wit" => Ok(Self::Wit),
            "attributes" => Ok(Self::Attributes),
            other => bail!("unknown wit_source {other:?}: expected auto, wit or attributes"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Wit => "wit",
            Self::Attributes => "attributes",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub mode: WitSource,
    pub world: String,
    pub dir: PathBuf,
    pub changed: bool,
    pub exports: usize,
}

impl Prepared {
    pub fn summary(&self) -> String {
        let file = self.dir.join(format!("{}.wit", self.world));
        match self.mode {
            WitSource::Attributes => format!(
                "WIT: {} (generated from attributes, {} exports{})",
                file.display(),
                self.exports,
                if self.changed { ", updated" } else { "" }
            ),
            _ => format!("WIT: {} (source)", file.display()),
        }
    }
}

struct Target {
    dir: PathBuf,
    world: String,
    version: String,
}

fn rust_target(plugin_dir: &Path) -> Result<Target> {
    let manifest = plugin_dir.join("Cargo.toml");
    let raw = std::fs::read_to_string(&manifest)
        .with_context(|| format!("read {}", manifest.display()))?;
    let doc: toml::Value = toml::from_str(&raw).context("parse Cargo.toml")?;
    let package = doc.get("package").context("Cargo.toml has no [package]")?;
    let name = package
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("component")
        .to_string();
    let version = package
        .get("version")
        .and_then(|value| value.as_str())
        .unwrap_or("0.1.0")
        .to_string();
    let target = package
        .get("metadata")
        .and_then(|value| value.get("component"))
        .and_then(|value| value.get("target"));
    let dir = target
        .and_then(|value| value.get("path"))
        .and_then(|value| value.as_str())
        .unwrap_or("wit");
    let world = target
        .and_then(|value| value.get("world"))
        .and_then(|value| value.as_str())
        .unwrap_or(&name)
        .to_string();
    Ok(Target {
        dir: PathBuf::from(dir),
        world,
        version,
    })
}

fn has_wit(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("wit")
            })
        })
        .unwrap_or(false)
}

fn is_artifact_dir(dir: &Path) -> bool {
    dir.components()
        .any(|component| component.as_os_str() == ".owasidea")
}

fn legacy_wit(plugin_dir: &Path, target_dir: &Path, world: &str) -> Option<PathBuf> {
    let wit = plugin_dir.join("wit");
    if wit != *target_dir && wit.join(format!("{world}.wit")).is_file() {
        Some(wit)
    } else {
        None
    }
}

fn resolve(plugin_dir: &Path, dir: &Path) -> PathBuf {
    if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        plugin_dir.join(dir)
    }
}

/// Removes generated `.wit` files left by an earlier world name. Only called
/// for artifact directories; hand-written `wit/` trees are never pruned.
fn prune_generated(dir: &Path, keep: &str) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("wit")
            && path.file_name().and_then(|name| name.to_str()) != Some(keep)
        {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn drift_error(name: &str, path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "plugin {name}: source attributes diverge from {}; \
         run `owasidea wit generate` or set wit_source = \"attributes\" so the contract is generated",
        path.display()
    )
}

/// Detects the language from the plugin directory when the caller does not
/// know it (used by `owasidea wit generate`).
pub fn detect_language(plugin_dir: &Path) -> &'static str {
    if plugin_dir.join("Cargo.toml").is_file() {
        "rust"
    } else if plugin_dir.join("go.mod").is_file() {
        "go"
    } else if plugin_dir.join("package.json").is_file() {
        "typescript"
    } else if std::fs::read_dir(plugin_dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("csproj")
            })
        })
        .unwrap_or(false)
    {
        "csharp"
    } else {
        "rust"
    }
}

/// Chooses the contract source, renders it when needed, writes generated WIT
/// and returns what the build should report. `None` means this plugin has no
/// contract to prepare (for example a UI plugin).
/// Renders and writes the contract unconditionally. Used by
/// `owasidea wit generate` and by `plugin new` to seed a buildable plugin.
/// Renders and writes the contract unconditionally. Used by
/// `owasidea wit generate` and by `plugin new` to seed a buildable plugin.
pub fn generate(plugin_dir: &Path, name: &str, language: &str, version: &str) -> Result<Prepared> {
    let csharp = language == "csharp";
    let (dir, world, package, wrap_interface, version) = if language == "rust" {
        let target = rust_target(plugin_dir)?;
        if !is_artifact_dir(&target.dir) {
            bail!(
                "plugin {name}: attribute-generated WIT must be an artifact; set \
                 [package.metadata.component.target] path = \".owasidea/wit\" (found {:?})",
                target.dir
            );
        }
        (
            resolve(plugin_dir, &target.dir),
            target.world,
            None,
            false,
            target.version,
        )
    } else {
        if has_wit(&plugin_dir.join("wit")) {
            bail!(
                "plugin {name} ({language}): wit/ already exists; remove it to use \
                 attribute-generated WIT (or keep it and use wit_source = \"wit\")"
            );
        }
        let world = if csharp {
            "library".to_string()
        } else {
            plugin_dir
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(name)
                .to_string()
        };
        (
            plugin_dir.join(".owasidea").join("wit"),
            world,
            csharp.then_some(name),
            csharp,
            version.to_string(),
        )
    };
    let rendered = crate::render_wit_source(plugin_dir, &world, &version, package, wrap_interface)?
        .with_context(|| format!("no attribute markers found in {}", plugin_dir.display()))?;
    let changed = crate::write_wit(&dir, &rendered)?;
    prune_generated(&dir, &format!("{}.wit", rendered.world))?;
    Ok(Prepared {
        mode: WitSource::Attributes,
        world: rendered.world,
        dir,
        changed,
        exports: rendered.exports,
    })
}

pub fn prepare(
    plugin_dir: &Path,
    name: &str,
    language: &str,
    explicit: Option<&str>,
    version: &str,
) -> Result<Option<Prepared>> {
    if language == "rust" {
        prepare_rust(plugin_dir, name, explicit)
    } else {
        prepare_other(plugin_dir, name, language, explicit, version)
    }
}

fn prepare_rust(plugin_dir: &Path, name: &str, explicit: Option<&str>) -> Result<Option<Prepared>> {
    let requested = WitSource::parse(explicit)?;
    let target = rust_target(plugin_dir)?;
    let target_dir = resolve(plugin_dir, &target.dir);
    let rendered =
        crate::render_wit_source(plugin_dir, &target.world, &target.version, None, false)?;
    let has = has_wit(&target_dir);
    let artifact = is_artifact_dir(&target.dir);
    let mode = match requested {
        // Files under .owasidea/wit are ours: when markers exist, regenerate
        // instead of mistaking a previous build for a hand-written contract.
        WitSource::Auto if artifact && rendered.is_some() => WitSource::Attributes,
        WitSource::Auto if has => WitSource::Wit,
        WitSource::Auto if rendered.is_some() => WitSource::Attributes,
        WitSource::Auto => WitSource::Wit,
        other => other,
    };

    match mode {
        WitSource::Attributes => {
            let Some(rendered) = rendered else {
                bail!(
                    "plugin {name}: wit_source = attributes but no #[owasidea::world] or #[owasidea::export] markers were found"
                );
            };
            if !is_artifact_dir(&target.dir) {
                bail!(
                    "plugin {name}: attribute-generated WIT must be an artifact; set \
                     [package.metadata.component.target] path = \".owasidea/wit\" \
                     (found {:?})",
                    target.dir
                );
            }
            if let Some(legacy) = legacy_wit(plugin_dir, &target_dir, &rendered.world) {
                bail!(
                    "plugin {name}: {} duplicates the attribute contract; remove it or set wit_source = \"wit\"",
                    legacy.display()
                );
            }
            let changed = crate::write_wit(&target_dir, &rendered)?;
            prune_generated(&target_dir, &format!("{}.wit", rendered.world))?;
            Ok(Some(Prepared {
                mode,
                world: rendered.world,
                dir: target_dir,
                changed,
                exports: rendered.exports,
            }))
        }
        WitSource::Wit => {
            if !has {
                bail!(
                    "plugin {name}: no WIT found in {}; add #[owasidea::world]/#[owasidea::export] \
                     markers for generated WIT, or provide a WIT file",
                    target_dir.display()
                );
            }
            if let Some(rendered) = &rendered {
                let path = target_dir.join(format!("{}.wit", rendered.world));
                let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
                if !on_disk.is_empty() && on_disk != rendered.content {
                    return Err(drift_error(name, &path));
                }
            }
            Ok(Some(Prepared {
                mode,
                world: target.world,
                dir: target_dir,
                changed: false,
                exports: 0,
            }))
        }
        WitSource::Auto => unreachable!("auto is resolved above"),
    }
}

fn first_world(dir: &Path) -> Option<String> {
    let world_re = regex::Regex::new(r"world\s+([A-Za-z0-9_-]+)").unwrap();
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("wit") {
            continue;
        }
        let content = std::fs::read_to_string(path).ok()?;
        if let Some(captures) = world_re.captures(&content) {
            return Some(captures[1].to_string());
        }
    }
    None
}

fn prepare_other(
    plugin_dir: &Path,
    name: &str,
    language: &str,
    explicit: Option<&str>,
    version: &str,
) -> Result<Option<Prepared>> {
    let requested = WitSource::parse(explicit)?;
    let dir_name = plugin_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_string();
    let source_dir = plugin_dir.join("wit");
    let artifact_dir = plugin_dir.join(".owasidea").join("wit");
    let csharp = language == "csharp";
    // C# uses a fixed `library` world and the plugin name as the WIT package;
    // the generated interface keeps the componentize-dotnet shape.
    let default_world = if csharp {
        "library".to_string()
    } else {
        dir_name.clone()
    };
    let rendered = crate::render_wit_source(
        plugin_dir,
        &default_world,
        version,
        csharp.then_some(name),
        csharp,
    )?;
    let has_source = has_wit(&source_dir);
    let mode = match requested {
        WitSource::Auto if has_source => WitSource::Wit,
        WitSource::Auto if rendered.is_some() => WitSource::Attributes,
        WitSource::Auto => WitSource::Wit,
        other => other,
    };

    match mode {
        WitSource::Attributes => {
            let Some(rendered) = rendered else {
                bail!(
                    "plugin {name} ({language}): wit_source = attributes but no marker comments were found"
                );
            };
            if has_source {
                bail!(
                    "plugin {name} ({language}): a wit/ directory duplicates the attribute contract; \
                     remove it or set wit_source = \"wit\""
                );
            }
            let changed = crate::write_wit(&artifact_dir, &rendered)?;
            prune_generated(&artifact_dir, &format!("{}.wit", rendered.world))?;
            Ok(Some(Prepared {
                mode,
                world: rendered.world,
                dir: artifact_dir,
                changed,
                exports: rendered.exports,
            }))
        }
        WitSource::Wit => {
            if !has_source {
                // No contract to prepare. Go may still use a prebuilt
                // component; other toolchains fail with their own message.
                return Ok(None);
            }
            // Non-Rust toolchains keep wit/ as the source of truth; marker
            // comments feed `owasidea wit generate`, not drift checks.
            Ok(Some(Prepared {
                mode,
                world: first_world(&source_dir).unwrap_or(default_world),
                dir: source_dir,
                changed: false,
                exports: 0,
            }))
        }
        WitSource::Auto => unreachable!("auto is resolved above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("owasidea-wit-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn cargo(dir: &Path, target: &str, world: &str) {
        write(
            &dir.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"demo\"\nversion = \"0.2.0\"\n\n\
                 [package.metadata.component]\npackage = \"poc:demo\"\n\n\
                 [package.metadata.component.target]\npath = \"{target}\"\nworld = \"{world}\"\n"
            ),
        );
    }

    fn source(dir: &Path) {
        write(
            &dir.join("src/lib.rs"),
            "wit_bindgen::generate!({ world: \"demo\", path: \".owasidea/wit\" });\n\
             #[owasidea::world(\"demo\")]\nstruct C;\n\
             impl Guest for C {\n    #[owasidea::export]\n    fn start_service() -> bool { true }\n}\n",
        );
    }

    #[test]
    fn parses_sources() {
        assert_eq!(WitSource::parse(None).unwrap(), WitSource::Auto);
        assert_eq!(WitSource::parse(Some("wit")).unwrap(), WitSource::Wit);
        assert_eq!(
            WitSource::parse(Some("attributes")).unwrap(),
            WitSource::Attributes
        );
        assert!(WitSource::parse(Some("nope")).is_err());
    }

    #[test]
    fn auto_generates_into_an_artifact_dir() {
        let dir = temp("auto");
        cargo(&dir, ".owasidea/wit", "demo");
        source(&dir);
        let prepared = prepare(&dir, "demo", "rust", None, "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!(prepared.mode, WitSource::Attributes);
        assert!(prepared.changed);
        let generated = std::fs::read_to_string(dir.join(".owasidea/wit/demo.wit")).unwrap();
        assert!(generated.contains("export start-service: func() -> bool;"));
        assert_eq!(prepared.exports, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renames_prune_the_old_generated_world() {
        let dir = temp("rename");
        cargo(&dir, ".owasidea/wit", "demo");
        source(&dir);
        write(
            &dir.join(".owasidea/wit/old.wit"),
            "package poc:old@0.1.0;\n",
        );
        prepare(&dir, "demo", "rust", None, "0.1.0").unwrap();
        assert!(!dir.join(".owasidea/wit/old.wit").exists());
        assert!(dir.join(".owasidea/wit/demo.wit").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_uses_existing_wit() {
        let dir = temp("existing");
        cargo(&dir, "wit", "demo");
        write(&dir.join("wit/demo.wit"), "package poc:demo@0.1.0;\n");
        let prepared = prepare(&dir, "demo", "rust", None, "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!(prepared.mode, WitSource::Wit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attributes_reject_a_source_dir_target() {
        let dir = temp("source-target");
        cargo(&dir, "wit", "demo");
        source(&dir);
        let error = prepare(&dir, "demo", "rust", Some("attributes"), "0.1.0").unwrap_err();
        assert!(error.to_string().contains(".owasidea/wit"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_and_hand_written_contracts_cannot_coexist() {
        let dir = temp("legacy");
        cargo(&dir, ".owasidea/wit", "demo");
        source(&dir);
        write(&dir.join("wit/demo.wit"), "package poc:demo@0.1.0;\n");
        let error = prepare(&dir, "demo", "rust", None, "0.1.0").unwrap_err();
        assert!(error.to_string().contains("duplicates"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drift_between_markers_and_wit_fails() {
        let dir = temp("drift");
        cargo(&dir, "wit", "demo");
        source(&dir);
        write(
            &dir.join("wit/demo.wit"),
            "package poc:demo@0.1.0;\n\nworld demo {\n    export other: func();\n}\n",
        );
        let error = prepare(&dir, "demo", "rust", Some("wit"), "0.1.0").unwrap_err();
        assert!(error.to_string().contains("diverge"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_contract_is_an_error() {
        let dir = temp("empty");
        cargo(&dir, "wit", "demo");
        write(&dir.join("src/lib.rs"), "pub fn helper() {}\n");
        let error = prepare(&dir, "demo", "rust", None, "0.1.0").unwrap_err();
        assert!(error.to_string().contains("no WIT found"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_languages() {
        let dir = temp("lang");
        assert_eq!(detect_language(&dir), "rust");
        write(&dir.join("go.mod"), "module demo\n");
        assert_eq!(detect_language(&dir), "go");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod other_language_tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("owasidea-wit-other-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn go_markers_generate_an_artifact() {
        let dir = temp("go");
        write(
            &dir.join("main.go"),
            "// owasidea:world demo\npackage demo\n\n// owasidea:export\nfunc Greet(name string) string { return name }\n",
        );
        let prepared = prepare(&dir, "demo", "go", None, "0.1.0").unwrap().unwrap();
        assert_eq!(prepared.mode, WitSource::Attributes);
        assert!(!dir.join("wit").exists());
        let wit = std::fs::read_to_string(dir.join(".owasidea/wit/demo.wit")).unwrap();
        assert!(wit.contains("export greet: func(name: string) -> string;"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typescript_markers_generate_an_artifact() {
        let dir = temp("ts");
        write(
            &dir.join("src/index.ts"),
            "/** @owasidea-world demo */\n/** @owasidea-export */\nexport function greet(name: string): string { return name; }\n",
        );
        let prepared = prepare(&dir, "demo", "typescript", None, "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!(prepared.mode, WitSource::Attributes);
        assert!(!dir.join("wit").exists());
        let wit = std::fs::read_to_string(dir.join(".owasidea/wit/demo.wit")).unwrap();
        assert!(wit.contains("export greet: func(name: string) -> string;"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn csharp_uses_the_plugin_package_and_library_world() {
        let dir = temp("cs");
        write(
            &dir.join("DemoImpl.cs"),
            "[OwasideaWorld(\"library\")]\npublic class ApiExportsImpl {\n    [OwasideaExport]\n    public static string Greet(string name) { return name; }\n}\n",
        );
        let prepared = prepare(&dir, "demo", "csharp", None, "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!(prepared.mode, WitSource::Attributes);
        assert_eq!(prepared.world, "library");
        let wit = std::fs::read_to_string(dir.join(".owasidea/wit/library.wit")).unwrap();
        assert!(wit.contains("package poc:demo@0.1.0;"));
        assert!(wit.contains("interface api {"));
        assert!(wit.contains("greet: func(name: string) -> string;"));
        assert!(wit.contains("world library {"));
        assert!(wit.contains("export api;"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_legacy_wit_directory_blocks_attributes() {
        let dir = temp("legacy-other");
        write(
            &dir.join("main.go"),
            "// owasidea:world demo\npackage demo\n",
        );
        write(&dir.join("wit/demo.wit"), "package poc:demo@0.1.0;\n");
        let error = prepare(&dir, "demo", "go", Some("attributes"), "0.1.0").unwrap_err();
        assert!(error.to_string().contains("duplicates"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
