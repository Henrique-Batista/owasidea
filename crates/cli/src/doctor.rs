//! Toolchain checks and install planning for `owasidea doctor`.
//!
//! The planner is pure: it takes `tools.lock`, the current OS, and a set of
//! probe results and returns a [`Plan`]. Tests can therefore cover Linux,
//! macOS and Windows plans without running a package manager. Execution stays
//! in `main.rs`, which only knows how to run a planned command or the built-in
//! wasi-sdk installer.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Tool name used by `tools.lock` for the built-in wasi-sdk installer.
pub const WASI_SDK_TOOL: &str = "wasi-sdk";
/// Human/JSON representation of the built-in wasi-sdk install action.
pub const WASI_SDK_COMMAND: &str = "owasidea doctor --install-wasi-sdk";

/// Canonical install order. Dependencies come before the tools they enable:
/// rustup before cargo-installed tools, Go before componentize-go, Node/npm
/// before jco, and .NET before the WASI workload.
pub const TOOL_ORDER: &[&str] = &[
    "rustc",
    "cargo",
    "cargo-component",
    "wasm-tools",
    "go",
    "node",
    "npm",
    "jco",
    "dotnet",
    "componentize-go",
    "componentize-dotnet",
    "trunk",
    "wasmtime",
    "python3",
    "wasi-sdk",
];

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct ToolLock {
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub min_version: Option<String>,
    /// Optional tools never make `doctor` exit non-zero.
    #[serde(default)]
    pub optional: bool,
    pub install: Option<InstallCmds>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct InstallCmds {
    pub linux: Option<String>,
    pub macos: Option<String>,
    pub windows: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
    Unknown,
}

impl Os {
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Macos => "macos",
            Os::Windows => "windows",
            Os::Unknown => "unknown",
        }
    }
}

impl Os {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "linux" => Some(Self::Linux),
            "macos" | "mac" | "darwin" => Some(Self::Macos),
            "windows" | "win" => Some(Self::Windows),
            _ => None,
        }
    }
}

pub fn detect_os() -> Os {
    if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else {
        Os::Unknown
    }
}

/// Install command for a tool on one OS. wasi-sdk is special-cased to the
/// installer built into the CLI instead of a shell command.
pub fn install_command(spec: &ToolSpec, os: Os) -> Option<String> {
    if spec.name == WASI_SDK_TOOL {
        return Some(WASI_SDK_COMMAND.to_string());
    }
    let cmds = spec.install.as_ref()?;
    match os {
        Os::Linux => cmds.linux.clone(),
        Os::Macos => cmds.macos.clone(),
        Os::Windows => cmds.windows.clone(),
        Os::Unknown => None,
    }
}

pub fn parse_lock(content: &str) -> Result<ToolLock, toml::de::Error> {
    toml::from_str(content)
}

pub fn load_lock(path: &std::path::Path) -> anyhow::Result<ToolLock> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        anyhow::anyhow!(
            "cannot read {}: {error}; run `owasidea doctor` from the repository root",
            path.display()
        )
    })?;
    parse_lock(&content).map_err(|error| anyhow::anyhow!("invalid {}: {error}", path.display()))
}

/// Locate `tools.lock` in the current directory or its nearest ancestor, so
/// `doctor` also works from `crates/cli` or a nested app directory.
pub fn lock_path() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    cwd.ancestors()
        .map(|dir| dir.join("tools.lock"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| cwd.join("tools.lock"))
}

/// Version comparison used by the planner. The probe may return a whole
/// version line (`rustc 1.90.0 (...)`), so the first version-looking token is
/// extracted before falling back to a plain string comparison.
pub fn version_ok(actual: Option<&str>, min: Option<&str>) -> bool {
    let Some(min) = min else {
        return true;
    };
    let Some(actual) = actual else {
        return false;
    };
    match (parse_version(actual), parse_version(min)) {
        (Some(actual), Some(min)) => actual >= min,
        _ => actual.trim() >= min.trim(),
    }
}

fn parse_version(value: &str) -> Option<semver::Version> {
    let token = extract_version(value).or_else(|| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })?;
    let mut parts = token.split('.').collect::<Vec<_>>();
    while parts.len() < 3 {
        parts.push("0");
    }
    semver::Version::parse(&parts.join(".")).ok()
}

