//! Project-level build layout.
//!
//! `owasidea build` keeps generated contracts, resolved dependency packages and
//! final components under a single `.owasidea/` directory at the app root:
//!
//! ```text
//! .owasidea/
//!   deps/<namespace>/<name>/<version>/*.wit   canonical provider packages
//!   deps/providers.json                        provider -> package index
//!   build/<plugin>/<plugin>.component.wasm    final component
//!   build/<plugin>/wit/                        contract + resolved deps
//!   cache/cargo/<plugin>/                      CARGO_TARGET_DIR
//!   cache/dotnet/<plugin>/                     dotnet --artifacts-path
//! ```
//!
//! Toolchains that accept an explicit WIT directory (Go, C#, C/C++, TypeScript)
//! read `build/<plugin>/wit`; the CLI passes it through the environment. Rust
//! resolves `deps/` next to the contract named in `Cargo.toml`, so its view is
//! mirrored there. The mirror is generated and can be deleted at any time.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const DIR: &str = ".owasidea";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackageId {
    pub namespace: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl PackageId {
    /// Parses the first `package ns:name@version;` declaration in a WIT file.
    pub fn parse_wit(text: &str) -> Option<Self> {
        let re = regex::Regex::new(
            r"(?m)^\s*package\s+([A-Za-z0-9_-]+):([A-Za-z0-9_-]+)(?:@([0-9A-Za-z.+-]+))?\s*;",
        )
        .ok()?;
        let captures = re.captures(text)?;
        Some(Self {
            namespace: captures[1].to_string(),
            name: captures[2].to_string(),
            version: captures.get(3).map(|m| m.as_str().to_string()),
        })
    }

    fn is_host(&self) -> bool {
        self.namespace == "poc" && self.name == "host"
    }
}

#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    pub fn base(&self) -> PathBuf {
        self.root.join(DIR)
    }

    pub fn deps(&self) -> PathBuf {
        self.base().join("deps")
    }

    pub fn providers_file(&self) -> PathBuf {
        self.deps().join("providers.json")
    }

    pub fn package_dir(&self, package: &PackageId) -> PathBuf {
        self.deps()
            .join(&package.namespace)
            .join(&package.name)
            .join(package.version.as_deref().unwrap_or("unversioned"))
    }

    pub fn build_dir(&self, plugin: &str) -> PathBuf {
        self.base().join("build").join(plugin)
    }

    pub fn component(&self, plugin: &str) -> PathBuf {
        self.build_dir(plugin)
            .join(format!("{plugin}.component.wasm"))
    }

    pub fn client(&self, plugin: &str) -> PathBuf {
        self.build_dir(plugin).join(format!("{plugin}.client.wasm"))
    }

    pub fn wit_view(&self, plugin: &str) -> PathBuf {
        self.build_dir(plugin).join("wit")
    }

    pub fn cache(&self, tool: &str, plugin: &str) -> PathBuf {
        self.base().join("cache").join(tool).join(plugin)
    }
}

/// Reads every `*.wit` file under `dir` and groups the paths by package.
pub fn scan_packages(dir: &Path) -> BTreeMap<PackageId, Vec<PathBuf>> {
    let mut packages: BTreeMap<PackageId, Vec<PathBuf>> = BTreeMap::new();
    visit_wit(dir, &mut packages);
    packages
}

fn visit_wit(dir: &Path, packages: &mut BTreeMap<PackageId, Vec<PathBuf>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if matches!(name, "target" | "node_modules" | ".git") {
                continue;
            }
            visit_wit(&path, packages);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("wit") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(package) = PackageId::parse_wit(&text) {
                packages.entry(package).or_default().push(path);
            }
        }
    }
}

/// Reads the top-level `*.wit` files of a contract directory. `deps/` is
/// excluded: those packages belong to someone else and are published by their
/// own plugin.
pub fn scan_own_packages(dir: &Path) -> BTreeMap<PackageId, Vec<PathBuf>> {
    let mut packages: BTreeMap<PackageId, Vec<PathBuf>> = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return packages;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("wit")
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(package) = PackageId::parse_wit(&text) {
            packages.entry(package).or_default().push(path);
        }
    }
    packages
}

