//! RL-04: software bill of materials (SBOM) and build provenance.
//!
//! The SBOM is derived from the lockfiles in the app tree:
//!
//! - `Cargo.lock` (root, desktop shell and Rust plugins) -> `pkg:cargo/...`
//! - `package-lock.json` (root, UI and JS/TS plugins) -> `pkg:npm/...`
//! - `go.mod` + `go.sum` (Go plugins) -> `pkg:golang/...`
//!
//! Two output formats are supported: CycloneDX 1.5 JSON (default) and SPDX
//! 2.3 JSON. The document is deterministic: components are sorted by purl,
//! the serial number / namespace is derived from a content hash, and no wall
//! clock is read. When `SOURCE_DATE_EPOCH` is set it is the only timestamp
//! source; without it the timestamp is omitted (CycloneDX) or fixed to the
//! Unix epoch (SPDX, whose schema requires `created`).
//!
//! `--provenance` writes a SLSA-like in-toto statement with the build id, git
//! commit, epoch and SHA-256 hashes of the artifacts in the output directory.
//! It is unsigned and is **not** a hermetic-build attestation; the limitations
//! are embedded in the document and documented in `docs/RELEASE.md`.

use anyhow::Context;
use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Directories that never contain the app's dependency lockfiles.
const SKIP_DIRS: &[&str] = &["target", "node_modules", ".git", "dist", ".owasidea"];

/// Maximum directory depth walked from the app root.
const MAX_DEPTH: usize = 8;

const CYCLONEDX_FILE: &str = "sbom.cdx.json";
const SPDX_FILE: &str = "sbom.spdx.json";
const PROVENANCE_FILE: &str = "provenance.intoto.json";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Package {
    ecosystem: &'static str,
    name: String,
    version: String,
    /// `(algorithm, lowercase hex digest)` pairs, deduplicated.
    hashes: BTreeSet<(String, String)>,
    /// Lockfiles that mention this package, relative to the app root.
    sources: BTreeSet<String>,
}

impl Package {
    fn new(ecosystem: &'static str, name: &str, version: &str) -> Self {
        Self {
            ecosystem,
            name: name.to_string(),
            version: version.to_string(),
            hashes: BTreeSet::new(),
            sources: BTreeSet::new(),
        }
    }

    fn purl(&self) -> String {
        format!(
            "pkg:{}/{}@{}",
            self.ecosystem,
            purl_encode(&self.name, true),
            purl_encode(&self.version, false)
        )
    }
}

