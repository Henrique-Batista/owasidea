//! Tauri updater integration (RL-02).
//!
//! The plugin is always registered, but it is only usable when
//! `plugins.updater` is rendered from `[updater]` (see `owasidea bundle` and
//! `docs/UPDATER.md`) or when the environment overrides below are set. The
//! private signing key is never read here; only the Tauri CLI uses it.
//!
//! Environment overrides:
//!
//! - `OWASIDEA_UPDATE_CHANNEL` — reported channel (`stable` by default).
//! - `OWASIDEA_UPDATER_ENDPOINT` — comma-separated endpoint URL(s); overrides
//!   the build-time endpoints (used to point at a local static server).
//! - `OWASIDEA_UPDATER_PUBKEY` — inline minisign public key or a path to it.

use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::UpdaterExt;

pub const ENV_CHANNEL: &str = "OWASIDEA_UPDATE_CHANNEL";
pub const ENV_ENDPOINT: &str = "OWASIDEA_UPDATER_ENDPOINT";
pub const ENV_PUBKEY: &str = "OWASIDEA_UPDATER_PUBKEY";

/// Result of an update check, returned to the UI/CLI bridge.
#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
    pub download_url: Option<String>,
    pub channel: String,
}

/// Active update channel (stable/beta).
pub fn channel() -> String {
    std::env::var(ENV_CHANNEL)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "stable".to_string())
}

fn env_endpoints() -> Result<Option<Vec<tauri::Url>>, String> {
    let Some(raw) = std::env::var(ENV_ENDPOINT)
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let mut endpoints = Vec::new();
    for value in raw.split(',') {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let url = tauri::Url::parse(value)
            .map_err(|error| format!("invalid {ENV_ENDPOINT} {value:?}: {error}"))?;
        endpoints.push(url);
    }
    Ok((!endpoints.is_empty()).then_some(endpoints))
}

fn env_pubkey() -> Option<String> {
    let raw = std::env::var(ENV_PUBKEY).ok()?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    if raw.contains("untrusted comment:") {
        return Some(raw);
    }
    std::fs::read_to_string(&raw)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn build_updater<R: Runtime>(app: &AppHandle<R>) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder();
    if let Some(endpoints) = env_endpoints()? {
        builder = builder
            .endpoints(endpoints)
            .map_err(|error| format!("invalid updater endpoints: {error}"))?;
    }
    if let Some(pubkey) = env_pubkey() {
        builder = builder.pubkey(pubkey);
    }
    builder
        .build()
        .map_err(|error| format!("updater is not configured: {error}"))
}

async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<UpdateInfo, String> {
    let updater = build_updater(app)?;
    let current_version = app.package_info().version.to_string();
    let channel = channel();
    match updater.check().await.map_err(|error| error.to_string())? {
        Some(update) => Ok(UpdateInfo {
            current_version,
            available: true,
            version: Some(update.version),
            notes: update.body,
            pub_date: update.date.map(|date| date.to_string()),
            download_url: Some(update.download_url.to_string()),
            channel,
        }),
        None => Ok(UpdateInfo {
            current_version,
            available: false,
            version: None,
            notes: None,
            pub_date: None,
            download_url: None,
            channel,
        }),
    }
}

/// Check the configured endpoint for an update without installing it.
#[tauri::command]
pub async fn updater_check(app: AppHandle) -> Result<UpdateInfo, String> {
    check(&app).await
}

/// Download and install the announced update, then restart the app.
///
/// The artifact signature is verified by the plugin against the configured
/// public key before anything is installed.
#[tauri::command]
pub async fn updater_install(app: AppHandle) -> Result<UpdateInfo, String> {
    let updater = build_updater(&app)?;
    let current_version = app.package_info().version.to_string();
    let channel = channel();
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(UpdateInfo {
            current_version,
            available: false,
            version: None,
            notes: None,
            pub_date: None,
            download_url: None,
            channel,
        });
    };
    let info = UpdateInfo {
        current_version,
        available: true,
        version: Some(update.version.clone()),
        notes: update.body.clone(),
        pub_date: update.date.map(|date| date.to_string()),
        download_url: Some(update.download_url.to_string()),
        channel,
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| format!("update failed: {error}"))?;
    eprintln!(
        "updater: installed {} (channel {}); restarting",
        info.version.as_deref().unwrap_or("unknown"),
        info.channel
    );
    app.restart();
}