/// Copies the canonical packages of one provider into `.owasidea/deps`.
pub fn publish_packages(layout: &Layout, wit_dir: &Path) -> Result<Vec<PackageId>> {
    let mut published = Vec::new();
    for (package, files) in scan_own_packages(wit_dir) {
        if package.is_host() {
            continue;
        }
        let destination = layout.package_dir(&package);
        std::fs::create_dir_all(&destination)
            .with_context(|| format!("create {}", destination.display()))?;
        for file in files {
            let Some(name) = file.file_name() else {
                continue;
            };
            let mut target = destination.join(name);
            let mut copy = 1;
            while target.exists() && std::fs::read(&target).ok() != std::fs::read(&file).ok() {
                copy += 1;
                target = destination.join(format!("{}-{copy}", name.to_string_lossy()));
            }
            std::fs::copy(&file, &target)
                .with_context(|| format!("copy {} to {}", file.display(), target.display()))?;
        }
        published.push(package);
    }
    Ok(published)
}

/// Publishes the dependency packages a provider's contract needs. Packages
/// already present in the store win: an authoritative provider package must
/// never be replaced by a stale copy from another plugin's `deps/`.
pub fn publish_deps(layout: &Layout, wit_dir: &Path) -> Result<Vec<PackageId>> {
    let mut published = Vec::new();
    for (package, files) in scan_packages(&wit_dir.join("deps")) {
        let destination = layout.package_dir(&package);
        if destination.exists() {
            // `poc:host` is assembled from several plugins (host-log,
            // service-control); merge it instead of keeping a partial copy.
            if package.is_host() {
                std::fs::create_dir_all(&destination)?;
                for file in files {
                    if let Some(name) = file.file_name() {
                        std::fs::copy(&file, destination.join(name))?;
                    }
                }
            }
            published.push(package);
            continue;
        }
        std::fs::create_dir_all(&destination)
            .with_context(|| format!("create {}", destination.display()))?;
        for file in files {
            if let Some(name) = file.file_name() {
                std::fs::copy(&file, destination.join(name)).with_context(|| {
                    format!("copy {} to {}", file.display(), destination.display())
                })?;
            }
        }
        published.push(package);
    }
    Ok(published)
}

pub fn write_providers(
    layout: &Layout,
    providers: &BTreeMap<String, Vec<PackageId>>,
) -> Result<()> {
    let parent = layout.deps();
    std::fs::create_dir_all(&parent)?;
    let path = layout.providers_file();
    std::fs::write(&path, serde_json::to_string_pretty(providers)? + "\n")
        .with_context(|| format!("write {}", path.display()))
}

/// Builds `build/<plugin>/wit`: the plugin's own contract plus the packages of
/// its declared dependencies.
pub fn materialize_view(
    layout: &Layout,
    plugin: &str,
    own_wit: &Path,
    dependencies: &[String],
    providers: &BTreeMap<String, Vec<PackageId>>,
) -> Result<PathBuf> {
    let view = layout.wit_view(plugin);
    if view.exists() {
        std::fs::remove_dir_all(&view).with_context(|| format!("clear {}", view.display()))?;
    }
    std::fs::create_dir_all(&view).with_context(|| format!("create {}", view.display()))?;
    copy_wit_tree(own_wit, &view)?;

    let own_packages: BTreeSet<_> = scan_own_packages(own_wit).into_keys().collect();
    let existing = scan_packages(&view);
    let mut installed = BTreeSet::new();
    for dependency in dependencies {
        let Some(packages) = providers.get(dependency) else {
            continue;
        };
        for package in packages {
            if own_packages.contains(package) || !installed.insert(package.clone()) {
                continue;
            }
            let source = layout.package_dir(package);
            if !source.is_dir() {
                continue;
            }
            // A stale hand-copied package must not shadow the canonical store.
            if let Some(files) = existing.get(package) {
                for file in files {
                    let _ = std::fs::remove_file(file);
                }
            }
            let target = view.join("deps").join(package_dir_name(package));
            copy_wit_tree(&source, &target)?;
        }
    }
    prune_empty_dirs(&view.join("deps"));
    Ok(view)
}