/// Percent-encode a purl component. `/` is kept for npm scopes and Go module
/// paths; everything outside the unreserved set is escaped.
fn purl_encode(value: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        let ch = byte as char;
        let unreserved = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '~');
        if unreserved || (keep_slash && ch == '/') {
            out.push(ch);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn base64_to_hex(value: &str) -> Option<String> {
    base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .ok()
        .map(|bytes| hex(&bytes))
}

/// Deterministic RFC 4122-shaped UUID derived from `bytes`.
fn deterministic_uuid(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&digest[..16]);
    raw[6] = (raw[6] & 0x0f) | 0x50; // version 5 (name-based)
    raw[8] = (raw[8] & 0x3f) | 0x80; // RFC 4122 variant
    let h = hex(&raw);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn rfc3339(epoch: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp(epoch as i64)
        .ok()
        .and_then(|ts| {
            ts.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

fn relative_slash(root: &Path, path: &Path) -> String {
    pathdiff::diff_paths(path, root)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Lockfile discovery and parsing
// ---------------------------------------------------------------------------

fn walk_lockfiles(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name) {
                walk_lockfiles(&path, depth + 1, out);
            }
            continue;
        }
        if matches!(name, "Cargo.lock" | "package-lock.json" | "go.mod") {
            out.push(path);
        }
    }
}

fn parse_cargo_lock(raw: &str) -> anyhow::Result<Vec<Package>> {
    let value: toml::Value = toml::from_str(raw).context("parse Cargo.lock")?;
    let mut packages = Vec::new();
    let Some(entries) = value.get("package").and_then(|value| value.as_array()) else {
        return Ok(packages);
    };
    for entry in entries {
        let (Some(name), Some(version)) = (
            entry.get("name").and_then(|value| value.as_str()),
            entry.get("version").and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        let mut package = Package::new("cargo", name, version);
        if let Some(checksum) = entry.get("checksum").and_then(|value| value.as_str()) {
            package
                .hashes
                .insert(("SHA-256".to_string(), checksum.to_ascii_lowercase()));
        }
        packages.push(package);
    }
    Ok(packages)
}

fn npm_name_from_key(key: &str) -> String {
    const MARKER: &str = "node_modules/";
    match key.rfind(MARKER) {
        Some(index) => key[index + MARKER.len()..].to_string(),
        None => key.to_string(),
    }
}

fn add_npm_integrity(package: &mut Package, entry: &Value) {
    let Some(integrity) = entry.get("integrity").and_then(|value| value.as_str()) else {
        return;
    };
    let Some(token) = integrity.split_whitespace().next() else {
        return;
    };
    let Some((algorithm, encoded)) = token.split_once('-') else {
        return;
    };
    let algorithm = match algorithm.to_ascii_lowercase().as_str() {
        "sha512" => "SHA-512",
        "sha384" => "SHA-384",
        "sha256" => "SHA-256",
        "sha1" => "SHA-1",
        _ => return,
    };
    if let Some(value) = base64_to_hex(encoded) {
        package.hashes.insert((algorithm.to_string(), value));
    }
}

fn parse_package_lock(raw: &str) -> anyhow::Result<Vec<Package>> {
    let value: Value = serde_json::from_str(raw).context("parse package-lock.json")?;
    let mut packages = Vec::new();
    if let Some(entries) = value.get("packages").and_then(|value| value.as_object()) {
        for (key, entry) in entries {
            if key.is_empty() {
                continue;
            }
            let Some(version) = entry.get("version").and_then(|value| value.as_str()) else {
                continue;
            };
            let name = entry
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| npm_name_from_key(key));
            if name.is_empty() {
                continue;
            }
            let mut package = Package::new("npm", &name, version);
            add_npm_integrity(&mut package, entry);
            packages.push(package);
        }
        return Ok(packages);
    }
    if let Some(entries) = value
        .get("dependencies")
        .and_then(|value| value.as_object())
    {
        collect_npm_v1(entries, &mut packages);
    }
    Ok(packages)
}

/// npm lockfile v1 nests dependencies instead of using a flat `packages` map.
fn collect_npm_v1(entries: &serde_json::Map<String, Value>, out: &mut Vec<Package>) {
    for (name, entry) in entries {
        if let Some(version) = entry.get("version").and_then(|value| value.as_str()) {
            let mut package = Package::new("npm", name, version);
            add_npm_integrity(&mut package, entry);
            out.push(package);
        }
        if let Some(nested) = entry
            .get("dependencies")
            .and_then(|value| value.as_object())
        {
            collect_npm_v1(nested, out);
        }
    }
}

/// Parse `require` directives from go.mod. `replace`/`exclude` are ignored.
fn parse_go_mod(raw: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut in_block = false;
    for line in raw.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if in_block {
            if line == ")" {
                in_block = false;
                continue;
            }
            if let Some(package) = parse_go_require(line) {
                packages.push(package);
            }
            continue;
        }
        if line == "require (" {
            in_block = true;
        } else if let Some(rest) = line.strip_prefix("require ") {
            if let Some(package) = parse_go_require(rest.trim()) {
                packages.push(package);
            }
        }
    }
    packages
}

fn parse_go_require(line: &str) -> Option<Package> {
    let mut fields = line.split_whitespace();
    let name = fields.next()?;
    let version = fields.next()?;
    if version.starts_with("=>") || name.starts_with("=>") {
        return None;
    }
    Some(Package::new("golang", name, version))
}

/// Map `(module, version)` to the SHA-256 digest recorded in go.sum.
fn parse_go_sum(raw: &str) -> BTreeMap<(String, String), String> {
    let mut hashes = BTreeMap::new();
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(version), Some(digest)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if version.ends_with("/go.mod") {
            continue;
        }
        let Some(encoded) = digest.strip_prefix("h1:") else {
            continue;
        };
        if let Some(value) = base64_to_hex(encoded) {
            hashes.insert((name.to_string(), version.to_string()), value);
        }
    }
    hashes
}

