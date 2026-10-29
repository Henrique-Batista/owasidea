//! `owasidea validate`: manifest/checksum/permission/ABI checks (FIX-02).
//!
//! Two renderings over the same collected result:
//!
//! - text (default): the historical line-per-check output;
//! - `--json`: a stable object `{ok, plugins, errors, warnings}` for CI/agents.
//!
//! Every check is recorded once and rendered by one of the two views. ABI 1
//! manifests fail validation and require an explicit migration.

use anyhow::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::abi;
use crate::config::{self, AppConfig, PluginSection};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Error,
    /// Human-only context ("checksum ok", "UI manifest ok"). Never serialized.
    Note,
}

struct Entry {
    level: Level,
    message: String,
    prefix_name: bool,
}

impl Entry {
    fn error(message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            message: message.into(),
            prefix_name: true,
        }
    }

    fn note(message: impl Into<String>) -> Self {
        Self {
            level: Level::Note,
            message: message.into(),
            prefix_name: true,
        }
    }
}

struct PluginReport {
    name: String,
    entries: Vec<Entry>,
}

impl PluginReport {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            entries: Vec::new(),
        }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::error(message));
    }

    fn note(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::note(message));
    }

    fn errors(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|e| e.level == Level::Error)
            .map(|e| e.message.as_str())
    }

    fn is_ok(&self) -> bool {
        !self.entries.iter().any(|e| e.level == Level::Error)
    }

    fn error_count(&self) -> usize {
        self.errors().count()
    }

    fn render_text(&self) {
        for entry in &self.entries {
            let prefix = "";
            if entry.prefix_name {
                println!("{prefix}{}: {}", self.name, entry.message);
            } else {
                println!("{prefix}{}", entry.message);
            }
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "ok": self.is_ok(),
            "errors": self.errors().collect::<Vec<_>>(),
            "warnings": [],
        })
    }
}

/// Result of one `validate` run.
struct Report {
    plugins: Vec<PluginReport>,
    /// Config-level failures (unreadable/invalid `wasm-app.toml`).
    global_errors: Vec<String>,
}

impl Report {
    fn empty() -> Self {
        Self {
            plugins: Vec::new(),
            global_errors: Vec::new(),
        }
    }

    fn config_failure(message: impl Into<String>) -> Self {
        Self {
            plugins: Vec::new(),
            global_errors: vec![message.into()],
        }
    }

    fn error_count(&self) -> usize {
        self.global_errors.len()
            + self
                .plugins
                .iter()
                .map(PluginReport::error_count)
                .sum::<usize>()
    }

    fn is_ok(&self) -> bool {
        self.error_count() == 0
    }

    fn errors(&self) -> Vec<String> {
        let mut out = self.global_errors.clone();
        for plugin in &self.plugins {
            out.extend(plugin.errors().map(|e| format!("{}: {e}", plugin.name)));
        }
        out
    }

    fn to_json(&self) -> Value {
        json!({
            "ok": self.is_ok(),
            "plugins": self.plugins.iter().map(PluginReport::to_json).collect::<Vec<_>>(),
            "errors": self.errors(),
            "warnings": [],
        })
    }

    fn render_text(&self) {
        for error in &self.global_errors {
            println!("error: {error}");
        }
        for plugin in &self.plugins {
            plugin.render_text();
        }
    }
}

pub fn cmd_validate(config_path: &Path, manifest_dir: &Path, json_out: bool) -> Result<()> {
    let root = config::config_dir(config_path);

    let cfg = match load_config(config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            // `--json` consumers must always get a parseable document, even
            // when the config itself is what is broken.
            if json_out {
                println!("{}", Report::config_failure(err.to_string()).to_json());
            }
            return Err(err);
        }
    };

    let manifest_dir = resolve(&root, manifest_dir);
    let plugins = cfg.plugins.clone().unwrap_or_default();
    let app_glue = cfg.app.as_ref().and_then(|app| app.ui_plugin.as_deref());
    let mut report = Report::empty();

    for (name, plugin) in &plugins {
        let mut plugin_report = PluginReport::new(name);
        check_plugin(
            &root,
            &manifest_dir,
            name,
            plugin,
            app_glue,
            &mut plugin_report,
        );
        report.plugins.push(plugin_report);
    }

    finish(report, json_out)
}

fn load_config(config_path: &Path) -> Result<AppConfig> {
    config::read_app_config(config_path)
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn finish(report: Report, json_out: bool) -> Result<()> {
    if json_out {
        println!("{}", report.to_json());
        if !report.is_ok() {
            anyhow::bail!("validation failed with {} error(s)", report.error_count());
        }
        return Ok(());
    }

    report.render_text();
    let errors = report.error_count();
    if errors > 0 {
        anyhow::bail!("validation failed with {errors} error(s)");
    }
    println!("Validation OK.");
    Ok(())
}

fn check_plugin(
    root: &Path,
    manifest_dir: &Path,
    name: &str,
    plugin: &PluginSection,
    app_glue: Option<&str>,
    report: &mut PluginReport,
) {
    let manifest_path = manifest_dir.join(format!("{name}.manifest.json"));
    if !manifest_path.exists() {
        report.entries.push(Entry {
            level: Level::Error,
            message: format!("MISSING manifest: {}", manifest_path.display()),
            prefix_name: false,
        });
        return;
    }
    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(err) => {
            report.error(format!("cannot read manifest: {err}"));
            return;
        }
    };

    check_abi(&manifest, report);
    if plugin.runtime.as_deref() == Some("js-wasm") {
        check_ui_manifest(&manifest, report);
        return;
    }
    check_checksum(root, plugin, name, &manifest, report);
    check_permissions(plugin, &manifest, report);
    check_dependency_drift(plugin, &manifest, app_glue, report);
}

