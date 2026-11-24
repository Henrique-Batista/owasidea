//! Native UI plugins: declaration checks, build, the `native-ui.json` index,
//! and process supervision.
//!
//! Native UIs are ordinary local executables, not WASI components. They run
//! with the user's privileges; see `docs/UI_PLUGINS.md`.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use crate::config::PluginSection;

pub const SCHEMA: &str = "native-v1";
pub const TRUST: &str = "unrestricted-local-process";
pub const PLUGIN_TRUST: &str = "unrestricted";

pub fn is_native(plugin: &PluginSection) -> bool {
    plugin.kind == "native-ui" || plugin.runtime.as_deref() == Some("native")
}

pub fn trust_warning(name: &str) -> String {
    format!(
        "warning: native UI {name} runs as trusted local code with your user privileges; it is not sandboxed by the WASI capability broker"
    )
}

/// Reject declarations that mix the native and webview/WASI models.
pub fn validate_declaration(name: &str, plugin: &PluginSection) -> anyhow::Result<()> {
    let native_type = plugin.kind == "native-ui";
    let native_runtime = plugin.runtime.as_deref() == Some("native");
    if native_type != native_runtime {
        anyhow::bail!(
            "plugin {name}: type = \"native-ui\" and runtime = \"native\" are required together"
        );
    }
    if !native_type {
        return Ok(());
    }
    for (field, present) in [
        ("ui_entry", plugin.ui_entry.is_some()),
        ("ui_build", !plugin.ui_build.is_empty()),
        ("bridge", !plugin.bridge.is_empty()),
        ("build", !plugin.build.is_empty()),
        ("wit_source", plugin.wit_source.is_some()),
        ("compose_with", !plugin.compose_with.is_empty()),
        ("dependencies", !plugin.dependencies.is_empty()),
    ] {
        if present {
            anyhow::bail!(
                "plugin {name}: {field} is webview/WASI-only and cannot be used by a native UI"
            );
        }
    }
    if !plugin.fs.is_empty()
        || !plugin.kv.is_empty()
        || !plugin.capabilities.is_empty()
        || !plugin.http_outbound.is_empty()
        || !plugin.network_outbound.is_empty()
        || !plugin.network_inbound.is_empty()
        || plugin.persistent
        || plugin.expose
    {
        anyhow::bail!(
            "plugin {name}: native UIs cannot be capability-restricted; remove fs/kv/capabilities/network/expose/persistent"
        );
    }
    if plugin.native_build.len() == 1 && plugin.native_build[0].trim().is_empty() {
        anyhow::bail!("plugin {name}: native_build program name must not be empty");
    }
    if plugin
        .native_build
        .first()
        .is_some_and(|program| program.trim().is_empty())
    {
        anyhow::bail!("plugin {name}: native_build program name must not be empty");
    }
    let entry = plugin
        .native_entry
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("plugin {name}: native_entry is required"))?;
    let path = Path::new(entry);
    if entry.trim().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        anyhow::bail!("plugin {name}: native_entry must be a safe relative path inside the plugin directory, found {entry:?}");
    }
    Ok(())
}

/// Resolve `native_entry` inside the plugin directory. `require_exists` is set
/// after `native_build` has run.
pub fn resolve_entry(
    root: &Path,
    name: &str,
    plugin: &PluginSection,
    require_exists: bool,
) -> anyhow::Result<PathBuf> {
    validate_declaration(name, plugin)?;
    let plugin_dir = crate::config_render::safe_resolve(root, &plugin.path)?;
    let entry = plugin.native_entry.as_deref().unwrap_or_default();
    let candidate = plugin_dir.join(entry);
    if !require_exists {
        return Ok(candidate);
    }
    let canonical = candidate.canonicalize().with_context(|| {
        format!(
            "plugin {name}: native_entry not found: {}",
            candidate.display()
        )
    })?;
    let plugin_dir = plugin_dir.canonicalize().unwrap_or(plugin_dir);
    if !canonical.starts_with(&plugin_dir) {
        anyhow::bail!("plugin {name}: native_entry escapes the plugin directory: {entry:?}");
    }
    if !canonical.is_file() {
        anyhow::bail!(
            "plugin {name}: native_entry is not a file: {}",
            canonical.display()
        );
    }
    Ok(canonical)
}

pub fn digest(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    #[serde(rename = "type", default = "default_kind")]
    pub kind: String,
    pub runtime: String,
    pub binary: String,
    pub sha256: String,
    #[serde(default = "default_plugin_trust")]
    pub trust: String,
}

fn default_kind() -> String {
    "native-ui".to_string()
}