fn merge_package(map: &mut BTreeMap<String, Package>, mut package: Package, source: &str) {
    package.sources.insert(source.to_string());
    match map.entry(package.purl()) {
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let existing = entry.get_mut();
            existing.hashes.extend(package.hashes);
            existing.sources.extend(package.sources);
        }
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(package);
        }
    }
}

fn collect_packages(root: &Path) -> anyhow::Result<BTreeMap<String, Package>> {
    let mut lockfiles = Vec::new();
    walk_lockfiles(root, 0, &mut lockfiles);
    let mut packages = BTreeMap::new();
    for path in lockfiles {
        let source = relative_slash(root, &path);
        let raw =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let parsed = match name {
            "Cargo.lock" => parse_cargo_lock(&raw)?,
            "package-lock.json" => parse_package_lock(&raw)?,
            "go.mod" => {
                let mut parsed = parse_go_mod(&raw);
                let sum_path = path.with_file_name("go.sum");
                if let Ok(sum) = std::fs::read_to_string(&sum_path) {
                    let hashes = parse_go_sum(&sum);
                    for package in &mut parsed {
                        if let Some(value) =
                            hashes.get(&(package.name.clone(), package.version.clone()))
                        {
                            package
                                .hashes
                                .insert(("SHA-256".to_string(), value.clone()));
                        }
                    }
                }
                parsed
            }
            _ => continue,
        };
        for package in parsed {
            merge_package(&mut packages, package, &source);
        }
    }
    Ok(packages)
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

impl AppInfo {
    pub fn from_config(cfg: &crate::config::AppConfig) -> Self {
        let app = cfg.app.clone().unwrap_or_default();
        Self {
            name: app
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "owasidea-app".to_string()),
            version: app
                .version
                .filter(|version| !version.trim().is_empty())
                .unwrap_or_else(|| "0.0.0".to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SbomOptions<'a> {
    pub root: &'a Path,
    pub app: AppInfo,
    pub epoch: Option<u64>,
    pub profile: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Generated {
    pub bytes: Vec<u8>,
    pub components: usize,
}

fn cyclonedx_component(package: &Package) -> Value {
    let mut component = json!({
        "type": "library",
        "bom-ref": package.purl(),
        "name": package.name,
        "version": package.version,
        "purl": package.purl(),
    });
    if !package.hashes.is_empty() {
        component["hashes"] = Value::Array(
            package
                .hashes
                .iter()
                .map(|(algorithm, content)| json!({"alg": algorithm, "content": content}))
                .collect(),
        );
    }
    component["properties"] = Value::Array(
        package
            .sources
            .iter()
            .map(|source| json!({"name": "owasidea:lockfile", "value": source}))
            .collect(),
    );
    component
}

pub fn cyclonedx(opts: &SbomOptions<'_>) -> anyhow::Result<Generated> {
    let packages = collect_packages(opts.root)?;
    let components: Vec<Value> = packages.values().map(cyclonedx_component).collect();
    let mut metadata = json!({
        "tools": {
            "components": [{
                "type": "application",
                "name": "owasidea",
                "version": env!("CARGO_PKG_VERSION"),
            }],
        },
        "component": {
            "type": "application",
            "bom-ref": format!(
                "pkg:generic/{}@{}",
                purl_encode(&opts.app.name, true),
                purl_encode(&opts.app.version, false)
            ),
            "name": opts.app.name,
            "version": opts.app.version,
        },
    });
    if let Some(epoch) = opts.epoch {
        metadata["timestamp"] = json!(rfc3339(epoch));
    }
    if let Some(profile) = opts.profile {
        metadata["properties"] = json!([{"name": "owasidea:profile", "value": profile}]);
    }
    let body = json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": metadata,
        "components": components,
    });
    let body_bytes = serde_json::to_vec(&body)?;
    let mut document = body;
    document["serialNumber"] = json!(format!("urn:uuid:{}", deterministic_uuid(&body_bytes)));
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    Ok(Generated {
        bytes,
        components: packages.len(),
    })
}

pub fn spdx(opts: &SbomOptions<'_>) -> anyhow::Result<Generated> {
    let packages = collect_packages(opts.root)?;
    let created = opts
        .epoch
        .map(rfc3339)
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
    let purls: Vec<String> = packages.values().map(Package::purl).collect();
    let namespace_hash = sha256_hex(purls.join("\n").as_bytes());
    let namespace = format!(
        "https://spdx.org/spdxdocs/owasidea/{}-{}-{}",
        purl_encode(&opts.app.name, true),
        purl_encode(&opts.app.version, false),
        &namespace_hash[..16]
    );
    let spdx_packages: Vec<Value> = packages
        .values()
        .enumerate()
        .map(|(index, package)| {
            json!({
                "SPDXID": format!("SPDXRef-Package-{index}"),
                "name": package.name,
                "versionInfo": package.version,
                "downloadLocation": "NOASSERTION",
                "filesAnalyzed": false,
                "externalRefs": [{
                    "referenceCategory": "PACKAGE-MANAGER",
                    "referenceType": "purl",
                    "referenceLocator": package.purl(),
                }],
            })
        })
        .collect();
    let document = json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": format!("{}-{}", opts.app.name, opts.app.version),
        "documentNamespace": namespace,
        "creationInfo": {
            "created": created,
            "creators": [format!("Tool: owasidea-{}", env!("CARGO_PKG_VERSION"))],
        },
        "packages": spdx_packages,
    });
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    Ok(Generated {
        bytes,
        components: packages.len(),
    })
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ProvenanceOptions<'a> {
    pub root: &'a Path,
    pub artifacts: &'a Path,
    pub app: AppInfo,
    pub build_id: String,
    pub epoch: Option<u64>,
    pub profile: Option<&'a str>,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn walk_artifacts(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk_artifacts(&path, base, out)?;
        } else if path.is_file() {
            out.push((relative_slash(base, &path), hash_file(&path)?));
        }
    }
    Ok(())
}