fn read_manifest(path: &Path) -> Result<Value> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// ABI 1 and legacy manifests require explicit migration and rebuild.
fn check_abi(manifest: &Value, report: &mut PluginReport) {
    let schema_version = manifest["schema_version"].as_u64().unwrap_or(0);
    if schema_version != abi::CURRENT_SCHEMA_VERSION {
        report.error(format!(
            "schema_version {schema_version} is not supported; expected 2. Run `owasidea migrate` and rebuild with Owasidea 1.2."
        ));
        return;
    }
    match manifest["host_abi"].as_str() {
        Some(host_abi) if host_abi == owasidea_host::HOST_ABI => {}
        Some(host_abi) => report.error(format!(
            "host_abi {host_abi} != {}",
            owasidea_host::HOST_ABI
        )),
        None => report.error("host_abi required for schema_version 2"),
    }
    match manifest["min_host"].as_str() {
        Some(min_host) => {
            if min_host_is_too_new(min_host) {
                report.error(format!(
                    "min_host {min_host} > host {}",
                    owasidea_host::HOST_VERSION
                ));
            }
        }
        None => report.error("min_host required for schema_version 2"),
    }
}

fn min_host_is_too_new(min_host: &str) -> bool {
    let required = semver::Version::parse(min_host).ok();
    let host = semver::Version::parse(owasidea_host::HOST_VERSION).ok();
    match (required, host) {
        (Some(required), Some(host)) => required > host,
        _ => true,
    }
}

fn check_ui_manifest(manifest: &Value, report: &mut PluginReport) {
    if manifest["runtime"].as_str() != Some("js-wasm") || manifest["type"].as_str() != Some("ui") {
        report.error("UI manifest runtime/type mismatch");
    } else {
        report.note("UI manifest ok");
    }
}

fn check_checksum(
    root: &Path,
    plugin: &PluginSection,
    name: &str,
    manifest: &Value,
    report: &mut PluginReport,
) {
    let component = resolve(root, Path::new(&plugin.path)).join(format!("{name}.component.wasm"));
    let bytes = match std::fs::read(&component) {
        Ok(bytes) => bytes,
        Err(_) => {
            report.error("component not found");
            return;
        }
    };
    let actual = format!("{:x}", Sha256::digest(&bytes));
    let expected = manifest["metadata"]["sha256"].as_str().unwrap_or("");
    if expected.is_empty() {
        report.note("no checksum in manifest");
    } else if expected != actual {
        report.error("CHECKSUM MISMATCH");
    } else {
        report.note("checksum ok");
    }
}

fn check_permissions(plugin: &PluginSection, manifest: &Value, report: &mut PluginReport) {
    let manifest_fs = manifest["permissions"]["filesystem"]
        .as_array()
        .map(|entries| entries.len())
        .unwrap_or(0);
    let preset_fs = usize::from(plugin.capabilities.iter().any(|c| c == "app-data"));
    if manifest_fs != plugin.fs.len() + preset_fs {
        report.error(format!(
            "fs permission mismatch (manifest {manifest_fs}, config {})",
            plugin.fs.len() + preset_fs
        ));
    }

    let manifest_kv: std::collections::BTreeSet<String> = manifest["permissions"]["kv"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|grant| {
            format!(
                "{}:{}",
                grant["namespace"].as_str().unwrap_or(""),
                grant["access"].as_str().unwrap_or("")
            )
        })
        .collect();
    let configured_kv: std::collections::BTreeSet<String> = plugin
        .kv
        .iter()
        .map(|grant| {
            let (namespace, access) = grant
                .rsplit_once(':')
                .unwrap_or((grant.as_str(), "read-write"));
            format!("{namespace}:{access}")
        })
        .collect();
    if manifest_kv != configured_kv {
        report.error("kv permission mismatch");
    }
}

