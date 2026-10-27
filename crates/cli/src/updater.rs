//! Tauri updater support (RL-02).
//!
//! Derives the updater configuration from `wasm-app.toml` (with environment
//! overrides), renders the `plugins.updater` block into
//! `tauri.generated.conf.json`, and generates `latest.json` from signed bundle
//! artifacts.
//!
//! Environment overrides (never committed, never written to the repo):
//!
//! - `OWASIDEA_UPDATE_CHANNEL` — `stable` (default) or `beta`.
//! - `OWASIDEA_UPDATER_ENDPOINT` — comma-separated endpoint URL(s); overrides
//!   `[updater].endpoint`.
//! - `OWASIDEA_UPDATER_PUBKEY` — inline minisign public key or a path to it.
//! - `OWASIDEA_UPDATER_BASE_URL` — base URL used for artifact `url` fields.
//! - `TAURI_SIGNING_PRIVATE_KEY` — enables `createUpdaterArtifacts` and is read
//!   by the Tauri CLI only; it is never copied into a generated file.
//!
//! The private key is never read or written by this module.

use std::path::{Path, PathBuf};

use crate::config::AppConfig;

/// Supported update channels.
pub const CHANNELS: [&str; 2] = ["stable", "beta"];

/// Environment variable names documented in `docs/UPDATER.md`.
pub const ENV_CHANNEL: &str = "OWASIDEA_UPDATE_CHANNEL";
pub const ENV_ENDPOINT: &str = "OWASIDEA_UPDATER_ENDPOINT";
pub const ENV_PUBKEY: &str = "OWASIDEA_UPDATER_PUBKEY";
pub const ENV_BASE_URL: &str = "OWASIDEA_UPDATER_BASE_URL";
pub const ENV_SIGNING_KEY: &str = "TAURI_SIGNING_PRIVATE_KEY";

/// Effective updater settings for a build or a `latest.json` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub channel: String,
    pub endpoints: Vec<String>,
    pub pubkey: String,
    pub pubkey_path: Option<PathBuf>,
    pub notes: Option<String>,
}

/// Validate a channel name.
pub fn validate_channel(channel: &str) -> anyhow::Result<String> {
    let channel = channel.trim();
    if channel.is_empty() {
        anyhow::bail!(
            "update channel must not be empty (expected one of: {})",
            CHANNELS.join(", ")
        );
    }
    if !CHANNELS.contains(&channel) {
        anyhow::bail!(
            "unknown update channel {:?}; expected one of: {}",
            channel,
            CHANNELS.join(", ")
        );
    }
    Ok(channel.to_string())
}

/// Validate an endpoint URL or template. HTTPS is required except for loopback
/// hosts, which are only meant for local testing (`docs/UPDATER.md`).
pub fn validate_endpoint(endpoint: &str) -> anyhow::Result<()> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        anyhow::bail!("updater.endpoint must not be empty");
    }
    let probe = endpoint
        .replace("{{channel}}", "stable")
        .replace("{{target}}", "linux-x86_64")
        .replace("{{arch}}", "x86_64")
        .replace("{{current_version}}", "0.0.0");
    let parsed = url::Url::parse(&probe)
        .map_err(|e| anyhow::anyhow!("invalid updater.endpoint {endpoint:?}: {e}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(parsed.host_str()) => Ok(()),
        "http" => anyhow::bail!(
            "updater.endpoint must use https (got {endpoint:?}); plain http is only allowed for loopback hosts"
        ),
        other => anyhow::bail!(
            "unsupported updater.endpoint scheme {other:?}; expected https (or http on loopback)"
        ),
    }
}

fn is_loopback(host: Option<&str>) -> bool {
    matches!(
        host,
        Some("127.0.0.1") | Some("localhost") | Some("[::1]") | Some("::1")
    )
}

/// Replace the updater endpoint template variables that the Tauri plugin does
/// not handle itself (`{{channel}}`), then normalize a trailing `/` to
/// `latest.json`.
pub fn render_endpoint(template: &str, channel: &str) -> String {
    let rendered = template.trim().replace("{{channel}}", channel);
    if rendered.ends_with('/') {
        format!("{rendered}latest.json")
    } else {
        rendered
    }
}

fn read_key(path: &Path) -> anyhow::Result<String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read updater public key {}: {e}", path.display()))?;
    let key = raw.trim().to_string();
    if key.is_empty() {
        anyhow::bail!("updater public key {} is empty", path.display());
    }
    Ok(key)
}