pub fn provenance(opts: &ProvenanceOptions<'_>) -> anyhow::Result<Vec<u8>> {
    let mut artifacts = Vec::new();
    if opts.artifacts.is_dir() {
        walk_artifacts(opts.artifacts, opts.artifacts, &mut artifacts)?;
    }
    let subject: Vec<Value> = artifacts
        .iter()
        .map(|(name, digest)| json!({"name": name, "digest": {"sha256": digest}}))
        .collect();
    let sbom_byproducts: Vec<Value> = artifacts
        .iter()
        .filter(|(name, _)| name.ends_with(".json") && name.contains("sbom"))
        .map(|(name, digest)| json!({"name": name, "digest": {"sha256": digest}}))
        .collect();

    let git_sha = git(opts.root, &["rev-parse", "HEAD"]);
    let git_dirty = git(opts.root, &["status", "--porcelain"])
        .map(|status| !status.is_empty())
        .unwrap_or(false);

    let mut external = json!({
        "buildId": opts.build_id,
        "app": {"name": opts.app.name, "version": opts.app.version},
        "sourceDateEpoch": opts.epoch,
    });
    if let Some(profile) = opts.profile {
        external["profile"] = json!(profile);
    }
    let resolved = match &git_sha {
        Some(sha) => json!([{
            "uri": "git+https://github.com/owasidea/owasidea",
            "digest": {"gitCommit": sha},
        }]),
        None => json!([]),
    };
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": subject,
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": "https://owasidea.dev/build/v1",
                "externalParameters": external,
                "internalParameters": {
                    "gitSha": git_sha,
                    "gitDirty": git_dirty,
                    "tool": {"name": "owasidea", "version": env!("CARGO_PKG_VERSION")},
                },
                "resolvedDependencies": resolved,
            },
            "runDetails": {
                "builder": {"id": "https://owasidea.dev/cli"},
                "metadata": {"invocationId": opts.build_id},
                "byproducts": sbom_byproducts,
            },
            "owasidea": {
                "limitations": [
                    "Not a hermetic build: components come from lockfiles in the working tree, not from a resolved, isolated dependency graph.",
                    "Unsigned SLSA-like attestation: there is no builder identity, signature, or transparency log. This is not SLSA Build L3.",
                    "Artifact hashes cover the output directory at generation time; Tauri/linuxdeploy bundlers are not byte-reproducible on every host.",
                    "Timestamps derive only from SOURCE_DATE_EPOCH; without it build time is omitted to keep the document deterministic.",
                ],
            },
        },
    });
    let mut bytes = serde_json::to_vec_pretty(&statement)?;
    bytes.push(b'\n');
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn epoch_from_env() -> Option<u64> {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn write_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn resolve_dir(
    config: &Path,
    cfg: &crate::config::AppConfig,
    profile: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let root = crate::config::config_dir(config);
    crate::profile::output_dir(&root, cfg, profile)
}

pub fn default_sbom_path(
    config: &Path,
    cfg: &crate::config::AppConfig,
    profile: Option<&str>,
    format: &str,
) -> anyhow::Result<PathBuf> {
    let file = if format == "spdx" {
        SPDX_FILE
    } else {
        CYCLONEDX_FILE
    };
    Ok(resolve_dir(config, cfg, profile)?.join(file))
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_sbom(
    config: &Path,
    output: Option<&Path>,
    format: &str,
    provenance_flag: bool,
    artifacts: Option<&Path>,
    check: bool,
    profile: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    let profile = match profile {
        Some(name) => Some(crate::profile::validate_profile(name)?),
        None => None,
    };
    let cfg = crate::profile::merged_config(config, profile)?;
    let root = crate::config::config_dir(config);
    let format = match format.trim().to_ascii_lowercase().as_str() {
        "cyclonedx" | "cdx" | "cdx-json" => "cyclonedx",
        "spdx" | "spdx-json" => "spdx",
        other => anyhow::bail!("unknown SBOM format: {other} (expected cyclonedx|spdx)"),
    };
    let opts = SbomOptions {
        root: &root,
        app: AppInfo::from_config(&cfg),
        epoch: epoch_from_env(),
        profile,
    };
    let generated = if format == "spdx" {
        spdx(&opts)?
    } else {
        cyclonedx(&opts)?
    };
    let output = match output {
        Some(path) => path.to_path_buf(),
        None => default_sbom_path(config, &cfg, profile, format)?,
    };
    let digest = sha256_hex(&generated.bytes);

    if check {
        let existing = std::fs::read(&output).with_context(|| {
            format!(
                "SBOM not found at {}; run `owasidea sbom` first",
                output.display()
            )
        })?;
        if existing != generated.bytes {
            anyhow::bail!(
                "SBOM at {} is stale; regenerate it with `owasidea sbom`",
                output.display()
            );
        }
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "ok": true,
                    "sbom": output,
                    "format": format,
                    "components": generated.components,
                    "sha256": digest,
                }))?
            );
        } else {
            println!(
                "SBOM up to date: {} ({} components)",
                output.display(),
                generated.components
            );
        }
        return Ok(());
    }

    write_file(&output, &generated.bytes)?;
    let mut provenance_path = None;
    if provenance_flag {
        let artifacts_dir = artifacts
            .map(Path::to_path_buf)
            .or_else(|| output.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let path = artifacts_dir.join(PROVENANCE_FILE);
        let bytes = provenance(&ProvenanceOptions {
            root: &root,
            artifacts: &artifacts_dir,
            app: AppInfo::from_config(&cfg),
            build_id: crate::build_id::compute_build_id(),
            epoch: epoch_from_env(),
            profile,
        })?;
        write_file(&path, &bytes)?;
        provenance_path = Some(path);
    }

    if json_output {
        let mut summary = json!({
            "ok": true,
            "sbom": output,
            "format": format,
            "components": generated.components,
            "sha256": digest,
        });
        if let Some(path) = &provenance_path {
            summary["provenance"] = json!(path);
        }
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "SBOM written to {} ({format}, {} components)",
            output.display(),
            generated.components
        );
        if let Some(path) = &provenance_path {
            println!("Provenance written to {}", path.display());
        }
    }
    Ok(())
}