fn check_dependency_drift(
    plugin: &PluginSection,
    manifest: &Value,
    app_glue: Option<&str>,
    report: &mut PluginReport,
) {
    let manifest_glue = manifest["metadata"]["app_ui_plugin"].as_str();
    if manifest_glue != app_glue {
        report.error(format!(
            "app.ui_plugin drift (manifest {:?}, config {:?})",
            manifest_glue, app_glue
        ));
    }

    let mut manifest_dependencies: Vec<&str> = manifest["imports"]["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut configured_dependencies: Vec<&str> =
        plugin.dependencies.iter().map(String::as_str).collect();
    manifest_dependencies.sort_unstable();
    configured_dependencies.sort_unstable();
    if manifest_dependencies != configured_dependencies {
        report.error(format!(
            "dependency drift (manifest {:?}, config {:?})",
            manifest_dependencies, configured_dependencies
        ));
    }

    let preset_outbound = plugin
        .capabilities
        .iter()
        .any(|capability| capability == "network-outbound");
    let manifest_any = manifest["permissions"]["network"]["outbound_any"]
        .as_bool()
        .unwrap_or(false);
    if manifest_any != preset_outbound {
        report.error("network-outbound preset drift");
    }

    let list_matches = |path: &str, configured: &[String]| {
        let manifest_entries: std::collections::BTreeSet<String> = manifest
            .pointer(path)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let configured: std::collections::BTreeSet<String> = configured.iter().cloned().collect();
        manifest_entries == configured
    };
    if !list_matches("/permissions/http/outbound", &plugin.http_outbound) {
        report.error("http outbound drift");
    }
    if !list_matches("/permissions/network/outbound", &plugin.network_outbound) {
        report.error("network outbound drift");
    }
    if !list_matches("/permissions/network/inbound", &plugin.network_inbound) {
        report.error("network inbound drift");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_plugin<'a>(report: &'a mut Report, name: &str) -> &'a mut PluginReport {
        report.plugins.push(PluginReport::new(name));
        report.plugins.last_mut().expect("just pushed")
    }

    #[test]
    fn report_ok_when_no_errors() {
        let mut report = Report::empty();
        add_plugin(&mut report, "notes").note("checksum ok");
        assert!(report.is_ok());
        let value = report.to_json();
        assert_eq!(value["ok"], true);
        assert_eq!(value["plugins"][0]["ok"], true);
        assert_eq!(value["plugins"][0]["errors"], json!([]));
        assert_eq!(value["errors"], json!([]));
        assert_eq!(value["warnings"], json!([]));
    }

    #[test]
    fn notes_are_not_serialized_as_errors_or_warnings() {
        let mut report = Report::empty();
        add_plugin(&mut report, "notes").note("UI manifest ok");
        let value = report.to_json();
        assert_eq!(value["plugins"][0]["errors"].as_array().unwrap().len(), 0);
        assert_eq!(value["plugins"][0]["warnings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn errors_are_aggregated_with_plugin_names() {
        let mut report = Report::empty();
        {
            let plugin = add_plugin(&mut report, "notes");
            plugin.error("CHECKSUM MISMATCH");
        }
        assert!(!report.is_ok());
        assert_eq!(report.error_count(), 1);
        let value = report.to_json();
        assert_eq!(value["ok"], false);
        assert_eq!(value["plugins"][0]["name"], "notes");
        assert_eq!(value["plugins"][0]["ok"], false);
        assert_eq!(value["errors"], json!(["notes: CHECKSUM MISMATCH"]));
        assert_eq!(value["warnings"], json!([]));
    }

    #[test]
    fn abi1_manifest_fails_with_migration_hint() {
        let mut report = PluginReport::new("legacy");
        check_abi(&json!({"schema_version": 1}), &mut report);
        assert!(!report.is_ok());
        assert!(report.errors().next().unwrap().contains("expected 2"));
        assert!(report.errors().next().unwrap().contains("owasidea migrate"));
    }

    #[test]
    fn dependency_and_glue_drift_is_reported() {
        let plugin = PluginSection {
            dependencies: vec!["provider".to_string()],
            http_outbound: vec!["api.example.com:443".to_string()],
            ..PluginSection::default()
        };
        let manifest = json!({
            "metadata": {"app_ui_plugin": "glue"},
            "imports": {"plugins": []},
            "permissions": {
                "http": {"outbound": ["evil.example.com:443"], "outbound_any": false},
                "network": {"outbound": [], "outbound_any": false, "inbound": []}
            }
        });
        let mut report = PluginReport::new("glue");
        check_dependency_drift(&plugin, &manifest, Some("other"), &mut report);
        let errors: Vec<_> = report.errors().collect();
        assert!(errors
            .iter()
            .any(|error| error.contains("app.ui_plugin drift")));
        assert!(errors
            .iter()
            .any(|error| error.contains("dependency drift")));
        assert!(errors
            .iter()
            .any(|error| error.contains("http outbound drift")));
    }

    #[test]
    fn config_failure_serializes_as_not_ok() {
        let value = Report::config_failure("app.name must not be empty").to_json();
        assert_eq!(value["ok"], false);
        assert_eq!(value["plugins"], json!([]));
        assert_eq!(value["errors"], json!(["app.name must not be empty"]));
    }

    #[test]
    fn min_host_comparison_matches_host_semver() {
        assert!(!min_host_is_too_new(owasidea_host::HOST_VERSION));
        assert!(min_host_is_too_new("9999.0.0"));
        assert!(min_host_is_too_new("not-a-version"));
    }
}