/// Copies the view's dependency packages into the contract directory a
/// toolchain reads. Used by Rust and by projects that have not opted into an
/// explicit WIT directory yet.
pub fn mirror_missing(view: &Path, own_wit: &Path) -> Result<()> {
    let own_packages: BTreeSet<_> = scan_own_packages(own_wit).into_keys().collect();
    let packages = scan_packages(&view.join("deps"));
    let mut imported = imported_interfaces(own_wit);
    // A provider's own imports (for example `poc:host/host-log`) must resolve
    // too, even when the consumer's contract never mentions the interface.
    loop {
        let mut changed = false;
        for package in imported.keys().cloned().collect::<Vec<_>>() {
            let Some(files) = packages.get(&package) else {
                continue;
            };
            for file in files {
                let text = std::fs::read_to_string(file).unwrap_or_default();
                for (dependency, interface) in parse_imports(&text) {
                    changed |= imported.entry(dependency).or_default().insert(interface);
                }
            }
        }
        if !changed {
            break;
        }
    }
    let existing = scan_packages(own_wit);
    for (package, files) in &packages {
        if own_packages.contains(package) || existing.contains_key(package) {
            continue;
        }
        let Some(interfaces) = imported.get(package) else {
            continue;
        };
        let selected: Vec<&PathBuf> = files
            .iter()
            .filter(|file| {
                if interfaces.is_empty() {
                    return true;
                }
                let text = std::fs::read_to_string(file).unwrap_or_default();
                !text.contains("interface ")
                    || interfaces
                        .iter()
                        .any(|interface| text.contains(&format!("interface {interface}")))
            })
            .collect();
        if selected.is_empty() {
            continue;
        }
        // Never move an existing package: Rust projects point
        // `package.metadata.component.target.dependencies` at these paths.
        // Merge new files into the directory the project already uses.
        let target = existing
            .get(package)
            .and_then(|current| current.first())
            .and_then(|file| file.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| own_wit.join("deps").join(package_dir_name(package)));
        std::fs::create_dir_all(&target).with_context(|| format!("create {}", target.display()))?;
        for file in selected {
            if let Some(name) = file.file_name() {
                std::fs::copy(file, target.join(name))
                    .with_context(|| format!("copy {} to {}", file.display(), target.display()))?;
            }
        }
    }
    Ok(())
}

/// Interfaces imported by the plugin's own top-level WIT files.
fn imported_interfaces(dir: &Path) -> BTreeMap<PackageId, BTreeSet<String>> {
    let mut imported: BTreeMap<PackageId, BTreeSet<String>> = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return imported;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("wit")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for (package, interface) in parse_imports(&text) {
            imported.entry(package).or_default().insert(interface);
        }
    }
    imported
}

fn parse_imports(text: &str) -> Vec<(PackageId, String)> {
    let re = regex::Regex::new(
        r"import\s+([A-Za-z0-9_-]+):([A-Za-z0-9_-]+)/([A-Za-z0-9_-]+)(?:@([0-9A-Za-z.+-]+))?\s*;",
    )
    .expect("valid import regex");
    re.captures_iter(text)
        .map(|captures| {
            (
                PackageId {
                    namespace: captures[1].to_string(),
                    name: captures[2].to_string(),
                    version: captures.get(4).map(|value| value.as_str().to_string()),
                },
                captures[3].to_string(),
            )
        })
        .collect()
}

/// Imports declared by the plugin's own top-level WIT files.
pub(crate) fn imports_in(dir: &Path) -> Vec<(PackageId, String)> {
    let mut imports = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return imports;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("wit")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        imports.extend(parse_imports(&text));
    }
    imports
}