/// Write the SBOM files (and optional provenance) into a bundle output
/// directory. Called by `owasidea bundle` after the artifacts are copied.
pub fn write_bundle_attestations(
    root: &Path,
    cfg: &crate::config::AppConfig,
    profile: Option<&str>,
    output_dir: &Path,
    with_provenance: bool,
) -> anyhow::Result<()> {
    let opts = SbomOptions {
        root,
        app: AppInfo::from_config(cfg),
        epoch: epoch_from_env(),
        profile,
    };
    let cyclonedx = cyclonedx(&opts)?;
    let spdx = spdx(&opts)?;
    write_file(&output_dir.join(CYCLONEDX_FILE), &cyclonedx.bytes)?;
    write_file(&output_dir.join(SPDX_FILE), &spdx.bytes)?;
    println!(
        "SBOM written to {} (CycloneDX + SPDX, {} components)",
        output_dir.display(),
        cyclonedx.components
    );
    if with_provenance {
        let bytes = provenance(&ProvenanceOptions {
            root,
            artifacts: output_dir,
            app: AppInfo::from_config(cfg),
            build_id: crate::build_id::compute_build_id(),
            epoch: epoch_from_env(),
            profile,
        })?;
        write_file(&output_dir.join(PROVENANCE_FILE), &bytes)?;
        println!(
            "Provenance written to {}",
            output_dir.join(PROVENANCE_FILE).display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_LOCK: &str = r#"
version = 4

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[[package]]
name = "local-app"
version = "0.1.0"
"#;

    const PACKAGE_LOCK: &str = r#"{
      "name": "demo",
      "lockfileVersion": 3,
      "packages": {
        "": {"name": "demo", "version": "0.1.0"},
        "node_modules/@scope/pkg": {
          "version": "2.0.0",
          "integrity": "sha512-3q2+7w=="
        },
        "node_modules/plain": {"version": "1.2.3"}
      }
    }"#;

    const GO_MOD: &str = r#"
module example.com/demo

go 1.27.1

require (
	github.com/pkg/errors v0.9.1 // indirect
	golang.org/x/sys v0.37.0
)
"#;

    const GO_SUM: &str =
        "github.com/pkg/errors v0.9.1 h1:FEBLx1zS214owpjy7qsBeixbURkuhQAwrK5UwLGTwt4=\n";

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), CARGO_LOCK).unwrap();
        std::fs::write(dir.path().join("package-lock.json"), PACKAGE_LOCK).unwrap();
        std::fs::write(dir.path().join("go.mod"), GO_MOD).unwrap();
        std::fs::write(dir.path().join("go.sum"), GO_SUM).unwrap();
        dir
    }

    #[test]
    fn cyclonedx_contains_all_ecosystems() {
        let dir = fixture();
        let opts = SbomOptions {
            root: dir.path(),
            app: AppInfo {
                name: "demo".into(),
                version: "0.1.0".into(),
            },
            epoch: Some(1_700_000_000),
            profile: None,
        };
        let generated = cyclonedx(&opts).unwrap();
        assert_eq!(generated.components, 6);
        let value: Value = serde_json::from_slice(&generated.bytes).unwrap();
        assert_eq!(value["bomFormat"], "CycloneDX");
        assert_eq!(value["specVersion"], "1.5");
        let purls: Vec<&str> = value["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|component| component["purl"].as_str().unwrap())
            .collect();
        assert!(purls.contains(&"pkg:cargo/serde@1.0.0"));
        assert!(purls.contains(&"pkg:npm/%40scope/pkg@2.0.0"));
        assert!(purls.contains(&"pkg:npm/plain@1.2.3"));
        assert!(purls.contains(&"pkg:golang/github.com/pkg/errors@v0.9.1"));
        assert_eq!(value["metadata"]["timestamp"], "2023-11-14T22:13:20Z");
        let serial = value["serialNumber"].as_str().unwrap();
        assert!(serial.starts_with("urn:uuid:"));
    }

    #[test]
    fn sbom_is_deterministic_with_fixed_epoch() {
        let dir = fixture();
        let opts = SbomOptions {
            root: dir.path(),
            app: AppInfo {
                name: "demo".into(),
                version: "0.1.0".into(),
            },
            epoch: Some(1_700_000_000),
            profile: Some("prod"),
        };
        let first = cyclonedx(&opts).unwrap();
        let second = cyclonedx(&opts).unwrap();
        assert_eq!(first.bytes, second.bytes);
        let spdx_first = spdx(&opts).unwrap();
        let spdx_second = spdx(&opts).unwrap();
        assert_eq!(spdx_first.bytes, spdx_second.bytes);
    }

    #[test]
    fn spdx_is_valid_and_names_components() {
        let dir = fixture();
        let opts = SbomOptions {
            root: dir.path(),
            app: AppInfo {
                name: "demo".into(),
                version: "0.1.0".into(),
            },
            epoch: None,
            profile: None,
        };
        let generated = spdx(&opts).unwrap();
        let value: Value = serde_json::from_slice(&generated.bytes).unwrap();
        assert_eq!(value["spdxVersion"], "SPDX-2.3");
        assert_eq!(value["creationInfo"]["created"], "1970-01-01T00:00:00Z");
        assert_eq!(value["packages"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn hashes_are_carried_from_lockfiles() {
        let dir = fixture();
        let opts = SbomOptions {
            root: dir.path(),
            app: AppInfo {
                name: "demo".into(),
                version: "0.1.0".into(),
            },
            epoch: None,
            profile: None,
        };
        let generated = cyclonedx(&opts).unwrap();
        let value: Value = serde_json::from_slice(&generated.bytes).unwrap();
        let serde = value["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|component| component["purl"] == "pkg:cargo/serde@1.0.0")
            .unwrap();
        assert_eq!(serde["hashes"][0]["alg"], "SHA-256");
        let go = value["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|component| component["purl"] == "pkg:golang/github.com/pkg/errors@v0.9.1")
            .unwrap();
        assert_eq!(go["hashes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn provenance_hashes_artifacts_without_self_reference() {
        let dir = fixture();
        let artifacts = dir.path().join("dist");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("app.deb"), b"deb").unwrap();
        std::fs::write(artifacts.join(CYCLONEDX_FILE), b"{}").unwrap();
        let opts = ProvenanceOptions {
            root: dir.path(),
            artifacts: &artifacts,
            app: AppInfo {
                name: "demo".into(),
                version: "0.1.0".into(),
            },
            build_id: "abc-1700000000".into(),
            epoch: Some(1_700_000_000),
            profile: Some("prod"),
        };
        let bytes = provenance(&opts).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["_type"], "https://in-toto.io/Statement/v1");
        assert_eq!(
            value["predicate"]["buildDefinition"]["externalParameters"]["buildId"],
            "abc-1700000000"
        );
        let subjects = value["subject"].as_array().unwrap();
        assert_eq!(subjects.len(), 2);
        assert!(subjects.iter().any(|subject| subject["name"] == "app.deb"));
        assert_eq!(
            value["predicate"]["runDetails"]["byproducts"][0]["name"],
            CYCLONEDX_FILE
        );
        assert!(
            value["predicate"]["owasidea"]["limitations"]
                .as_array()
                .unwrap()
                .len()
                >= 3
        );
    }

    #[test]
    fn purl_encoding_matches_ecosystem_rules() {
        assert_eq!(purl_encode("@scope/pkg", true), "%40scope/pkg");
        assert_eq!(purl_encode("1.0.0+build", false), "1.0.0%2Bbuild");
        assert_eq!(purl_encode("golang.org/x/sys", true), "golang.org/x/sys");
    }
}