/// Extract the first `1`, `1.2` or `1.2.3` token from a version banner.
fn extract_version(value: &str) -> Option<String> {
    for token in value.split_whitespace() {
        let Some(start) = token.find(|c: char| c.is_ascii_digit()) else {
            continue;
        };
        let candidate = &token[start..];
        let end = candidate
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(candidate.len());
        let candidate = candidate[..end].trim_end_matches('.');
        if candidate.is_empty() {
            continue;
        }
        if candidate
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(candidate.to_string());
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    pub name: String,
    pub present: bool,
    pub version: Option<String>,
}

impl ToolStatus {
    pub fn present(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            present: true,
            version: None,
        }
    }

    pub fn missing(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            present: false,
            version: None,
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

fn probe_argv(name: &str) -> Option<(&'static str, &'static [&'static str])> {
    match name {
        "rustc" => Some(("rustc", &["--version"])),
        "cargo" => Some(("cargo", &["--version"])),
        "cargo-component" => Some(("cargo", &["component", "--version"])),
        "wasm-tools" => Some(("wasm-tools", &["--version"])),
        "go" => Some(("go", &["version"])),
        "node" => Some(("node", &["--version"])),
        "npm" => Some(("npm", &["--version"])),
        "jco" => Some(("jco", &["--version"])),
        "dotnet" => Some(("dotnet", &["--version"])),
        "componentize-go" => Some(("componentize-go", &["--version"])),
        "componentize-dotnet" => Some(("componentize-dotnet", &["--version"])),
        "trunk" => Some(("trunk", &["--version"])),
        "wasmtime" => Some(("wasmtime", &["--version"])),
        "python3" => Some(("python3", &["--version"])),
        _ => None,
    }
}

fn probe_command(program: &str, args: &[&str]) -> Option<String> {
    let output = probe_process(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Some(text)
}

/// PATH additions for tools installed during this doctor run. Windows does
/// not update the parent process PATH when an installer finishes, so probes and
/// follow-up installers must see the usual install locations explicitly.
#[cfg(windows)]
pub fn windows_path() -> Option<std::ffi::OsString> {
    let mut dirs = Vec::new();
    let mut push = |base: Option<std::ffi::OsString>, rel: &str| {
        if let Some(base) = base {
            dirs.push(std::path::PathBuf::from(base).join(rel));
        }
    };
    let user = std::env::var_os("USERPROFILE");
    let appdata = std::env::var_os("APPDATA");
    let local = std::env::var_os("LOCALAPPDATA");
    let program_files = std::env::var_os("ProgramFiles");
    push(user.clone(), ".cargo/bin");
    push(user, "go/bin");
    push(appdata.clone(), "npm");
    push(program_files.clone(), "nodejs");
    push(program_files.clone(), "dotnet");
    push(program_files.clone(), "Go/bin");
    push(local.clone(), "Programs/Go/bin");
    push(local, "Microsoft/WinGet/Links");
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(dirs).ok()
}

#[cfg(not(windows))]
pub fn windows_path() -> Option<std::ffi::OsString> {
    None
}

fn probe_process(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        // npm-installed shims (npm, jco) are .cmd files and cannot be started
        // directly by CreateProcess.
        if matches!(program, "npm" | "npx" | "jco") {
            let mut command = std::process::Command::new("cmd");
            command.arg("/C").arg(program);
            return command;
        }
    }
    let mut command = std::process::Command::new(program);
    if let Some(path) = windows_path() {
        command.env("PATH", path);
    }
    command
}

fn probe_spec(spec: &ToolSpec, wasi_sdk_present: Option<bool>) -> ToolStatus {
    if spec.name == WASI_SDK_TOOL {
        return ToolStatus {
            name: spec.name.clone(),
            present: wasi_sdk_present.unwrap_or(false),
            version: None,
        };
    }
    let Some((program, args)) = probe_argv(&spec.name) else {
        return ToolStatus::missing(spec.name.clone());
    };
    let Some(text) = probe_command(program, args) else {
        // componentize-dotnet is a NuGet SDK: the executable is optional, so
        // the dotnet WASI workload is the meaningful presence check.
        if spec.name == "componentize-dotnet"
            && probe_command("dotnet", &["workload", "list"])
                .is_some_and(|list| list.to_ascii_lowercase().contains("wasi"))
        {
            return ToolStatus {
                name: spec.name.clone(),
                present: true,
                version: None,
            };
        }
        return ToolStatus::missing(spec.name.clone());
    };
    let version = extract_version(&text);
    ToolStatus {
        name: spec.name.clone(),
        present: true,
        version,
    }
}

/// Probe every selected spec. `wasi_sdk_present` comes from the CLI's own
/// `find_wasi_sdk()` so the two detection paths cannot drift.
pub fn probe_tools(specs: &[ToolSpec], wasi_sdk_present: Option<bool>) -> Vec<ToolStatus> {
    specs
        .iter()
        .map(|spec| probe_spec(spec, wasi_sdk_present))
        .collect()
}

/// What to plan for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    mode: SelectionMode,
    extra: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionMode {
    All,
    MissingRequired,
    Explicit,
}

impl Selection {
    pub fn all() -> Self {
        Self {
            mode: SelectionMode::All,
            extra: Vec::new(),
        }
    }

    pub fn missing_required() -> Self {
        Self {
            mode: SelectionMode::MissingRequired,
            extra: Vec::new(),
        }
    }

    pub fn tools(names: Vec<String>) -> Self {
        Self {
            mode: SelectionMode::Explicit,
            extra: names,
        }
    }

    /// Add a tool to the selection (used for the legacy `--install-wasi-sdk`
    /// flag when it is combined with another install mode).
    pub fn plus(mut self, name: &str) -> Self {
        if !self.extra.iter().any(|extra| extra == name) {
            self.extra.push(name.to_string());
        }
        self
    }
}

fn unknown_tool_error(name: &str, lock: &ToolLock) -> String {
    let mut valid = lock
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();
    valid.sort_unstable();
    format!("unknown tool `{name}`; valid tools: {}", valid.join(", "))
}

/// Resolve the selection against the lock, validate names, and sort by the
/// canonical install order (so a hand-edited lock cannot break dependencies).
pub fn resolve_selection<'a>(
    lock: &'a ToolLock,
    selection: &Selection,
) -> Result<Vec<&'a ToolSpec>, String> {
    for name in &selection.extra {
        if !lock.tools.iter().any(|tool| &tool.name == name) {
            return Err(unknown_tool_error(name, lock));
        }
    }

    let mut specs = match selection.mode {
        SelectionMode::All => lock.tools.iter().collect::<Vec<_>>(),
        SelectionMode::MissingRequired => lock
            .tools
            .iter()
            .filter(|tool| !tool.optional)
            .collect::<Vec<_>>(),
        SelectionMode::Explicit => lock
            .tools
            .iter()
            .filter(|tool| selection.extra.contains(&tool.name))
            .collect::<Vec<_>>(),
    };
    if selection.mode == SelectionMode::MissingRequired {
        for name in &selection.extra {
            if let Some(spec) = lock.tools.iter().find(|tool| &tool.name == name) {
                specs.push(spec);
            }
        }
    }

    specs.sort_by_key(|spec| {
        (
            TOOL_ORDER
                .iter()
                .position(|name| name == &spec.name)
                .unwrap_or(TOOL_ORDER.len()),
            spec.name.clone(),
        )
    });
    specs.dedup_by(|left, right| left.name == right.name);
    Ok(specs)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanAction {
    /// Command should be printed and, in execute mode, run.
    Install,
    /// Tool is already present (or the command is covered by another entry).
    Skip,
    /// Command contains sudo and `--allow-sudo` was not given.
    SkipSudo,
    /// No install command exists for this OS or the tool needs manual setup.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedCommand {
    pub tool: String,
    pub required: bool,
    pub present: bool,
    pub action: PlanAction,
    pub command: Option<String>,
    pub platform: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanOptions {
    pub execute: bool,
    pub allow_sudo: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PlanSummary {
    pub total: usize,
    pub install: usize,
    pub skip: usize,
    pub skip_sudo: usize,
    pub manual: usize,
    /// Number of commands actually run in execute mode.
    pub executed: usize,
    /// Commands that exited non-zero.
    pub failed: usize,
    /// Required tools still missing or below their minimum version.
    pub missing_required: Vec<String>,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub platform: String,
    pub dry_run: bool,
    pub allow_sudo: bool,
    pub entries: Vec<PlannedCommand>,
    pub summary: PlanSummary,
}

/// Options collected by the `doctor` clap arm. The selection rules live here
/// so `main.rs` only constructs this struct and dispatches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorOptions {
    pub json: bool,
    pub platform: Option<String>,
    pub install_wasi_sdk: bool,
    pub fix: bool,
    pub yes: bool,
    pub execute: bool,
    pub dry_run: bool,
    pub allow_sudo: bool,
    pub force: bool,
    pub install_all: bool,
    pub install: Vec<String>,
}

impl DoctorOptions {
    pub fn execute(&self) -> bool {
        (self.yes || self.execute) && !self.dry_run
    }

    /// `--install-wasi-sdk` alone keeps its historical behaviour: download
    /// immediately, then print the normal doctor report.
    pub fn legacy_wasi_sdk_only(&self) -> bool {
        self.install_wasi_sdk
            && self.platform.is_none()
            && !self.dry_run
            && !self.install_all
            && self.install.is_empty()
            && !self.fix
            && !self.yes
            && !self.execute
    }

    pub fn selection(&self) -> Option<Selection> {
        if self.legacy_wasi_sdk_only() {
            return None;
        }
        if self.install_all {
            return Some(Selection::all());
        }
        if !self.install.is_empty() {
            let selection = Selection::tools(self.install.clone());
            return Some(if self.install_wasi_sdk {
                selection.plus(WASI_SDK_TOOL)
            } else {
                selection
            });
        }
        if self.fix {
            let selection = Selection::missing_required();
            return Some(if self.install_wasi_sdk {
                selection.plus(WASI_SDK_TOOL)
            } else {
                selection
            });
        }
        if self.install_wasi_sdk {
            return Some(Selection::tools(vec![WASI_SDK_TOOL.to_string()]));
        }
        if self.platform.is_some() || self.dry_run {
            return Some(Selection::missing_required());
        }
        None
    }
}

fn is_sudo_command(command: &str) -> bool {
    command
        .split(|c: char| {
            c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '"' | '\'' | '`')
        })
        .any(|word| word == "sudo")
}

fn installed_reason(version: Option<&str>, min: Option<&str>) -> String {
    match (version, min) {
        (Some(version), Some(min)) => format!("already installed ({version}; minimum {min})"),
        (Some(version), None) => format!("already installed ({version})"),
        (None, _) => "already installed".to_string(),
    }
}

/// Build a pure install plan.
pub fn build_plan(
    lock: &ToolLock,
    os: Os,
    statuses: &[ToolStatus],
    selection: &Selection,
    options: PlanOptions,
) -> Result<Plan, String> {
    let specs = resolve_selection(lock, selection)?;
    let status_by_name = statuses
        .iter()
        .map(|status| (status.name.as_str(), status))
        .collect::<HashMap<_, _>>();
    let mut entries = Vec::new();
    let mut planned_commands: HashMap<String, String> = HashMap::new();
    let mut summary = PlanSummary::default();

    for spec in specs {
        let status = status_by_name.get(spec.name.as_str()).copied();
        let present = status.is_some_and(|status| status.present);
        let version = status.and_then(|status| status.version.as_deref());
        // Only fail a present tool when the probe actually reported a version
        // that is below the minimum. A present tool with an unparseable banner
        // is treated as satisfied, matching the old presence-only behaviour.
        let outdated =
            present && version.is_some() && !version_ok(version, spec.min_version.as_deref());
        let should_install = !present || outdated || options.force;
        let required = !spec.optional;

        let mut action;
        let mut command;
        let mut reason;
        if !should_install {
            action = PlanAction::Skip;
            command = None;
            reason = installed_reason(version, spec.min_version.as_deref());
        } else {
            command = install_command(spec, os);
            match command.as_deref() {
                None => {
                    action = PlanAction::Manual;
                    reason = format!("no install command for {} in tools.lock", os.as_str());
                }
                Some(_)
                    if is_sudo_command(command.as_deref().unwrap_or_default())
                        && options.execute
                        && !options.allow_sudo =>
                {
                    action = PlanAction::SkipSudo;
                    reason = "requires sudo; not executed without --allow-sudo".to_string();
                }
                Some(_) => {
                    action = PlanAction::Install;
                    reason = if outdated {
                        format!(
                            "installed version {} is below required {}",
                            version.unwrap_or("unknown"),
                            spec.min_version.as_deref().unwrap_or("unknown")
                        )
                    } else if options.force && present {
                        "forced reinstall".to_string()
                    } else {
                        "not found".to_string()
                    };
                    if !options.execute && is_sudo_command(command.as_deref().unwrap_or_default()) {
                        reason.push_str("; sudo only runs with --yes --allow-sudo");
                    }
                }
            }
        }

        // Two required tools can share one installer (node/npm on macOS and
        // Windows). Keep both entries for the JSON contract, but run the
        // command once.
        if matches!(action, PlanAction::Install | PlanAction::SkipSudo) {
            if let Some(planned) = command.as_deref() {
                if let Some(first) = planned_commands.get(planned) {
                    action = PlanAction::Skip;
                    reason = format!("identical install command already planned for `{first}`");
                    command = None;
                } else {
                    planned_commands.insert(planned.to_string(), spec.name.clone());
                }
            }
        }

        match action {
            PlanAction::Install => summary.install += 1,
            PlanAction::Skip => summary.skip += 1,
            PlanAction::SkipSudo => summary.skip_sudo += 1,
            PlanAction::Manual => summary.manual += 1,
        }
        if required && (!present || outdated) {
            summary.missing_required.push(spec.name.clone());
        }

        entries.push(PlannedCommand {
            tool: spec.name.clone(),
            required,
            present,
            action,
            command,
            platform: os.as_str().to_string(),
            reason,
        });
    }

    summary.total = entries.len();
    summary.ok = summary.missing_required.is_empty();
    Ok(Plan {
        platform: os.as_str().to_string(),
        dry_run: !options.execute,
        allow_sudo: options.allow_sudo,
        entries,
        summary,
    })
}

/// Required tools in the selection that are still missing or too old.
pub fn missing_required(
    lock: &ToolLock,
    selection: &Selection,
    statuses: &[ToolStatus],
) -> Vec<String> {
    let Ok(specs) = resolve_selection(lock, selection) else {
        return Vec::new();
    };
    let status_by_name = statuses
        .iter()
        .map(|status| (status.name.as_str(), status))
        .collect::<HashMap<_, _>>();
    let mut missing = Vec::new();
    for spec in specs {
        if spec.optional {
            continue;
        }
        let status = status_by_name.get(spec.name.as_str()).copied();
        let present = status.is_some_and(|status| status.present);
        let version = status.and_then(|status| status.version.as_deref());
        let outdated =
            present && version.is_some() && !version_ok(version, spec.min_version.as_deref());
        if !present || outdated {
            missing.push(spec.name.clone());
        }
    }
    missing
}

impl Plan {
    /// Fold post-execution probe results back into the serialized plan. This
    /// is what makes the exit status reflect the machine after the run.
    pub fn apply_outcome(
        &mut self,
        lock: &ToolLock,
        selection: &Selection,
        statuses: &[ToolStatus],
        executed: usize,
        failed: usize,
    ) {
        let status_by_name = statuses
            .iter()
            .map(|status| (status.name.as_str(), status))
            .collect::<HashMap<_, _>>();
        for entry in &mut self.entries {
            if let Some(status) = status_by_name.get(entry.tool.as_str()) {
                entry.present = status.present;
            }
        }
        self.summary.executed = executed;
        self.summary.failed = failed;
        self.summary.missing_required = missing_required(lock, selection, statuses);
        self.summary.ok = self.summary.missing_required.is_empty();
    }
}

fn action_label(action: PlanAction) -> &'static str {
    match action {
        PlanAction::Install => "install",
        PlanAction::Skip => "skip",
        PlanAction::SkipSudo => "SKIP sudo",
        PlanAction::Manual => "manual",
    }
}

pub fn render_plan_entries(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Install plan for {} ({})",
        plan.platform,
        if plan.dry_run { "dry-run" } else { "execute" }
    );
    for entry in &plan.entries {
        let detail = match entry.action {
            PlanAction::Install | PlanAction::SkipSudo => entry
                .command
                .as_deref()
                .map(|command| format!("{command}  [{}]", entry.reason))
                .unwrap_or_else(|| entry.reason.clone()),
            PlanAction::Skip | PlanAction::Manual => entry.reason.clone(),
        };
        let _ = writeln!(
            out,
            "  {:<19} {:<10} {}",
            entry.tool,
            action_label(entry.action),
            detail
        );
    }
    out
}