/// World declared by the plugin's own top-level WIT files.
pub(crate) fn world_name(dir: &Path) -> Option<String> {
    let re = regex::Regex::new(r"(?m)^\s*world\s+([A-Za-z0-9_-]+)").ok()?;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("wit")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        if let Some(captures) = re.captures(&text) {
            return Some(captures[1].to_string());
        }
    }
    None
}

/// Renders the C# `global using` file for a consumer's imported interfaces.
/// `using static` keeps `Greet("x")` short; duplicate interface names get an
/// alias instead to avoid ambiguity.
pub(crate) fn render_csharp_global_usings(world: &str, imports: &[(PackageId, String)]) -> String {
    let mut entries: Vec<(String, String, String)> = Vec::new();
    for (package, interface) in imports {
        if package.namespace == "wasi" {
            continue;
        }
        let valid_segment = |value: &str| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        };
        if !valid_segment(&package.namespace) || !valid_segment(&package.name) {
            continue;
        }
        let Some(version) = package.version.as_deref() else {
            continue;
        };
        let namespace = format!(
            "{}World.wit.Imports.{}.{}.v{}",
            pascal_ident(world),
            package.namespace,
            package.name,
            version
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch
                } else {
                    '_'
                })
                .collect::<String>()
        );
        let type_name = format!("I{}Imports", pascal_ident(interface));
        let alias = format!("{}{}", pascal_ident(&package.name), pascal_ident(interface));
        entries.push((namespace, type_name, alias));
    }
    entries.sort();
    entries.dedup();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, interface, _) in &entries {
        *counts.entry(interface.clone()).or_default() += 1;
    }
    let mut out = String::from("// Generated by owasidea build. Do not edit.\n");
    let mut used_aliases = BTreeSet::new();
    for (namespace, interface, alias) in entries {
        let qualified = format!("{namespace}.{interface}");
        if counts.get(&interface) == Some(&1) {
            out.push_str(&format!("global using static {qualified};\n"));
        } else {
            let mut alias = alias;
            let mut suffix = 2;
            while !used_aliases.insert(alias.clone()) {
                alias = format!(
                    "{}_{suffix}",
                    alias.trim_end_matches(|c: char| c.is_ascii_digit())
                );
                suffix += 1;
            }
            out.push_str(&format!("global using {alias} = {qualified};\n"));
        }
    }
    out
}