fn default_plugin_trust() -> String {
    PLUGIN_TRUST.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Index {
    pub schema: String,
    pub trust: String,
    pub plugins: Vec<IndexEntry>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            schema: SCHEMA.to_string(),
            trust: TRUST.to_string(),
            plugins: Vec::new(),
        }
    }
}

pub fn index_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("native-ui.json")
}

/// Build one native UI, returning its index entry.
pub fn build(
    root: &Path,
    name: &str,
    plugin: &PluginSection,
    manifest_dir: &Path,
) -> anyhow::Result<IndexEntry> {
    validate_declaration(name, plugin)?;
    let plugin_dir = crate::config_render::safe_resolve(root, &plugin.path)?;
    if !plugin_dir.is_dir() {
        anyhow::bail!(
            "plugin {name}: directory not found: {}",
            plugin_dir.display()
        );
    }
    if let Some((program, args)) = plugin.native_build.split_first() {
        if program.trim().is_empty() {
            anyhow::bail!("plugin {name}: native_build program name must not be empty");
        }
        let status = Command::new(program)
            .args(args)
            .current_dir(&plugin_dir)
            .status()
            .with_context(|| {
                format!("plugin {name}: cannot run native_build program {program:?}")
            })?;
        if !status.success() {
            anyhow::bail!("plugin {name}: native_build failed ({status})");
        }
    }
    let binary = resolve_entry(root, name, plugin, true)?;
    let sha256 = digest(&binary)?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let relative = pathdiff::diff_paths(&binary, &root).ok_or_else(|| {
        anyhow::anyhow!("plugin {name}: cannot express binary path relative to the app root")
    })?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    let _ = manifest_dir;
    Ok(IndexEntry {
        name: name.to_string(),
        kind: "native-ui".to_string(),
        runtime: "native".to_string(),
        binary: relative,
        sha256,
        trust: PLUGIN_TRUST.to_string(),
    })
}

pub fn write_index(manifest_dir: &Path, entries: Vec<IndexEntry>) -> anyhow::Result<()> {
    let path = index_path(manifest_dir);
    if entries.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let mut entries = entries;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    std::fs::create_dir_all(manifest_dir)?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&Index {
            schema: SCHEMA.to_string(),
            trust: TRUST.to_string(),
            plugins: entries,
        })? + "\n",
    )?;
    println!("    native UI index: {}", path.display());
    Ok(())
}

pub fn read_index(manifest_dir: &Path) -> anyhow::Result<Option<Index>> {
    let path = index_path(manifest_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let index: Index = serde_json::from_str(&raw)
        .with_context(|| format!("invalid native UI index {}", path.display()))?;
    if index.schema != SCHEMA {
        anyhow::bail!(
            "native UI index {} has schema {:?}; expected {SCHEMA:?}",
            path.display(),
            index.schema
        );
    }
    Ok(Some(index))
}

/// Resolve an index `binary` against the tree that owns the index. Build trees
/// store app-root-relative paths; bundles store `native/<name>/...` paths.
pub fn resolve_binary(base: &Path, binary: &str) -> PathBuf {
    base.join(binary)
}

// ---------------------------------------------------------------------------
// Supervision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Stopped,
    Running,
    Failed,
}

impl ProcessState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcessState::Stopped => "stopped",
            ProcessState::Running => "running",
            ProcessState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeUiSpec {
    pub name: String,
    pub binary: PathBuf,
}

pub type LogFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

struct Process {
    spec: NativeUiSpec,
    child: Option<Child>,
    state: ProcessState,
}

/// Owns every native UI child. Never restarts on its own: `run` reports exits,
/// `dev` restarts only on an explicit operator action.
pub struct Supervisor {
    host_url: String,
    manifest_dir: PathBuf,
    processes: Vec<Process>,
    log: LogFn,
}

