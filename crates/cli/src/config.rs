use std::path::{Path, PathBuf};

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub app: Option<AppSection>,
    pub host: Option<HostSection>,
    pub desktop: Option<DesktopSection>,
    pub test: Option<TestSection>,
    pub smoke: Option<SmokeSection>,
    pub plugins: Option<std::collections::BTreeMap<String, PluginSection>>,
    pub dev: Option<DevSection>,
    pub updater: Option<UpdaterSection>,
}

impl AppConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(app) = &self.app {
            if let Some(name) = &app.name {
                if name.trim().is_empty() {
                    anyhow::bail!("app.name must not be empty");
                }
            }
            if let Some(version) = &app.version {
                if version.trim().is_empty() {
                    anyhow::bail!("app.version must not be empty");
                }
            }
            if let Some(identifier) = &app.identifier {
                if identifier.trim().is_empty() {
                    anyhow::bail!("app.identifier must not be empty");
                }
            }
        }
        if let Some(host) = &self.host {
            if let Some(addr) = &host.addr {
                validate_socket_addr("host.addr", addr)?;
            }
        }
        if let Some(desktop) = &self.desktop {
            validate_nonempty_opt("desktop.path", desktop.path.as_deref())?;
            validate_nonempty_opt("desktop.ui", desktop.ui.as_deref())?;
            validate_nonempty_opt("desktop.output", desktop.output.as_deref())?;
        }
        if let Some(plugins) = &self.plugins {
            for (name, plugin) in plugins {
                if plugin.path.trim().is_empty() {
                    anyhow::bail!("plugins.{}.path must not be empty", name);
                }
                for (i, fs) in plugin.fs.iter().enumerate() {
                    if fs.trim().is_empty() {
                        anyhow::bail!("plugins.{}.fs[{}] must not be empty", name, i);
                    }
                }
                for dependency in &plugin.dependencies {
                    if !plugins.contains_key(dependency) {
                        anyhow::bail!(
                            "plugins.{name}.dependencies names unknown plugin {dependency:?}"
                        );
                    }
                    if plugin.compose_with.contains(dependency) {
                        anyhow::bail!(
                            "plugins.{name} declares {dependency:?} in both dependencies and compose_with"
                        );
                    }
                }
                let mut kv_namespaces = std::collections::BTreeSet::new();
                for grant in &plugin.kv {
                    let (namespace, access) = grant
                        .rsplit_once(':')
                        .unwrap_or((grant.as_str(), "read-write"));
                    if namespace.trim().is_empty()
                        || !namespace
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                        || !matches!(access, "read" | "read-write")
                    {
                        anyhow::bail!("plugins.{name}.kv has invalid grant {grant:?}");
                    }
                    if !kv_namespaces.insert(namespace) {
                        anyhow::bail!("plugins.{name}.kv repeats namespace {namespace:?}");
                    }
                }
                for (i, host) in plugin.http_outbound.iter().enumerate() {
                    validate_host_port(&format!("plugins.{name}.http_outbound[{i}]"), host)?;
                }
                for (i, host) in plugin.network_outbound.iter().enumerate() {
                    validate_host_port(&format!("plugins.{name}.network_outbound[{i}]"), host)?;
                }
                for (i, address) in plugin.network_inbound.iter().enumerate() {
                    validate_ip_port(&format!("plugins.{name}.network_inbound[{i}]"), address)?;
                }
                for capability in &plugin.capabilities {
                    if !matches!(capability.as_str(), "network-outbound" | "app-data") {
                        anyhow::bail!(
                            "plugins.{name}.capabilities has unknown preset {capability:?}"
                        );
                    }
                }
                if let Some(source) = plugin.wit_source.as_deref() {
                    if !matches!(source, "auto" | "wit" | "attributes") {
                        anyhow::bail!(
                            "plugins.{}.wit_source must be auto, wit or attributes (found {:?})",
                            name,
                            source
                        );
                    }
                }
            }
        }
        if let Some(ui_plugin) = self.app.as_ref().and_then(|app| app.ui_plugin.as_deref()) {
            let plugin = self
                .plugins
                .as_ref()
                .and_then(|plugins| plugins.get(ui_plugin))
                .ok_or_else(|| anyhow::anyhow!("app.ui_plugin {ui_plugin:?} is not configured"))?;
            if plugin.runtime.as_deref() == Some("js-wasm") || plugin.kind != "library" {
                anyhow::bail!("app.ui_plugin must name a headless library plugin");
            }
        }

        if let Some(dev) = &self.dev {
            if let Some(addr) = &dev.ui_addr {
                validate_socket_addr("dev.ui_addr", addr)?;
            }
        }
        if let Some(updater) = &self.updater {
            validate_nonempty_opt("updater.channel", updater.channel.as_deref())?;
            validate_nonempty_opt("updater.endpoint", updater.endpoint.as_deref())?;
            validate_nonempty_opt("updater.pubkey", updater.pubkey.as_deref())?;
            validate_nonempty_opt("updater.notes_file", updater.notes_file.as_deref())?;
            if let Some(channel) = &updater.channel {
                crate::updater::validate_channel(channel)?;
            }
            if let Some(endpoint) = &updater.endpoint {
                crate::updater::validate_endpoint(endpoint)?;
            }
        }
        Ok(())
    }

    pub fn validate_for_bundle(&self) -> anyhow::Result<()> {
        self.validate()?;
        if let Some(host) = &self.host {
            if let Some(addr) = &host.addr {
                let parsed: std::net::SocketAddr = addr
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid address for host.addr {addr:?}: {e}"))?;
                if parsed.port() == 0 {
                    anyhow::bail!(
                        "port 0 is not allowed for host.addr in bundle context: {addr:?}"
                    );
                }
            }
        }
        if let Some(dev) = &self.dev {
            if let Some(addr) = &dev.ui_addr {
                let parsed: std::net::SocketAddr = addr.parse().map_err(|e| {
                    anyhow::anyhow!("invalid address for dev.ui_addr {addr:?}: {e}")
                })?;
                if parsed.port() == 0 {
                    anyhow::bail!(
                        "port 0 is not allowed for dev.ui_addr in bundle context: {addr:?}"
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostSection {
    #[serde(default)]
    pub addr: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppSection {
    pub name: Option<String>,
    pub version: Option<String>,
    pub identifier: Option<String>,
    #[serde(default)]
    pub ui_plugin: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopSection {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub ui: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DevSection {
    #[serde(default = "default_true")]
    pub watch: bool,
    #[serde(default = "default_true")]
    pub hot_reload: bool,
    #[serde(default)]
    pub ui_addr: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdaterSection {
    /// Enable updater config/latest.json generation (default true when a
    /// pubkey and endpoint resolve).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Release channel: `stable` or `beta`.
    #[serde(default)]
    pub channel: Option<String>,
    /// Update manifest endpoint. `{{channel}}` is substituted at render time;
    /// a trailing `/` means `<endpoint>/latest.json`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Public key file (minisign) for update signatures, relative to the app root.
    #[serde(default)]
    pub pubkey: Option<String>,
    /// Inline release notes written to latest.json.
    #[serde(default)]
    pub notes: Option<String>,
    /// Release notes file, relative to the app root.
    #[serde(default)]
    pub notes_file: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestSection {
    #[serde(default)]
    pub ui: Vec<String>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SmokeSection {
    #[serde(default)]
    pub calls: Vec<SmokeCall>,
    #[serde(default)]
    pub http: Vec<SmokeHttp>,
    #[serde(default)]
    pub services: Vec<SmokeService>,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SmokeService {
    pub plugin: String,
}

#[derive(serde::Deserialize, Debug, Default, Clone, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub kv: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub http_outbound: Vec<String>,
    #[serde(default)]
    pub network_outbound: Vec<String>,
    #[serde(default)]
    pub network_inbound: Vec<String>,
    #[serde(default)]
    pub persistent: bool,
    /// Expose this plugin's HTTP surface (`/plugin/<name>/*` and
    /// `/app/<name>`). Defaults to `false`; the configured `ui_plugin` is
    /// always reachable so the webview can call it.
    #[serde(default)]
    pub expose: bool,
    #[serde(default)]
    pub runtime: Option<String>,
    /// Where the component contract comes from: `auto` (default), `wit`
    /// (hand-written `wit/` or the Cargo metadata target path) or `attributes`
    /// (generated from `#[owasidea::world]` / `#[owasidea::export]` markers).
    #[serde(default)]
    pub wit_source: Option<String>,
    #[serde(default)]
    pub ui_entry: Option<String>,
    #[serde(default)]
    pub ui_build: Vec<String>,
    #[serde(default)]
    pub build: Vec<String>,
    #[serde(default)]
    pub compose_with: Vec<String>,
    #[serde(default)]
    pub bridge: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_plugin_type() -> String {
    "library".to_string()
}

fn validate_socket_addr(field: &str, addr: &str) -> anyhow::Result<()> {
    addr.parse::<std::net::SocketAddr>()
        .map_err(|e| anyhow::anyhow!("invalid address for {field} {addr:?}: {e}"))?;
    Ok(())
}

pub(crate) fn validate_host_port(field: &str, value: &str) -> anyhow::Result<()> {
    let (host, port) = if let Ok(address) = value.parse::<std::net::SocketAddr>() {
        (address.ip().to_string(), address.port())
    } else {
        let (host, port) = value
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("{field} must be exact host:port: {value:?}"))?;
        if host.contains(':') || host.starts_with('[') || host.ends_with(']') {
            anyhow::bail!("{field} has invalid IPv6 host: {value:?}");
        }
        (host.to_string(), port.parse::<u16>().unwrap_or(0))
    };
    if port == 0 || host.is_empty() || host == "*" || host.contains('/') {
        anyhow::bail!("{field} must be exact host:port with nonzero port: {value:?}");
    }
    if host.parse::<std::net::IpAddr>().is_err()
        && (host.len() > 253
            || host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }))
    {
        anyhow::bail!("{field} has invalid hostname: {value:?}");
    }
    Ok(())
}

pub(crate) fn validate_ip_port(field: &str, value: &str) -> anyhow::Result<()> {
    let address = value.parse::<std::net::SocketAddr>().map_err(|error| {
        anyhow::anyhow!("{field} must be an IP:port address {value:?}: {error}")
    })?;
    if address.port() == 0 {
        anyhow::bail!("{field} port must be nonzero: {value:?}");
    }
    Ok(())
}

fn validate_nonempty_opt(field: &str, value: Option<&str>) -> anyhow::Result<()> {
    if let Some(v) = value {
        if v.trim().is_empty() {
            anyhow::bail!("{field} must not be empty");
        }
    }
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
    let cfg: AppConfig = toml::from_str(&raw).map_err(|error| {
        anyhow::anyhow!("{error}; if this is a 1.1 config, run `owasidea migrate`")
    })?;
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
        let config =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello/wasm-app.toml");
        let cfg = read_app_config(&config).expect("example config should load");
        assert!(cfg.plugins.unwrap_or_default().contains_key("greeter"));
    }

    #[test]
    fn validates_dependencies_kv_and_exact_network_grants() {
        let valid = r#"
[plugins.provider]
path = "provider"
language = "rust"

[plugins.consumer]
path = "consumer"
language = "rust"
dependencies = ["provider"]
kv = ["state:read", "cache:read-write"]
http_outbound = ["api.example.com:443"]
network_outbound = ["[2001:db8::1]:443"]
network_inbound = ["127.0.0.1:8080"]
"#;
        toml::from_str::<AppConfig>(valid)
            .unwrap()
            .validate()
            .unwrap();

        let unknown_dependency = valid.replace("[\"provider\"]", "[\"missing\"]");
        assert!(toml::from_str::<AppConfig>(&unknown_dependency)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unknown plugin"));

        let wildcard = valid.replace("api.example.com:443", "*:443");
        assert!(toml::from_str::<AppConfig>(&wildcard)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string()
            .contains("exact host:port"));
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
    fn unknown_top_level_field_fails() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-unknown-top");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(
            &path,
            "[app]\nversion = \"1.2.3\"\n\n[unknown_top]\nfoo = 1\n",
        )
        .unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(
            err.to_string().contains("unknown_top") || err.to_string().contains("unknown field")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_plugin_field_fails() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-unknown-plugin");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(
            &path,
            "[plugins.demo]\npath = \"plugins/demo\"\nlanguage = \"rust\"\ntype = \"library\"\nextra = \"ignored\"\n",
        )
        .unwrap();
        let err = read_app_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("extra"), "msg: {msg}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_top_level_key_fails() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-unknown-toplevel");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "bogus_top = 1\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bogus_top"), "msg: {msg}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn host_addr_invalid_errors() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-host-invalid");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[host]\naddr = \"not-an-addr\"\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(err.to_string().contains("invalid address"), "msg: {}", err);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ui_addr_invalid_errors() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-ui-invalid");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[dev]\nui_addr = \"nope\"\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(err.to_string().contains("invalid address"), "msg: {}", err);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn port_zero_accepted_normal() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-port0-normal");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[host]\naddr = \"127.0.0.1:0\"\n").unwrap();
        let cfg = read_app_config(&path).expect("port 0 allowed in normal validation");
        cfg.validate().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn port_zero_rejected_bundle() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-port0-bundle");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[host]\naddr = \"127.0.0.1:0\"\n").unwrap();
        let cfg = read_app_config(&path).expect("port 0 allowed in normal validation");
        let err = cfg.validate_for_bundle().unwrap_err();
        assert!(err.to_string().contains("port 0"), "msg: {}", err);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_path_rejected() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-empty-path");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[desktop]\npath = \"\"\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(err.to_string().contains("desktop.path"), "msg: {}", err);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_plugin_path_rejected() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-empty-plugin-path");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[plugins.demo]\npath = \"\"\nlanguage = \"rust\"\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(
            err.to_string().contains("plugins.demo.path"),
            "msg: {}",
            err
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn relative_path_resolved_test() {
        let p = resolve_path(Path::new("/tmp/app/wasm-app.toml"), "plugins/notes");
        assert_eq!(p, PathBuf::from("/tmp/app/plugins/notes"));
    }

    #[test]
    fn app_version_empty_rejected() {
        let dir = std::env::temp_dir().join("owasidea-cfg-test-empty-version");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wasm-app.toml");
        std::fs::write(&path, "[app]\nversion = \"\"\n").unwrap();
        let err = read_app_config(&path).unwrap_err();
        assert!(err.to_string().contains("app.version"), "msg: {}", err);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn shipyard_config_loads() {
        let config =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/shipyard/wasm-app.toml");
        if config.exists() {
            let cfg = read_app_config(&config).expect("shipyard config should load");
            let plugins = cfg.plugins.clone().unwrap_or_default();
            assert!(!plugins.is_empty());
        }
    }

    #[test]
    fn expose_defaults_to_false_and_parses_true() {
        let cfg: AppConfig = toml::from_str(
            "[plugins.a]\npath = \"a\"\nlanguage = \"rust\"\n\
             [plugins.b]\npath = \"b\"\nlanguage = \"rust\"\nexpose = true\n",
        )
        .unwrap();
        let plugins = cfg.plugins.unwrap();
        assert!(!plugins["a"].expose);
        assert!(plugins["b"].expose);
    }
}