fn pascal_ident(name: &str) -> String {
    let mut out = String::new();
    for part in name
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|part| !part.is_empty())
    {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn prune_empty_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_empty_dirs(&path);
            if std::fs::read_dir(&path)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

fn package_dir_name(package: &PackageId) -> String {
    format!(
        "{}_{}_{}",
        package.namespace,
        package.name,
        package.version.as_deref().unwrap_or("unversioned")
    )
}

fn copy_wit_tree(source: &Path, destination: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(source) else {
        return Ok(());
    };
    std::fs::create_dir_all(destination)
        .with_context(|| format!("create {}", destination.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            copy_wit_tree(&path, &destination.join(&name))?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("wit") {
            std::fs::copy(&path, destination.join(&name))
                .with_context(|| format!("copy {} to {}", path.display(), destination.display()))?;
        }
    }
    Ok(())
}

/// Whether the project's toolchain reads an explicit WIT directory through the
/// environment or MSBuild property the CLI sets.
pub fn accepts_wit_dir(language: &str, plugin_dir: &Path) -> bool {
    match language {
        "csharp" => first_file(plugin_dir, "csproj")
            .map(|path| read(&path).contains("OwasideaWitDir"))
            .unwrap_or(false),
        "c" | "cpp" | "c++" => read(&plugin_dir.join("Makefile")).contains("WIT_DIR"),
        "typescript" | "javascript" => {
            read(&plugin_dir.join("package.json")).contains("OWASIDEA_WIT_DIR")
                || read(&plugin_dir.join("scripts").join("build.mjs")).contains("OWASIDEA_WIT_DIR")
        }
        "go" => true,
        _ => false,
    }
}

fn first_file(dir: &Path, extension: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some(extension))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versioned_and_unversioned_packages() {
        assert_eq!(
            PackageId::parse_wit("package val:provider@0.1.0;\n"),
            Some(PackageId {
                namespace: "val".into(),
                name: "provider".into(),
                version: Some("0.1.0".into()),
            })
        );
        assert_eq!(
            PackageId::parse_wit("// header\npackage val:provider;\n"),
            Some(PackageId {
                namespace: "val".into(),
                name: "provider".into(),
                version: None,
            })
        );
        assert!(PackageId::parse_wit("world x {}").is_none());
    }

    #[test]
    fn publishes_one_copy_and_dedupes_the_view() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let provider = root.join("plugins/provider/wit");
        std::fs::create_dir_all(&provider).unwrap();
        std::fs::write(
            provider.join("provider.wit"),
            "package val:provider@0.1.0;\ninterface api { f: func(); }\n",
        )
        .unwrap();

        let layout = Layout::new(root);
        let published = publish_packages(&layout, &provider).unwrap();
        assert_eq!(published.len(), 1);
        assert!(layout
            .package_dir(&published[0])
            .join("provider.wit")
            .is_file());

        let consumer = root.join("plugins/consumer/wit");
        std::fs::create_dir_all(&consumer).unwrap();
        std::fs::write(
            consumer.join("consumer.wit"),
            "package val:consumer@0.1.0;\nworld consumer { import val:provider/api@0.1.0; }\n",
        )
        .unwrap();
        let mut providers = BTreeMap::new();
        providers.insert("provider".to_string(), published);
        let view = materialize_view(
            &layout,
            "consumer",
            &consumer,
            &["provider".to_string()],
            &providers,
        )
        .unwrap();
        assert!(view.join("consumer.wit").is_file());
        assert!(view.join("deps/val_provider_0.1.0/provider.wit").is_file());

        materialize_view(
            &layout,
            "consumer",
            &consumer,
            &["provider".to_string()],
            &providers,
        )
        .unwrap();
        assert!(view.join("deps/val_provider_0.1.0/provider.wit").is_file());
    }

    #[test]
    fn renders_csharp_global_usings_for_unique_and_duplicate_interfaces() {
        let provider = PackageId {
            namespace: "poc".into(),
            name: "test".into(),
            version: Some("0.1.0".into()),
        };
        let rendered =
            render_csharp_global_usings("test-2", &[(provider.clone(), "api".to_string())]);
        assert!(rendered
            .contains("global using static Test2World.wit.Imports.poc.test.v0_1_0.IApiImports;"));

        let core = PackageId {
            namespace: "shipyard".into(),
            name: "core".into(),
            version: Some("0.1.0".into()),
        };
        let audit = PackageId {
            namespace: "shipyard".into(),
            name: "audit".into(),
            version: Some("0.1.0".into()),
        };
        let rendered = render_csharp_global_usings(
            "glue",
            &[(core, "api".to_string()), (audit, "api".to_string())],
        );
        assert!(rendered.contains(
            "global using CoreApi = GlueWorld.wit.Imports.shipyard.core.v0_1_0.IApiImports;"
        ));
        assert!(rendered.contains(
            "global using AuditApi = GlueWorld.wit.Imports.shipyard.audit.v0_1_0.IApiImports;"
        ));
        assert!(!rendered.contains("global using static"));
    }

    #[test]
    fn finds_world_and_imports_in_source_wit() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("test.wit"),
            "package poc:test@0.1.0;\nworld test-2 { import poc:test/api@0.1.0; }\n",
        )
        .unwrap();
        assert_eq!(world_name(temp.path()).as_deref(), Some("test-2"));
        let imports = imports_in(temp.path());
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].1, "api");
    }
}