impl Supervisor {
    pub fn new(
        specs: Vec<NativeUiSpec>,
        host_url: impl Into<String>,
        manifest_dir: PathBuf,
        log: LogFn,
    ) -> Self {
        Self {
            host_url: host_url.into(),
            manifest_dir,
            processes: specs
                .into_iter()
                .map(|spec| Process {
                    spec,
                    child: None,
                    state: ProcessState::Stopped,
                })
                .collect(),
            log,
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.processes
            .iter()
            .map(|process| process.spec.name.clone())
            .collect()
    }

    #[cfg(test)]
    pub fn status(&self, name: &str) -> Option<ProcessState> {
        self.processes
            .iter()
            .find(|process| process.spec.name == name)
            .map(|process| process.state)
    }

    pub fn statuses(&self) -> BTreeMap<String, ProcessState> {
        self.processes
            .iter()
            .map(|process| (process.spec.name.clone(), process.state))
            .collect()
    }

    pub fn start(&mut self, name: &str) -> anyhow::Result<()> {
        let process = self
            .processes
            .iter_mut()
            .find(|process| process.spec.name == name)
            .ok_or_else(|| anyhow::anyhow!("unknown native UI {name}"))?;
        if process.child.is_some() {
            return Ok(());
        }
        let binary = &process.spec.binary;
        let mut child = Command::new(binary)
            .env("OWASIDEA_HOST", &self.host_url)
            .env("OWASIDEA_PLUGIN", &process.spec.name)
            .env("OWASIDEA_MANIFESTS", &self.manifest_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "cannot start native UI {} ({})",
                    process.spec.name,
                    binary.display()
                )
            })?;
        let name = process.spec.name.clone();
        if let Some(stdout) = child.stdout.take() {
            pipe(name.clone(), stdout, self.log.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            pipe(name.clone(), stderr, self.log.clone());
        }
        (self.log)(&name, "started");
        process.child = Some(child);
        process.state = ProcessState::Running;
        Ok(())
    }

    pub fn start_all(&mut self) {
        let names = self.names();
        for name in names {
            if let Err(error) = self.start(&name) {
                (self.log)(&name, &error.to_string());
            }
        }
    }

    pub fn stop(&mut self, name: &str) -> anyhow::Result<()> {
        let process = self
            .processes
            .iter_mut()
            .find(|process| process.spec.name == name)
            .ok_or_else(|| anyhow::anyhow!("unknown native UI {name}"))?;
        let was_running = process.child.is_some();
        if let Some(mut child) = process.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        process.state = ProcessState::Stopped;
        if was_running {
            (self.log)(name, "stopped");
        }
        Ok(())
    }

    pub fn stop_all(&mut self) {
        let names = self.names();
        for name in names {
            let _ = self.stop(&name);
        }
    }

    pub fn restart(&mut self, name: &str) -> anyhow::Result<()> {
        self.stop(name)?;
        self.start(name)
    }

    /// Reap exited children and report them. Does not restart anything.
    pub fn poll(&mut self) -> Vec<(String, ProcessState)> {
        let mut changed = Vec::new();
        for process in &mut self.processes {
            let Some(child) = process.child.as_mut() else {
                continue;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    process.child = None;
                    process.state = if status.success() {
                        ProcessState::Stopped
                    } else {
                        ProcessState::Failed
                    };
                    (self.log)(&process.spec.name, &format!("exited with {status}"));
                    changed.push((process.spec.name.clone(), process.state));
                }
                Ok(None) => {}
                Err(error) => {
                    process.child = None;
                    process.state = ProcessState::Failed;
                    (self.log)(&process.spec.name, &format!("status error: {error}"));
                    changed.push((process.spec.name.clone(), process.state));
                }
            }
        }
        changed
    }

    pub fn shutdown(&mut self) {
        self.stop_all();
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn pipe<R: std::io::Read + Send + 'static>(name: String, stream: R, log: LogFn) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            log(&name, &line);
        }
    });
}

/// Host target triple, used for Tauri sidecar names. Native UI binaries are
/// never cross-compiled, so this must match the produced executable.
pub fn host_triple() -> anyhow::Result<String> {
    if let Ok(value) = std::env::var("OWASIDEA_TARGET_TRIPLE") {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }
    let output = Command::new("rustc").args(["-vV"]).output().context(
        "cannot determine the host target triple; install rustc or set OWASIDEA_TARGET_TRIPLE",
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|triple| triple.trim().to_string())
        .filter(|triple| !triple.is_empty())
        .ok_or_else(|| anyhow::anyhow!("rustc -vV did not report a host triple"))
}

