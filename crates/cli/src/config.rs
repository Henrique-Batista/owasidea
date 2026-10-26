use std::path::{Path, PathBuf};

#[derive(serde::Deserialize, Debug, Default)]
pub struct AppConfig {
    pub app: Option<AppSection>,
    pub host: Option<HostSection>,
    pub desktop: Option<DesktopSection>,
    pub signing: Option<SigningSection>,
    pub test: Option<TestSection>,
    pub smoke: Option<SmokeSection>,
    pub plugins: Option<std::collections::BTreeMap<String, PluginSection>>,
    pub dev: Option<DevSection>,
}

impl AppConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(host) = &self.host {
            if let Some(addr) = &host.addr {
                validate_socket_addr(addr)?;
            }
        }
        if let Some(dev) = &self.dev {
            if let Some(addr) = &dev.ui_addr {
                validate_socket_addr(addr)?;
            }
        }
        Ok(())
    }
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct HostSection {
    #[serde(default)]
    pub addr: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct AppSection {
    pub version: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct DesktopSection {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub ui: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct DevSection {
    #[serde(default = "default_true")]
    pub watch: bool,
    #[serde(default = "default_true")]
    pub hot_reload: bool,
    #[serde(default)]
    pub ui_addr: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct SigningSection {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub generate: bool,
    #[serde(default)]
    pub require: Option<bool>,
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct TestSection {
    #[serde(default)]
    pub ui: Vec<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone)]
pub struct SmokeSection {
    #[serde(default)]
    pub calls: Vec<SmokeCall>,
    #[serde(default)]
    pub http: Vec<SmokeHttp>,
    #[serde(default)]
    pub services: Vec<SmokeService>,
}

#[derive(serde::Deserialize, Debug, Default, Clone)]
pub struct SmokeCall {
    pub plugin: String,
    pub function: String,
    #[serde(default)]
    pub args: Vec<serde_json::Value>,
    #[serde(default)]
    pub expect_contains: Option<String>,
    #[serde(default)]
    pub expect_equals: Option<serde_json::Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(serde::Deserialize, Debug, Default, Clone)]
pub struct SmokeHttp {
    pub path: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub expect_status: Option<u16>,
    #[serde(default)]
    pub expect_contains: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone)]
pub struct SmokeService {
    pub plugin: String,
}

#[derive(serde::Deserialize, Debug, Default, Clone)]
pub struct PluginSection {
    pub path: String,
    pub language: String,
    #[serde(rename = "type", default = "default_plugin_type")]
    pub kind: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub fs: Vec<String>,
    #[serde(default)]
    pub kv: Option<String>,
    #[serde(default)]
    pub http_outbound: Vec<String>,
    #[serde(default)]
    pub network_outbound: Vec<String>,
    #[serde(default)]
    pub network_inbound: Vec<String>,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub ui_entry: Option<String>,
    #[serde(default)]
    pub ui_build: Vec<String>,
    #[serde(default)]
    pub build: Vec<String>,
    #[serde(default)]
    pub compose_with: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_plugin_type() -> String {
    "library".to_string()
}

fn validate_socket_addr(addr: &str) -> anyhow::Result<()> {
    addr.parse::<std::net::SocketAddr>()
        .map_err(|e| anyhow::anyhow!("invalid address {addr:?}: {e}"))?;
    Ok(())
}

pub fn read_host_addr(config: &Path) -> Option<String> {
    read_app_config(config)
        .ok()
        .and_then(|cfg| cfg.host)
        .and_then(|h| h.addr)
}

pub fn read_app_config(config_path: &Path) -> anyhow::Result<AppConfig> {
    let raw = std::fs::read_to_string(config_path)?;
    let cfg: AppConfig = toml::from_str(&raw)?;
    cfg.validate()?;
    Ok(cfg)
}

pub fn config_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg_attr(not(test), allow(dead_code))] // used by RF-01/B0.6 path resolution
pub fn resolve_path(config_path: &Path, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_dir(config_path).join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_config_loads() {
        let config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../wasm-app.toml");
        let cfg = read_app_config(&config).expect("current config should load");
        assert!(cfg.plugins.unwrap_or_default().contains_key("notes"));
    }

    #[test]
    fn relative_path_resolves_against_config_dir() {
        let p = resolve_path(Path::new("/tmp/app/wasm-app.toml"), "plugins/notes");
        assert_eq!(p, PathBuf::from("/tmp/app/plugins/notes"));
    }

    #[test]
    fn invalid_port_errors() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-invalid-port");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[host]\naddr = \"127.0.0.1:70000\"\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(err.to_string().contains("invalid address"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_fields_do_not_break_loading() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-unknown");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(
            &path,
            "[app]\nversion = \"1.2.3\"\nunknown_field = 42\n\n[plugins.demo]\npath = \"plugins/demo\"\nlanguage = \"rust\"\ntype = \"library\"\nextra = \"ignored\"\n",
        )
        .unwrap();
        let cfg = read_app_config(&path).unwrap();
        assert_eq!(cfg.app.unwrap().version.as_deref(), Some("1.2.3"));
        let _ = std::fs::remove_file(&path);
    }
}