/// Resolve the effective updater settings.
///
/// Returns `Ok(None)` when the updater is disabled or not configured (no
/// endpoint and/or no public key). Callers that require a configured updater
/// (for example `owasidea latest`) turn `None` into an actionable error.
pub fn resolve(root: &Path, cfg: &AppConfig) -> anyhow::Result<Option<Settings>> {
    let section = cfg.updater.as_ref();
    if section.and_then(|s| s.enabled) == Some(false) {
        return Ok(None);
    }

    let channel = std::env::var(ENV_CHANNEL)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| section.and_then(|s| s.channel.clone()))
        .unwrap_or_else(|| "stable".to_string());
    let channel = validate_channel(&channel)?;

    let endpoint_override = std::env::var(ENV_ENDPOINT)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let endpoints: Vec<String> = match endpoint_override {
        Some(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| render_endpoint(value, &channel))
            .collect(),
        None => section
            .and_then(|s| s.endpoint.clone())
            .map(|value| vec![render_endpoint(&value, &channel)])
            .unwrap_or_default(),
    };
    for endpoint in &endpoints {
        validate_endpoint(endpoint)?;
    }

    let (pubkey, pubkey_path) = match std::env::var(ENV_PUBKEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        Some(value) if value.contains("untrusted comment:") => (value.trim().to_string(), None),
        Some(value) => {
            let path = crate::config_render::safe_resolve(root, value.trim())?;
            (read_key(&path)?, Some(path))
        }
        None => match section.and_then(|s| s.pubkey.clone()) {
            Some(value) => {
                let path = crate::config_render::safe_resolve(root, &value)?;
                (read_key(&path)?, Some(path))
            }
            None => (String::new(), None),
        },
    };

    let notes = section.and_then(|s| {
        s.notes.clone().or_else(|| {
            s.notes_file.as_ref().and_then(|path| {
                let resolved = crate::config_render::safe_resolve(root, path).ok()?;
                std::fs::read_to_string(resolved).ok()
            })
        })
    });

    if endpoints.is_empty() || pubkey.is_empty() {
        return Ok(None);
    }

    Ok(Some(Settings {
        channel,
        endpoints,
        pubkey,
        pubkey_path,
        notes,
    }))
}

/// `plugins.updater` block for `tauri.generated.conf.json`.
///
/// Loopback (http) endpoints also enable `dangerousInsecureTransportProtocol`
/// so a local static server can be used for testing; release endpoints are
/// always https.
pub fn tauri_plugin_config(settings: &Settings) -> serde_json::Value {
    let insecure = settings
        .endpoints
        .iter()
        .any(|endpoint| endpoint.starts_with("http://") && validate_endpoint(endpoint).is_ok());
    serde_json::json!({
        "endpoints": settings.endpoints,
        "pubkey": settings.pubkey,
        "dangerousInsecureTransportProtocol": insecure,
    })
}

/// True when the Tauri CLI has a signing key available. Only the presence is
/// checked; the value is never read into generated files or logs.
pub fn updater_signing_available() -> bool {
    std::env::var(ENV_SIGNING_KEY)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// One artifact entry for `latest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub platform: String,
    pub file: String,
    pub signature: String,
}

/// Map an updater artifact file name to a Tauri target key.
///
/// Returns `None` for artifacts the Tauri updater cannot install
/// (`deb`, `rpm`, `dmg`, installers without a signature, ...).
pub fn platform_for_file(file: &str) -> Option<String> {
    let arch = if file.contains("aarch64") || file.contains("arm64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".appimage") {
        Some(format!("linux-{arch}"))
    } else if lower.ends_with(".app.tar.gz") || lower.ends_with(".app.tar.xz") {
        Some(format!("darwin-{arch}"))
    } else if lower.ends_with(".exe") || lower.ends_with(".msi") {
        Some(format!("windows-{arch}"))
    } else {
        None
    }
}