/// Copy native UI binaries next to a Tauri desktop shell and add the matching
/// `bundle.externalBin` entries to the generated config. Returns the count.
pub fn stage_tauri_sidecars(
    root: &Path,
    plugins: &std::collections::BTreeMap<String, PluginSection>,
    desktop_dir: &Path,
    generated_config: &Path,
    requested_triple: Option<&str>,
) -> anyhow::Result<usize> {
    let native: Vec<_> = plugins
        .iter()
        .filter(|(_, plugin)| is_native(plugin))
        .collect();
    if native.is_empty() {
        return Ok(0);
    }
    let host = host_triple()?;
    if let Some(requested) = requested_triple {
        if requested != host {
            anyhow::bail!(
                "native UI binaries are not cross-compiled: requested target {requested}, host is {host}"
            );
        }
    }
    let binaries = desktop_dir.join("binaries");
    std::fs::create_dir_all(&binaries)?;
    let mut entries = Vec::new();
    for (name, plugin) in native {
        let binary = resolve_entry(root, name, plugin, true)?;
        let target = binaries.join(format!("{name}-{host}"));
        std::fs::copy(&binary, &target)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&target)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&target, permissions)?;
        }
        entries.push(format!("binaries/{name}"));
    }
    let count = entries.len();
    let raw = std::fs::read_to_string(generated_config)?;
    let mut value: serde_json::Value = serde_json::from_str(&raw)?;
    let bundle = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("tauri.generated.conf.json root must be an object"))?
        .entry("bundle".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let bundle = bundle
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("tauri.generated.conf.json bundle must be an object"))?;
    let external = bundle
        .entry("externalBin".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let external = external
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("bundle.externalBin must be an array"))?;
    for entry in entries {
        let value = serde_json::Value::String(entry);
        if !external.contains(&value) {
            external.push(value);
        }
    }
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    std::fs::write(generated_config, bytes)?;
    Ok(count)
}

