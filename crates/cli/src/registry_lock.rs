use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginLockEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub sha256: String,
    pub signature_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginLock {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub plugins: Vec<PluginLockEntry>,
}

fn default_version() -> u32 {
    1
}

pub fn lockfile_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .parent()
        .unwrap_or(manifest_dir)
        .join("plugins.lock")
}

pub fn read_lock(path: &Path) -> anyhow::Result<PluginLock> {
    if !path.exists() {
        return Ok(PluginLock {
            version: 1,
            plugins: Vec::new(),
        });
    }
    let content = std::fs::read_to_string(path)?;
    let lock: PluginLock = toml::from_str(&content)?;
    Ok(lock)
}

pub fn write_lock(path: &Path, lock: &PluginLock) -> anyhow::Result<()> {
    let mut lock = lock.clone();
    lock.plugins
        .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
    let content = toml::to_string_pretty(&lock)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

pub fn find_entry<'a>(lock: &'a PluginLock, name: &str) -> Option<&'a PluginLockEntry> {
    lock.plugins.iter().find(|e| e.name == name)
}