/// Percent-encode a path segment (file name) for use in a URL.
pub fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_'
                    | b'.'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'@'
            );
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Scan an artifact directory for `<artifact>.sig` pairs.
///
/// `prefer` can contain platform keys that win over a later duplicate (for
/// example `windows-x86_64` from NSIS over MSI). Duplicates are resolved by
/// sorted file name so the output is deterministic.
pub fn collect_artifacts(dir: &Path, prefer: &[&str]) -> anyhow::Result<Vec<Artifact>> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_sig_files(dir, &mut files)?;
    files.sort();
    let mut artifacts: Vec<Artifact> = Vec::new();
    for sig in files {
        let Some(file) = sig.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(artifact_name) = file.strip_suffix(".sig") else {
            continue;
        };
        let Some(platform) = platform_for_file(artifact_name) else {
            continue;
        };
        let artifact_path = sig.with_file_name(artifact_name);
        if !artifact_path.is_file() {
            continue;
        }
        let signature = std::fs::read_to_string(&sig)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", sig.display()))?
            .trim()
            .to_string();
        if signature.is_empty() {
            anyhow::bail!("update signature {} is empty", sig.display());
        }
        if artifacts.iter().any(|a| a.platform == platform) {
            if prefer.contains(&platform.as_str()) && artifact_name.ends_with(".exe") {
                artifacts.retain(|a| a.platform != platform);
            } else {
                continue;
            }
        }
        artifacts.push(Artifact {
            platform,
            file: artifact_name.to_string(),
            signature,
        });
    }
    artifacts.sort_by(|a, b| a.platform.cmp(&b.platform));
    Ok(artifacts)
}

fn collect_sig_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_sig_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("sig") {
            out.push(path);
        }
    }
    Ok(())
}

/// Build the static-format `latest.json` document.
pub fn latest_json(
    version: &str,
    channel: &str,
    notes: Option<&str>,
    pub_date: &str,
    url_base: &str,
    artifacts: &[Artifact],
) -> anyhow::Result<serde_json::Value> {
    let channel = validate_channel(channel)?;
    if version.trim().is_empty() {
        anyhow::bail!("latest.json version must not be empty");
    }
    semver::Version::parse(version.trim())
        .map_err(|e| anyhow::anyhow!("latest.json version {version:?} is not valid semver: {e}"))?;
    let base = url_base.trim().trim_end_matches('/');
    validate_endpoint(&format!("{base}/latest.json"))?;
    if artifacts.is_empty() {
        anyhow::bail!(
            "no updater artifacts with signatures found; build with a signing key or pass --artifacts"
        );
    }
    let mut platforms = serde_json::Map::new();
    for artifact in artifacts {
        let url = format!("{base}/{}", encode_segment(&artifact.file));
        platforms.insert(
            artifact.platform.clone(),
            serde_json::json!({
                "signature": artifact.signature,
                "url": url,
            }),
        );
    }
    Ok(serde_json::json!({
        "version": version.trim(),
        "notes": notes.unwrap_or(""),
        "pub_date": pub_date,
        "channel": channel,
        "platforms": platforms,
    }))
}

/// Validate a parsed `latest.json` document.
pub fn validate_latest(value: &serde_json::Value) -> anyhow::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("latest.json must be a JSON object"))?;
    let version = object
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("latest.json is missing a string `version`"))?;
    semver::Version::parse(version)
        .map_err(|e| anyhow::anyhow!("latest.json version {version:?} is not semver: {e}"))?;
    let pub_date = object
        .get("pub_date")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("latest.json is missing a string `pub_date`"))?;
    time::OffsetDateTime::parse(pub_date, &time::format_description::well_known::Rfc3339)
        .map_err(|e| anyhow::anyhow!("latest.json pub_date {pub_date:?} is not RFC 3339: {e}"))?;
    if let Some(channel) = object.get("channel").and_then(|v| v.as_str()) {
        validate_channel(channel)?;
    }
    let platforms = object
        .get("platforms")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("latest.json is missing a `platforms` object"))?;
    if platforms.is_empty() {
        anyhow::bail!("latest.json `platforms` must not be empty");
    }
    for (platform, entry) in platforms {
        let url = entry
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("latest.json platform {platform} is missing `url`"))?;
        validate_endpoint(url)?;
        let signature = entry
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("latest.json platform {platform} is missing `signature`")
            })?;
        if signature.trim().is_empty() {
            anyhow::bail!("latest.json platform {platform} has an empty `signature`");
        }
    }
    Ok(())
}