pub fn render_summary(plan: &Plan) -> String {
    let mut out = String::new();
    let missing = if plan.summary.missing_required.is_empty() {
        "none".to_string()
    } else {
        plan.summary.missing_required.join(", ")
    };
    let _ = writeln!(
        out,
        "Summary: {} tools, {} install, {} skip, {} skip-sudo, {} manual; executed {}, failed {}; missing required: {}",
        plan.summary.total,
        plan.summary.install,
        plan.summary.skip,
        plan.summary.skip_sudo,
        plan.summary.manual,
        plan.summary.executed,
        plan.summary.failed,
        missing
    );
    out
}

pub fn render_plan(plan: &Plan) -> String {
    let mut out = render_plan_entries(plan);
    out.push_str(&render_summary(plan));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOCK: &str = r#"
[[tools]]
name = "rustc"
min_version = "1.80"
[tools.install]
linux = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
macos = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
windows = "winget install --id Rustlang.Rustup -e"

[[tools]]
name = "cargo"
min_version = "1.80"
[tools.install]
linux = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
macos = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
windows = "winget install --id Rustlang.Rustup -e"

[[tools]]
name = "cargo-component"
[tools.install]
linux = "cargo install cargo-component --locked"
macos = "cargo install cargo-component --locked"
windows = "cargo install cargo-component --locked"

[[tools]]
name = "wasm-tools"
[tools.install]
linux = "cargo install wasm-tools --locked"
macos = "cargo install wasm-tools --locked"
windows = "cargo install wasm-tools --locked"

[[tools]]
name = "go"
min_version = "1.27.1"
[tools.install]
linux = "sudo apt-get install -y golang-go"
macos = "brew install go"
windows = "winget install --id GoLang.Go -e"

[[tools]]
name = "node"
min_version = "20"
[tools.install]
linux = "sudo apt-get install -y nodejs"
macos = "brew install node@20"
windows = "winget install --id OpenJS.NodeJS.LTS -e"

[[tools]]
name = "npm"
[tools.install]
linux = "sudo apt-get install -y npm"
macos = "brew install node@20"
windows = "winget install --id OpenJS.NodeJS.LTS -e"

[[tools]]
name = "jco"
optional = true
min_version = "1.39.0"
[tools.install]
linux = "npm install -g @bytecodealliance/jco"
macos = "npm install -g @bytecodealliance/jco"
windows = "npm install -g @bytecodealliance/jco"

[[tools]]
name = "dotnet"
optional = true
min_version = "10.0"
[tools.install]
linux = "sudo apt-get install -y dotnet-sdk-10.0"
macos = "brew install --cask dotnet-sdk"
windows = "winget install --id Microsoft.DotNet.SDK.10 -e"

[[tools]]
name = "componentize-go"
optional = true
[tools.install]
linux = "go install github.com/bytecodealliance/componentize-go@latest"
macos = "go install github.com/bytecodealliance/componentize-go@latest"
windows = "go install github.com/bytecodealliance/componentize-go@latest"

[[tools]]
name = "componentize-dotnet"
optional = true
[tools.install]
linux = "dotnet workload install wasi-experimental"
macos = "dotnet workload install wasi-experimental"
windows = "dotnet workload install wasi-experimental"

[[tools]]
name = "trunk"
optional = true
[tools.install]
linux = "cargo install trunk --locked"
macos = "cargo install trunk --locked"
windows = "cargo install trunk --locked"

[[tools]]
name = "wasmtime"
optional = true
[tools.install]
linux = "curl -sSf https://wasmtime.dev/install.sh | bash"
macos = "brew install wasmtime"
windows = "winget install --id BytecodeAlliance.Wasmtime -e"

[[tools]]
name = "python3"
optional = true
[tools.install]
linux = "sudo apt-get install -y python3"
macos = "brew install python3"
windows = "winget install --id Python.Python.3.13 -e"

[[tools]]
name = "wasi-sdk"
optional = true
"#;

    fn lock() -> ToolLock {
        parse_lock(SAMPLE_LOCK).unwrap()
    }

    fn all_missing(lock: &ToolLock) -> Vec<ToolStatus> {
        lock.tools
            .iter()
            .map(|spec| ToolStatus::missing(spec.name.clone()))
            .collect()
    }

    fn all_present(lock: &ToolLock) -> Vec<ToolStatus> {
        lock.tools
            .iter()
            .map(|spec| ToolStatus::present(spec.name.clone()))
            .collect()
    }

    fn plan(os: Os, statuses: &[ToolStatus], selection: &Selection) -> Plan {
        build_plan(
            &lock(),
            os,
            statuses,
            selection,
            PlanOptions {
                execute: false,
                allow_sudo: false,
                force: false,
            },
        )
        .unwrap()
    }

    fn entry<'a>(plan: &'a Plan, tool: &str) -> &'a PlannedCommand {
        plan.entries
            .iter()
            .find(|entry| entry.tool == tool)
            .unwrap_or_else(|| panic!("missing plan entry for {tool}"))
    }

    #[test]
    fn parse_lock_basic() {
        let c = r#"
[[tools]]
name = "rustc"
min_version = "1.80"
[tools.install]
linux = "rustup toolchain install 1.80"
macos = "rustup toolchain install 1.80"
windows = "rustup toolchain install 1.80"
"#;
        let l = parse_lock(c).unwrap();
        assert_eq!(l.tools.len(), 1);
        assert_eq!(l.tools[0].name, "rustc");
        assert!(!l.tools[0].optional);
    }

    #[test]
    fn install_command_os() {
        let spec = ToolSpec {
            name: "x".into(),
            min_version: None,
            optional: false,
            install: Some(InstallCmds {
                linux: Some("apt install x".into()),
                macos: None,
                windows: None,
            }),
        };
        assert_eq!(
            install_command(&spec, Os::Linux),
            Some("apt install x".into())
        );
        assert_eq!(install_command(&spec, Os::Macos), None);
    }

    #[test]
    fn wasi_sdk_uses_the_builtin_installer_on_every_platform() {
        let spec = lock()
            .tools
            .into_iter()
            .find(|spec| spec.name == WASI_SDK_TOOL)
            .unwrap();
        for os in [Os::Linux, Os::Macos, Os::Windows] {
            assert_eq!(
                install_command(&spec, os),
                Some(WASI_SDK_COMMAND.to_string())
            );
        }
    }

    #[test]
    fn linux_plan_uses_linux_commands() {
        let l = lock();
        let p = plan(Os::Linux, &all_missing(&l), &Selection::all());
        assert_eq!(p.platform, "linux");
        assert!(p.dry_run);
        assert!(entry(&p, "go")
            .command
            .as_deref()
            .unwrap()
            .starts_with("sudo apt-get install -y golang-go"));
        assert!(entry(&p, "jco")
            .command
            .as_deref()
            .unwrap()
            .contains("npm install -g @bytecodealliance/jco"));
        assert_eq!(
            entry(&p, WASI_SDK_TOOL).command.as_deref(),
            Some(WASI_SDK_COMMAND)
        );
        assert_eq!(p.entries.len(), l.tools.len());
    }

    #[test]
    fn macos_plan_uses_macos_commands_and_deduplicates_node_npm() {
        let l = lock();
        let p = plan(Os::Macos, &all_missing(&l), &Selection::all());
        assert_eq!(p.platform, "macos");
        assert_eq!(entry(&p, "go").command.as_deref(), Some("brew install go"));
        assert_eq!(
            entry(&p, "dotnet").command.as_deref(),
            Some("brew install --cask dotnet-sdk")
        );
        assert!(entry(&p, "rustc")
            .command
            .as_deref()
            .unwrap()
            .contains("sh.rustup.rs"));

        let node_commands = p
            .entries
            .iter()
            .filter(|entry| {
                entry.action == PlanAction::Install
                    && entry.command.as_deref() == Some("brew install node@20")
            })
            .count();
        assert_eq!(node_commands, 1, "node/npm install must run once");
        assert_eq!(entry(&p, "npm").action, PlanAction::Skip);
        assert!(entry(&p, "npm").reason.contains("already planned"));
    }

    #[test]
    fn windows_plan_uses_winget_commands() {
        let l = lock();
        let p = plan(Os::Windows, &all_missing(&l), &Selection::all());
        assert_eq!(p.platform, "windows");
        assert_eq!(
            entry(&p, "go").command.as_deref(),
            Some("winget install --id GoLang.Go -e")
        );
        assert_eq!(
            entry(&p, "dotnet").command.as_deref(),
            Some("winget install --id Microsoft.DotNet.SDK.10 -e")
        );
        assert!(entry(&p, "componentize-dotnet")
            .command
            .as_deref()
            .unwrap()
            .contains("dotnet workload install wasi-experimental"));
    }

    #[test]
    fn plan_orders_dependencies_before_dependants() {
        let l = lock();
        let p = plan(Os::Linux, &all_missing(&l), &Selection::all());
        let index = |name: &str| {
            p.entries
                .iter()
                .position(|entry| entry.tool == name)
                .unwrap()
        };
        assert!(index("rustc") < index("cargo-component"));
        assert!(index("rustc") < index("wasm-tools"));
        assert!(index("go") < index("componentize-go"));
        assert!(index("node") < index("jco"));
        assert!(index("npm") < index("jco"));
        assert!(index("dotnet") < index("componentize-dotnet"));
        assert_eq!(index("rustc"), 0);
    }

    #[test]
    fn selected_tools_only_plan_those_tools() {
        let l = lock();
        let selection = Selection::tools(vec!["jco".into(), "trunk".into()]);
        let p = plan(Os::Linux, &all_missing(&l), &selection);
        let names = p
            .entries
            .iter()
            .map(|entry| entry.tool.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["jco", "trunk"]);
        assert!(!entry(&p, "jco").required);
        assert!(p.summary.ok, "optional tools never block the exit status");
    }

    #[test]
    fn unknown_tool_is_an_error_listing_valid_names() {
        let l = lock();
        let error = build_plan(
            &l,
            Os::Linux,
            &all_missing(&l),
            &Selection::tools(vec!["nope".into()]),
            PlanOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("unknown tool `nope`"), "{error}");
        assert!(error.contains("cargo-component"), "{error}");
        assert!(error.contains(WASI_SDK_TOOL), "{error}");
    }

    #[test]
    fn sudo_commands_are_skipped_without_allow_sudo() {
        let l = lock();
        let statuses = all_missing(&l);
        let selection = Selection::tools(vec!["go".into()]);
        let blocked = build_plan(
            &l,
            Os::Linux,
            &statuses,
            &selection,
            PlanOptions {
                execute: true,
                allow_sudo: false,
                force: false,
            },
        )
        .unwrap();
        let go = entry(&blocked, "go");
        assert_eq!(go.action, PlanAction::SkipSudo);
        assert_eq!(
            go.command.as_deref(),
            Some("sudo apt-get install -y golang-go")
        );
        assert!(go.reason.contains("--allow-sudo"));
        assert_eq!(blocked.summary.skip_sudo, 1);
        assert_eq!(blocked.summary.executed, 0);

        let allowed = build_plan(
            &l,
            Os::Linux,
            &statuses,
            &selection,
            PlanOptions {
                execute: true,
                allow_sudo: true,
                force: false,
            },
        )
        .unwrap();
        assert_eq!(entry(&allowed, "go").action, PlanAction::Install);
    }

    #[test]
    fn dry_run_keeps_sudo_commands_as_planned_installs() {
        let l = lock();
        let p = plan(
            Os::Linux,
            &all_missing(&l),
            &Selection::tools(vec!["go".into()]),
        );
        let go = entry(&p, "go");
        assert_eq!(go.action, PlanAction::Install);
        assert!(go.reason.contains("sudo only runs with --yes --allow-sudo"));
    }

    #[test]
    fn json_shape_is_stable() {
        let l = lock();
        let p = plan(Os::Linux, &all_missing(&l), &Selection::all());
        let value = serde_json::to_value(&p).unwrap();
        assert_eq!(value["platform"], "linux");
        assert_eq!(value["dry_run"], true);
        let entries = value["entries"].as_array().unwrap();
        assert_eq!(entries.len(), p.entries.len());
        for entry in entries {
            let object = entry.as_object().unwrap();
            let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec!["action", "command", "platform", "present", "reason", "required", "tool"]
            );
        }
        assert_eq!(value["summary"]["total"], entries.len());
        assert!(value["summary"]["missing_required"].is_array());
        assert!(value["summary"]["ok"].is_boolean());
    }

    #[test]
    fn required_and_optional_selection_differ() {
        let l = lock();
        let statuses = all_missing(&l);
        let required = plan(Os::Linux, &statuses, &Selection::missing_required());
        assert!(required.entries.iter().all(|entry| entry.required));
        assert!(required.entries.iter().any(|entry| entry.tool == "rustc"));
        assert!(!required.entries.iter().any(|entry| entry.tool == "jco"));

        let all = plan(Os::Linux, &statuses, &Selection::all());
        assert!(all.entries.iter().any(|entry| entry.tool == "jco"));
        assert!(all.entries.iter().any(|entry| entry.tool == WASI_SDK_TOOL));
        assert!(all.entries.iter().all(|entry| !entry.present));
        assert!(!all.summary.ok);
    }

    #[test]
    fn present_tools_are_skipped() {
        let l = lock();
        let statuses = all_present(&l);
        let p = plan(Os::Linux, &statuses, &Selection::all());
        assert!(p
            .entries
            .iter()
            .all(|entry| entry.action == PlanAction::Skip));
        assert_eq!(p.summary.install, 0);
        assert_eq!(p.summary.total, p.summary.skip);
        assert!(p.summary.ok);
    }

    #[test]
    fn outdated_tool_is_reinstalled_even_when_present() {
        let l = lock();
        let mut statuses = all_present(&l);
        for status in &mut statuses {
            if status.name == "rustc" {
                status.version = Some("1.70.0".into());
            }
        }
        let p = plan(
            Os::Linux,
            &statuses,
            &Selection::tools(vec!["rustc".into()]),
        );
        let rustc = entry(&p, "rustc");
        assert!(rustc.present);
        assert_eq!(rustc.action, PlanAction::Install);
        assert!(rustc.reason.contains("1.70.0"));
        assert!(!p.summary.ok);
        assert_eq!(p.summary.missing_required, vec!["rustc"]);
    }

    #[test]
    fn force_replans_install_for_present_tool() {
        let l = lock();
        let statuses = all_present(&l);
        let p = build_plan(
            &l,
            Os::Linux,
            &statuses,
            &Selection::tools(vec!["wasm-tools".into()]),
            PlanOptions {
                execute: false,
                allow_sudo: false,
                force: true,
            },
        )
        .unwrap();
        assert_eq!(entry(&p, "wasm-tools").action, PlanAction::Install);
        assert!(entry(&p, "wasm-tools").reason.contains("forced"));
    }

    #[test]
    fn apply_outcome_updates_presence_and_exit_status() {
        let l = lock();
        let selection = Selection::tools(vec!["go".into()]);
        let mut p = plan(
            Os::Linux,
            &all_missing(&l),
            &Selection::tools(vec!["go".into()]),
        );
        assert!(!p.summary.ok);
        let now_present = vec![ToolStatus::present("go").with_version("1.27.1")];
        p.apply_outcome(&l, &selection, &now_present, 1, 0);
        assert!(p.entries[0].present);
        assert_eq!(p.summary.executed, 1);
        assert_eq!(p.summary.failed, 0);
        assert!(p.summary.ok);
    }

    #[test]
    fn doctor_options_selection_prefers_explicit_flags() {
        let legacy = DoctorOptions {
            install_wasi_sdk: true,
            ..DoctorOptions::default()
        };
        assert!(legacy.legacy_wasi_sdk_only());
        assert!(legacy.selection().is_none());

        let dry = DoctorOptions {
            install_wasi_sdk: true,
            dry_run: true,
            ..DoctorOptions::default()
        };
        assert_eq!(
            dry.selection(),
            Some(Selection::tools(vec![WASI_SDK_TOOL.to_string()]))
        );

        let fix = DoctorOptions {
            fix: true,
            install_wasi_sdk: true,
            ..DoctorOptions::default()
        };
        let selection = fix.selection().unwrap();
        assert_eq!(selection.mode, SelectionMode::MissingRequired);
        assert_eq!(selection.extra, vec![WASI_SDK_TOOL.to_string()]);

        let explicit = DoctorOptions {
            fix: true,
            install: vec!["jco".into()],
            ..DoctorOptions::default()
        };
        assert_eq!(
            explicit.selection(),
            Some(Selection::tools(vec!["jco".into()]))
        );

        assert_eq!(
            DoctorOptions {
                install_all: true,
                ..DoctorOptions::default()
            }
            .selection(),
            Some(Selection::all())
        );
        assert!(DoctorOptions::default().selection().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_includes_tool_install_dirs() {
        let path = windows_path().expect("PATH");
        let text = path.to_string_lossy().to_lowercase();
        assert!(text.contains(".cargo"), "{text}");
        assert!(text.contains("npm"), "{text}");
        assert!(text.contains("dotnet"), "{text}");
    }

    #[test]
    fn version_ok_works() {
        assert!(version_ok(Some("1.90"), Some("1.80")));
        assert!(!version_ok(Some("1.70"), Some("1.80")));
        assert!(version_ok(None, None));
        assert!(!version_ok(None, Some("1.80")));
        assert!(version_ok(Some("rustc 1.90.0 (abc)"), Some("1.80")));
        assert!(version_ok(
            Some("go version go1.27.1 linux/amd64"),
            Some("1.27.1")
        ));
        assert!(version_ok(Some("1.10.0"), Some("1.9.0")));
        assert!(!version_ok(Some("1.9.0"), Some("1.10.0")));
        assert!(version_ok(Some("v20.11.0"), Some("20")));
    }

    #[test]
    fn extract_version_reads_common_banners() {
        assert_eq!(extract_version("rustc 1.90.0 (abc)"), Some("1.90.0".into()));
        assert_eq!(
            extract_version("go version go1.27.1 linux/amd64"),
            Some("1.27.1".into())
        );
        assert_eq!(extract_version("v20.11.0"), Some("20.11.0".into()));
        assert_eq!(extract_version("no version here"), None);
    }

    #[test]
    fn render_plan_mentions_skipped_sudo_and_summary() {
        let l = lock();
        let p = build_plan(
            &l,
            Os::Linux,
            &all_missing(&l),
            &Selection::tools(vec!["go".into()]),
            PlanOptions {
                execute: true,
                allow_sudo: false,
                force: false,
            },
        )
        .unwrap();
        let text = render_plan(&p);
        assert!(text.contains("SKIP sudo"), "{text}");
        assert!(text.contains("sudo apt-get install -y golang-go"), "{text}");
        assert!(text.contains("--allow-sudo"), "{text}");
        assert!(text.contains("Summary:"), "{text}");
    }
}