pub fn display_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "owasidea-native-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn plugin(entry: &str) -> PluginSection {
        PluginSection {
            path: "plugins/board".to_string(),
            language: "gpui".to_string(),
            kind: "native-ui".to_string(),
            runtime: Some("native".to_string()),
            native_entry: Some(entry.to_string()),
            ..PluginSection::default()
        }
    }

    fn log() -> (LogFn, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let captured = lines.clone();
        let log: LogFn = Arc::new(move |name, text| {
            captured.lock().unwrap().push(format!("[{name}] {text}"));
        });
        (log, lines)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn display_detection_follows_the_environment() {
        let _guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let display = std::env::var_os("DISPLAY");
        let wayland = std::env::var_os("WAYLAND_DISPLAY");
        std::env::remove_var("DISPLAY");
        std::env::remove_var("WAYLAND_DISPLAY");
        assert!(!display_available());
        std::env::set_var("DISPLAY", ":99");
        assert!(display_available());
        match display {
            Some(value) => std::env::set_var("DISPLAY", value),
            None => std::env::remove_var("DISPLAY"),
        }
        match wayland {
            Some(value) => std::env::set_var("WAYLAND_DISPLAY", value),
            None => std::env::remove_var("WAYLAND_DISPLAY"),
        }
    }

    #[test]
    fn trust_warning_names_the_plugin() {
        let warning = trust_warning("board");
        assert!(warning.contains("board"), "{warning}");
        assert!(warning.contains("trusted local code"), "{warning}");
        assert!(warning.contains("not sandboxed"), "{warning}");
    }

    #[test]
    fn declaration_rejects_webview_fields() {
        let mut p = plugin("target/release/board");
        p.ui_entry = Some("index.html".to_string());
        let error = validate_declaration("board", &p).unwrap_err().to_string();
        assert!(error.contains("ui_entry"), "{error}");
    }

    #[test]
    fn declaration_rejects_capabilities() {
        let mut p = plugin("target/release/board");
        p.fs = vec!["reports".to_string()];
        let error = validate_declaration("board", &p).unwrap_err().to_string();
        assert!(error.contains("capability"), "{error}");
        p.fs.clear();
        p.kv = vec!["ns:read".to_string()];
        assert!(validate_declaration("board", &p).is_err());
    }

    #[test]
    fn declaration_requires_the_native_runtime() {
        let mut p = plugin("target/release/board");
        p.runtime = None;
        let error = validate_declaration("board", &p).unwrap_err().to_string();
        assert!(error.contains("runtime = \"native\""), "{error}");
    }

    #[test]
    fn declaration_rejects_empty_program() {
        let mut p = plugin("target/release/board");
        p.native_build = vec!["".to_string(), "build".to_string()];
        assert!(validate_declaration("board", &p).is_err());
    }

    #[test]
    fn entry_escaping_the_plugin_directory_fails_before_build() {
        let root = temp_dir("escape");
        let p = plugin("../../bin/evil");
        let error = validate_declaration("board", &p).unwrap_err().to_string();
        assert!(error.contains("safe relative path"), "{error}");
        let _ = root;
    }

    #[test]
    fn build_reports_a_missing_binary() {
        let root = temp_dir("missing");
        std::fs::create_dir_all(root.join("plugins/board")).unwrap();
        let error = build(
            &root,
            "board",
            &plugin("target/release/board"),
            &root.join("manifests"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("not found"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn build_reports_a_failed_command() {
        let root = temp_dir("failed-command");
        std::fs::create_dir_all(root.join("plugins/board")).unwrap();
        let mut p = plugin("board");
        p.native_build = vec!["sh".to_string(), "-c".to_string(), "exit 3".to_string()];
        let error = build(&root, "board", &p, &root.join("manifests"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("native_build failed"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn build_writes_an_entry_with_a_digest() {
        let root = temp_dir("build-ok");
        std::fs::create_dir_all(root.join("plugins/board")).unwrap();
        let mut p = plugin("board");
        p.native_build = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf '#!/bin/sh\\nexit 0\\n' > board && chmod +x board".to_string(),
        ];
        let entry = build(&root, "board", &p, &root.join("manifests")).unwrap();
        assert_eq!(entry.name, "board");
        assert_eq!(entry.binary, "plugins/board/board");
        assert_eq!(entry.sha256.len(), 64);
        assert_eq!(entry.trust, PLUGIN_TRUST);
    }

    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[cfg(unix)]
    fn script_for_process(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = script(dir, name, body);
        // Avoid ETXTBSY while the test's write handle is still closing.
        std::thread::sleep(std::time::Duration::from_millis(50));
        path
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_reports_crashes_and_does_not_restart() {
        let dir = temp_dir("supervisor-crash");
        let binary = script_for_process(&dir, "crash", "exit 4");
        let (log, lines) = log();
        let mut supervisor = Supervisor::new(
            vec![NativeUiSpec {
                name: "crash".to_string(),
                binary,
            }],
            "http://127.0.0.1:8787",
            dir.clone(),
            log,
        );
        supervisor.start("crash").unwrap();
        assert_eq!(supervisor.status("crash"), Some(ProcessState::Running));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while supervisor.status("crash") == Some(ProcessState::Running)
            && std::time::Instant::now() < deadline
        {
            supervisor.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(supervisor.status("crash"), Some(ProcessState::Failed));
        let lines = lines.lock().unwrap().join("\n");
        assert!(lines.contains("exited with"), "{lines}");
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_stops_a_long_running_child() {
        let dir = temp_dir("supervisor-long");
        let binary = script_for_process(&dir, "long", "sleep 30");
        let (log, _) = log();
        let mut supervisor = Supervisor::new(
            vec![NativeUiSpec {
                name: "long".to_string(),
                binary,
            }],
            "http://127.0.0.1:8787",
            dir,
            log,
        );
        supervisor.start("long").unwrap();
        supervisor.stop("long").unwrap();
        assert_eq!(supervisor.status("long"), Some(ProcessState::Stopped));
    }

    #[cfg(unix)]
    #[test]
    fn desktop_sidecars_use_the_host_triple_and_generate_external_bin() {
        let root = temp_dir("sidecar");
        std::fs::create_dir_all(root.join("plugins/board")).unwrap();
        std::fs::write(root.join("plugins/board/board"), b"binary").unwrap();
        std::fs::create_dir_all(root.join("desktop")).unwrap();
        let config = root.join("desktop/tauri.generated.conf.json");
        std::fs::write(&config, br#"{"bundle":{"resources":[]}}"#).unwrap();
        let plugins = std::collections::BTreeMap::from([("board".to_string(), plugin("board"))]);

        let count =
            stage_tauri_sidecars(&root, &plugins, &root.join("desktop"), &config, None).unwrap();
        assert_eq!(count, 1);
        let host = host_triple().unwrap();
        assert!(root
            .join("desktop")
            .join(format!("binaries/board-{host}"))
            .is_file());
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(value["bundle"]["externalBin"][0], "binaries/board");

        let error = stage_tauri_sidecars(
            &root,
            &plugins,
            &root.join("desktop"),
            &config,
            Some("aarch64-unknown-linux-gnu"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("not cross-compiled"), "{error}");
    }

    #[test]
    fn index_round_trip_keeps_trust_disclosure() {
        let dir = temp_dir("index");
        let entries = vec![IndexEntry {
            name: "board".to_string(),
            kind: "native-ui".to_string(),
            runtime: "native".to_string(),
            binary: "plugins/board/board".to_string(),
            sha256: "a".repeat(64),
            trust: PLUGIN_TRUST.to_string(),
        }];
        write_index(&dir, entries.clone()).unwrap();
        let index = read_index(&dir).unwrap().unwrap();
        assert_eq!(index.schema, SCHEMA);
        assert_eq!(index.trust, TRUST);
        assert_eq!(index.plugins, entries);
    }
}