/// Base URL for artifact downloads: explicit override, env, or the parent of
/// the first update endpoint.
pub fn url_base(settings: &Settings, explicit: Option<&str>) -> anyhow::Result<String> {
    if let Some(base) = explicit.filter(|value| !value.trim().is_empty()) {
        return Ok(base.trim().trim_end_matches('/').to_string());
    }
    if let Ok(base) = std::env::var(ENV_BASE_URL) {
        if !base.trim().is_empty() {
            return Ok(base.trim().trim_end_matches('/').to_string());
        }
    }
    let endpoint = settings
        .endpoints
        .first()
        .ok_or_else(|| anyhow::anyhow!("no updater endpoint configured; pass --url-base"))?;
    let parsed = url::Url::parse(endpoint)
        .map_err(|e| anyhow::anyhow!("invalid updater endpoint {endpoint:?}: {e}"))?;
    let mut base = parsed.clone();
    base.set_query(None);
    base.set_fragment(None);
    let path = parsed.path().trim_end_matches('/');
    let parent = path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    base.set_path(if parent.is_empty() { "/" } else { parent });
    Ok(base.to_string().trim_end_matches('/').to_string())
}

/// Current UTC time in RFC 3339, used for `pub_date`.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Write `latest.json` (pretty, trailing newline) atomically.
pub fn write_latest(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    validate_latest(value)?;
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    crate::config_render::atomic_write(path, &bytes)
}

/// `owasidea latest` implementation.
#[allow(clippy::too_many_arguments)]
pub fn cmd_latest(
    config: &Path,
    profile: Option<&str>,
    artifacts: Option<&Path>,
    output: Option<&Path>,
    url_base_override: Option<&str>,
    channel_override: Option<&str>,
    version_override: Option<&str>,
    notes_override: Option<&str>,
    notes_file: Option<&Path>,
    pub_date_override: Option<&str>,
    check: bool,
    json: bool,
) -> anyhow::Result<()> {
    let profile = match profile {
        Some(name) => Some(crate::profile::validate_profile(name)?),
        None => None,
    };
    let cfg = crate::profile::merged_config(config, profile)?;
    let root = crate::config::config_dir(config);

    let settings = resolve(&root, &cfg)?;
    let artifacts_dir = match artifacts {
        Some(dir) => crate::config_render::safe_resolve(&root, &dir.to_string_lossy())?,
        None => crate::profile::output_dir(&root, &cfg, profile)?,
    };
    let output = match output {
        Some(path) => crate::config_render::safe_resolve(&root, &path.to_string_lossy())?,
        None => artifacts_dir.join("latest.json"),
    };

    let channel = match channel_override {
        Some(channel) => validate_channel(channel)?,
        None => match &settings {
            Some(settings) => settings.channel.clone(),
            None => {
                let from_env = std::env::var(ENV_CHANNEL).unwrap_or_else(|_| "stable".to_string());
                validate_channel(&from_env)?
            }
        },
    };

    if check {
        let raw = std::fs::read_to_string(&output).map_err(|e| {
            anyhow::anyhow!(
                "cannot read {} for --check: {e} (run without --check to generate it)",
                output.display()
            )
        })?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", output.display()))?;
        validate_latest(&value)?;
        if let Some(settings) = &settings {
            if value.get("channel").and_then(|v| v.as_str()) != Some(settings.channel.as_str()) {
                anyhow::bail!(
                    "{} channel does not match the effective channel {}",
                    output.display(),
                    settings.channel
                );
            }
        }
        if json {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!("latest.json OK: {}", output.display());
        }
        return Ok(());
    }

    let settings = settings.ok_or_else(|| {
        anyhow::anyhow!(
            "updater is not configured; set [updater].endpoint and [updater].pubkey in {} (or {ENV_ENDPOINT}/{ENV_PUBKEY})",
            config.display()
        )
    })?;

    let artifacts = collect_artifacts(&artifacts_dir, &["windows-x86_64", "windows-aarch64"])?;
    if artifacts.is_empty() {
        anyhow::bail!(
            "no updater artifacts (*.AppImage.sig, *.exe.sig, *.msi.sig, *.app.tar.gz.sig) found under {}; run `owasidea bundle` with {ENV_SIGNING_KEY} set",
            artifacts_dir.display()
        );
    }
    let version = match version_override {
        Some(version) => version.trim().to_string(),
        None => cfg
            .app
            .as_ref()
            .and_then(|app| app.version.clone())
            .unwrap_or_else(|| "0.1.0".to_string()),
    };
    let notes =
        match (notes_override, notes_file) {
            (Some(notes), _) => Some(notes.to_string()),
            (None, Some(path)) => {
                let path = crate::config_render::safe_resolve(&root, &path.to_string_lossy())?;
                Some(std::fs::read_to_string(&path).map_err(|e| {
                    anyhow::anyhow!("cannot read notes file {}: {e}", path.display())
                })?)
            }
            (None, None) => settings.notes.clone(),
        };
    let pub_date = match pub_date_override {
        Some(date) => date.to_string(),
        None => now_rfc3339(),
    };
    let base = url_base(&settings, url_base_override)?;
    let value = latest_json(
        &version,
        &channel,
        notes.as_deref(),
        &pub_date,
        &base,
        &artifacts,
    )?;
    write_latest(&output, &value)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "latest.json: {} (channel {channel}, {} platform(s), base {base})",
            output.display(),
            value["platforms"].as_object().map(|p| p.len()).unwrap_or(0)
        );
        for artifact in &artifacts {
            println!("  {} -> {}", artifact.platform, artifact.file);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "owasidea-updater-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn channels_are_validated() {
        assert_eq!(validate_channel("stable").unwrap(), "stable");
        assert_eq!(validate_channel(" beta ").unwrap(), "beta");
        assert!(validate_channel("nightly").is_err());
        assert!(validate_channel("").is_err());
    }

    #[test]
    fn endpoints_require_https_except_loopback() {
        assert!(validate_endpoint("https://updates.example.com/latest.json").is_ok());
        assert!(validate_endpoint("https://updates.example.com/{{channel}}/latest.json").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:9000/latest.json").is_ok());
        assert!(validate_endpoint("http://updates.example.com/latest.json").is_err());
        assert!(validate_endpoint("ftp://updates.example.com/latest.json").is_err());
    }

    #[test]
    fn endpoint_template_renders_channel() {
        assert_eq!(
            render_endpoint("https://u.example.com/{{channel}}/latest.json", "beta"),
            "https://u.example.com/beta/latest.json"
        );
        assert_eq!(
            render_endpoint("https://u.example.com/beta/", "beta"),
            "https://u.example.com/beta/latest.json"
        );
    }

    #[test]
    fn platform_keys_cover_supported_artifacts() {
        assert_eq!(
            platform_for_file("Shipyard_0.1.0_amd64.AppImage").as_deref(),
            Some("linux-x86_64")
        );
        assert_eq!(
            platform_for_file("Shipyard_0.1.0_aarch64.AppImage").as_deref(),
            Some("linux-aarch64")
        );
        assert_eq!(
            platform_for_file("Shipyard_0.1.0_x64-setup.exe").as_deref(),
            Some("windows-x86_64")
        );
        assert_eq!(
            platform_for_file("Shipyard_0.1.0_aarch64.app.tar.gz").as_deref(),
            Some("darwin-aarch64")
        );
        assert_eq!(platform_for_file("shipyard_0.1.0_amd64.deb"), None);
        assert_eq!(platform_for_file("shipyard-0.1.0-1.x86_64.rpm"), None);
    }

    #[test]
    fn segments_are_percent_encoded() {
        assert_eq!(
            encode_segment("Shipyard_0.1.0_amd64.AppImage"),
            "Shipyard_0.1.0_amd64.AppImage"
        );
        assert_eq!(encode_segment("a b.AppImage"), "a%20b.AppImage");
        assert_eq!(encode_segment("a+b.AppImage"), "a+b.AppImage");
    }

    #[test]
    fn collects_signed_artifacts_and_skips_unsigned() {
        let dir = temp_dir("collect");
        std::fs::write(dir.join("App.AppImage"), b"bin").unwrap();
        std::fs::write(dir.join("App.AppImage.sig"), "sig-appimage\n").unwrap();
        std::fs::write(dir.join("App_amd64.deb"), b"deb").unwrap();
        std::fs::write(dir.join("App_amd64.deb.sig"), "sig-deb\n").unwrap();
        std::fs::write(dir.join("App-setup.exe"), b"exe").unwrap();
        std::fs::write(dir.join("App-setup.exe.sig"), "sig-exe\n").unwrap();
        std::fs::write(dir.join("orphan.sig"), "sig\n").unwrap();
        let artifacts = collect_artifacts(&dir, &[]).unwrap();
        let keys: Vec<&str> = artifacts.iter().map(|a| a.platform.as_str()).collect();
        assert_eq!(keys, vec!["linux-x86_64", "windows-x86_64"]);
        assert_eq!(artifacts[0].signature, "sig-appimage");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_nsis_exe_over_msi_for_windows() {
        let dir = temp_dir("prefer");
        std::fs::write(dir.join("App_0.1.0_x64_en-US.msi"), b"msi").unwrap();
        std::fs::write(dir.join("App_0.1.0_x64_en-US.msi.sig"), "sig-msi").unwrap();
        std::fs::write(dir.join("App_0.1.0_x64-setup.exe"), b"exe").unwrap();
        std::fs::write(dir.join("App_0.1.0_x64-setup.exe.sig"), "sig-exe").unwrap();
        let artifacts = collect_artifacts(&dir, &["windows-x86_64"]).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].file.ends_with(".exe"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn latest_json_has_tauri_static_format() {
        let artifacts = vec![Artifact {
            platform: "linux-x86_64".to_string(),
            file: "App 0.1.0.AppImage".to_string(),
            signature: "sig".to_string(),
        }];
        let value = latest_json(
            "1.2.3",
            "beta",
            Some("notes"),
            "2026-10-27T00:00:00Z",
            "https://updates.example.com/beta",
            &artifacts,
        )
        .unwrap();
        validate_latest(&value).unwrap();
        assert_eq!(value["version"], "1.2.3");
        assert_eq!(value["channel"], "beta");
        assert_eq!(
            value["platforms"]["linux-x86_64"]["url"],
            "https://updates.example.com/beta/App%200.1.0.AppImage"
        );
        assert_eq!(value["platforms"]["linux-x86_64"]["signature"], "sig");
    }

    #[test]
    fn latest_json_rejects_bad_input() {
        let artifacts = vec![Artifact {
            platform: "linux-x86_64".to_string(),
            file: "App.AppImage".to_string(),
            signature: "sig".to_string(),
        }];
        assert!(latest_json(
            "not-semver",
            "stable",
            None,
            "2026-10-27T00:00:00Z",
            "https://u.example.com",
            &artifacts
        )
        .is_err());
        assert!(latest_json(
            "1.0.0",
            "stable",
            None,
            "2026-10-27T00:00:00Z",
            "http://u.example.com",
            &artifacts
        )
        .is_err());
        assert!(latest_json(
            "1.0.0",
            "nightly",
            None,
            "2026-10-27T00:00:00Z",
            "https://u.example.com",
            &artifacts
        )
        .is_err());
    }

    #[test]
    fn validate_latest_catches_missing_fields() {
        let mut value = serde_json::json!({
            "version": "1.0.0",
            "notes": "",
            "pub_date": "2026-10-27T00:00:00Z",
            "platforms": {"linux-x86_64": {"signature": "sig", "url": "https://u.example.com/a.AppImage"}}
        });
        validate_latest(&value).unwrap();
        value["platforms"]["linux-x86_64"]
            .as_object_mut()
            .unwrap()
            .remove("signature");
        assert!(validate_latest(&value).is_err());
        value["platforms"] = serde_json::json!({});
        assert!(validate_latest(&value).is_err());
    }

    #[test]
    fn url_base_defaults_to_endpoint_parent() {
        let settings = Settings {
            channel: "stable".to_string(),
            endpoints: vec!["https://updates.example.com/stable/latest.json".to_string()],
            pubkey: "key".to_string(),
            pubkey_path: None,
            notes: None,
        };
        assert_eq!(
            url_base(&settings, None).unwrap(),
            "https://updates.example.com/stable"
        );
        assert_eq!(
            url_base(&settings, Some("https://cdn.example.com/")).unwrap(),
            "https://cdn.example.com"
        );
    }

    #[test]
    fn tauri_plugin_config_marks_loopback_insecure() {
        let settings = Settings {
            channel: "stable".to_string(),
            endpoints: vec!["http://127.0.0.1:9000/latest.json".to_string()],
            pubkey: "key".to_string(),
            pubkey_path: None,
            notes: None,
        };
        let value = tauri_plugin_config(&settings);
        assert_eq!(value["dangerousInsecureTransportProtocol"], true);
        assert_eq!(value["pubkey"], "key");
    }

    #[test]
    fn now_rfc3339_is_parseable() {
        let now = now_rfc3339();
        time::OffsetDateTime::parse(&now, &time::format_description::well_known::Rfc3339).unwrap();
    }
}
