//! Pure dashboard state. Key presses and background results go in, `Action`s
//! come out. Nothing here touches the terminal or the network, so the whole
//! interaction model is unit-tested.

use super::api::{LogLine, PluginRow, ReloadRecord, Snapshot};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use serde_json::Value;
use std::collections::VecDeque;

pub const OUTPUT_CAP: usize = 2000;
pub const LOG_CAP: usize = 500;
pub const KEY_HINTS: &str =
    "↑↓ plugin · r reload · s service · R rebuild · o open UI · ? help · q quit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Manifest,
    Call,
    Logs,
    Output,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Overview,
        Tab::Manifest,
        Tab::Call,
        Tab::Logs,
        Tab::Output,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Manifest => "Manifest",
            Tab::Call => "Call",
            Tab::Logs => "Logs",
            Tab::Output => "Output",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    fn step(self, delta: isize) -> Tab {
        let len = Tab::ALL.len() as isize;
        let next = (self.index() as isize + delta).rem_euclid(len);
        Tab::ALL[next as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputLine {
    pub source: String,
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Reload(String),
    ServiceStart(String),
    ServiceStop(String),
    Rebuild {
        reload: Vec<String>,
    },
    Call {
        plugin: String,
        function: String,
        args: Vec<Value>,
    },
    OpenUi,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallForm {
    pub export: usize,
    pub args: String,
    pub editing: bool,
    pub result: Option<Result<String, String>>,
}

pub struct App {
    pub app_name: String,
    pub host_url: String,
    pub ui_url: Option<String>,
    pub dev: bool,
    pub host_online: bool,
    pub build_id: Option<String>,
    pub watch: bool,
    pub hot_reload: bool,
    pub last_reload: Vec<ReloadRecord>,
    pub plugins: Vec<PluginRow>,
    pub selected: usize,
    pub tab: Tab,
    pub scroll: u16,
    pub logs: VecDeque<LogLine>,
    pub logs_for: Option<String>,
    pub output: VecDeque<OutputLine>,
    pub manifest: Option<Value>,
    pub manifest_for: Option<String>,
    pub manifest_requested: Option<String>,
    pub call: CallForm,
    pub status: Option<(Level, String)>,
    /// Last build failure. Kept until a rebuild succeeds so the footer cannot
    /// hide it behind a later transient status message.
    pub build_error: Option<String>,
    pub busy: Option<String>,
    pub help: bool,
    pub quit: bool,
}

impl App {
    pub fn new(app_name: String, host_url: String, ui_url: Option<String>, dev: bool) -> Self {
        Self {
            app_name,
            host_url,
            ui_url,
            dev,
            host_online: false,
            build_id: None,
            watch: false,
            hot_reload: false,
            last_reload: Vec::new(),
            plugins: Vec::new(),
            selected: 0,
            tab: Tab::Overview,
            scroll: 0,
            logs: VecDeque::new(),
            logs_for: None,
            output: VecDeque::new(),
            manifest: None,
            manifest_for: None,
            manifest_requested: None,
            call: CallForm::default(),
            status: None,
            build_error: None,
            busy: None,
            help: false,
            quit: false,
        }
    }

    pub fn selected_plugin(&self) -> Option<&PluginRow> {
        self.plugins.get(self.selected)
    }

    pub fn push_line(&mut self, line: OutputLine) {
        self.output.push_back(line);
        while self.output.len() > OUTPUT_CAP {
            self.output.pop_front();
        }
    }

    pub fn set_status(&mut self, level: Level, text: impl Into<String>) {
        self.status = Some((level, text.into()));
    }

    pub fn apply_done(&mut self, ok: bool, text: String, refresh_manifest: bool) {
        self.busy = None;
        if ok {
            self.build_error = None;
        } else if text.starts_with("build") {
            self.build_error = Some(text.clone());
        }
        self.set_status(if ok { Level::Info } else { Level::Error }, text);
        if refresh_manifest {
            self.forget_manifest();
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let previous = self.selected_plugin().map(|plugin| plugin.name.clone());
        self.host_online = true;
        self.build_id = snapshot.build_id;
        self.watch = snapshot.watch;
        self.hot_reload = snapshot.hot_reload;
        self.last_reload = snapshot.last_reload;
        self.plugins = snapshot.plugins;
        let index = previous
            .and_then(|name| self.plugins.iter().position(|plugin| plugin.name == name))
            .unwrap_or(0);
        self.selected = index;
        if self.plugins.is_empty() {
            self.selected = 0;
        }
    }

    pub fn apply_offline(&mut self) {
        self.host_online = false;
    }

    pub fn apply_logs(&mut self, plugin: &str, logs: Vec<LogLine>) {
        if self.selected_plugin().map(|p| p.name.as_str()) != Some(plugin) {
            return;
        }
        self.logs_for = Some(plugin.to_string());
        self.logs = logs.into_iter().rev().take(LOG_CAP).rev().collect();
    }

    pub fn apply_manifest(&mut self, plugin: &str, manifest: Value) {
        if self.selected_plugin().map(|p| p.name.as_str()) != Some(plugin) {
            return;
        }
        let first_load = self.manifest_for.as_deref() != Some(plugin);
        self.manifest_for = Some(plugin.to_string());
        self.manifest = Some(manifest);
        if first_load {
            self.refresh_call_args();
        }
    }

    /// Returns the selected plugin when its manifest still has to be fetched.
    /// Requests are deduplicated; a failed fetch is retried on the next tick.
    pub fn take_manifest_request(&mut self) -> Option<String> {
        let name = self.selected_plugin()?.name.clone();
        if self.manifest_for.as_deref() == Some(name.as_str())
            || self.manifest_requested.as_deref() == Some(name.as_str())
        {
            return None;
        }
        self.manifest_requested = Some(name.clone());
        Some(name)
    }

    pub fn forget_manifest(&mut self) {
        self.manifest = None;
        self.manifest_for = None;
        self.manifest_requested = None;
    }

    pub fn apply_call_result(&mut self, result: Result<Value, String>) {
        self.call.result = Some(match result {
            Ok(value) => Ok(serde_json::to_string_pretty(&value).unwrap_or_default()),
            Err(error) => Err(error),
        });
    }

    pub fn select(&mut self, index: usize) {
        if self.plugins.is_empty() {
            self.selected = 0;
            return;
        }
        let index = index.min(self.plugins.len() - 1);
        if index == self.selected {
            return;
        }
        self.selected = index;
        self.logs.clear();
        self.logs_for = None;
        self.forget_manifest();
        self.call = CallForm::default();
        self.scroll = 0;
    }

    fn step_selection(&mut self, delta: isize) {
        if self.plugins.is_empty() {
            return;
        }
        let len = self.plugins.len() as isize;
        let next = (self.selected as isize + delta).rem_euclid(len);
        self.select(next as usize);
    }

    fn export_names(&self) -> Vec<String> {
        self.selected_plugin()
            .map(|plugin| plugin.exports.clone())
            .unwrap_or_default()
    }

    fn cycle_export(&mut self, delta: isize) {
        let len = self.export_names().len();
        if len == 0 {
            return;
        }
        let next = (self.call.export as isize + delta).rem_euclid(len as isize);
        self.call.export = next as usize;
        self.refresh_call_args();
    }

    fn refresh_call_args(&mut self) {
        let function = self
            .export_names()
            .get(self.call.export)
            .cloned()
            .unwrap_or_default();
        self.call.args = match &self.manifest {
            Some(manifest) => default_args(manifest, &function),
            None => "[]".to_string(),
        };
        self.call.result = None;
    }

    fn submit_call(&mut self) -> Option<Action> {
        let Some(plugin) = self.selected_plugin().map(|p| p.name.clone()) else {
            self.set_status(Level::Error, "no plugin selected");
            return None;
        };
        let function = match self.export_names().get(self.call.export).cloned() {
            Some(function) => function,
            None => {
                self.set_status(Level::Error, "this plugin exports nothing to call");
                return None;
            }
        };
        match serde_json::from_str::<Value>(&self.call.args) {
            Ok(Value::Array(args)) => Some(Action::Call {
                plugin,
                function,
                args,
            }),
            _ => {
                self.set_status(Level::Error, "args must be a JSON array, e.g. [\"hi\", 2]");
                None
            }
        }
    }

    /// Mouse hit-testing for the tab bar, plugin list and footer buttons.
    /// Coordinates are terminal cells; layout constants mirror `view.rs`.
    pub fn handle_mouse(&mut self, mouse: MouseEvent, height: u16) -> Option<Action> {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return None;
        }
        let (col, row) = (mouse.column, mouse.row);

        // Tab bar: detail pane starts at x=40, first row under the header.
        if row == 4 && col >= 40 {
            let mut start = 40u16;
            for tab in Tab::ALL {
                let width = tab.title().len() as u16;
                if col >= start && col < start + width {
                    self.set_tab(tab);
                    return None;
                }
                start += width + 3; // " │ "
            }
            return None;
        }

        // Plugin list: body starts at y=4, list block border occupies y=4.
        if col < 40 && (5..height.saturating_sub(4)).contains(&row) {
            let index = (row - 5) as usize;
            if index < self.plugins.len() {
                self.selected = index;
            }
            return None;
        }

        // Footer buttons: second line inside the footer block.
        if row == height.saturating_sub(2) {
            let x = col.saturating_sub(1) as usize;
            let hit = |needle: &str| {
                KEY_HINTS
                    .find(needle)
                    .is_some_and(|start| x >= start && x < start + needle.len())
            };
            if hit("r reload") {
                return self
                    .selected_plugin()
                    .map(|p| Action::Reload(p.name.clone()));
            }
            if hit("s service") {
                return self.service_toggle();
            }
            if hit("R rebuild") {
                let reload = self
                    .plugins
                    .iter()
                    .filter(|p| !p.is_service())
                    .map(|p| p.name.clone())
                    .collect();
                return Some(Action::Rebuild { reload });
            }
            if hit("o open") {
                return Some(Action::OpenUi);
            }
            if hit("? help") {
                self.help = true;
            }
            if hit("q quit") {
                self.quit = true;
            }
        }
        None
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return None;
        }
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?' | 'q')) {
                self.help = false;
            }
            return None;
        }
        if self.tab == Tab::Call && self.call.editing {
            return self.handle_edit_key(key);
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Esc => self.status = None,
            KeyCode::Tab => self.set_tab(self.tab.step(1)),
            KeyCode::BackTab => self.set_tab(self.tab.step(-1)),
            KeyCode::Char(digit @ '1'..='5') => {
                let index = digit as usize - '1' as usize;
                self.set_tab(Tab::ALL[index]);
            }
            KeyCode::Up | KeyCode::Char('k') => self.step_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.step_selection(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::Left if self.tab == Tab::Call => self.cycle_export(-1),
            KeyCode::Right if self.tab == Tab::Call => self.cycle_export(1),
            KeyCode::Enter if self.tab == Tab::Call => {
                if self.selected_plugin().is_none() {
                    self.set_status(Level::Error, "no plugin selected");
                } else {
                    self.call.editing = true;
                }
            }
            KeyCode::Char('r') => {
                return self
                    .selected_plugin()
                    .map(|p| Action::Reload(p.name.clone()));
            }
            KeyCode::Char('R') => {
                let reload = self
                    .plugins
                    .iter()
                    .filter(|p| !p.is_service())
                    .map(|p| p.name.clone())
                    .collect();
                return Some(Action::Rebuild { reload });
            }
            KeyCode::Char('s') => return self.service_toggle(),
            KeyCode::Char('o') => return Some(Action::OpenUi),
            _ => {}
        }
        None
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.scroll = 0;
    }

    fn service_toggle(&mut self) -> Option<Action> {
        let plugin = self.selected_plugin()?.clone();
        if !plugin.is_service() {
            self.set_status(
                Level::Error,
                format!("{} is not a stateful service", plugin.name),
            );
            return None;
        }
        let running = plugin.service.as_ref().is_some_and(|s| s.running);
        Some(if running {
            Action::ServiceStop(plugin.name)
        } else {
            Action::ServiceStart(plugin.name)
        })
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => self.call.editing = false,
            KeyCode::Enter => return self.submit_call(),
            KeyCode::Backspace => {
                self.call.args.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.call.args.push(c);
            }
            _ => {}
        }
        None
    }
}

/// Builds a best-effort argument list from the WIT parameter types of
/// `function`, so the call form starts with a shape the export accepts.
pub fn default_args(manifest: &Value, function: &str) -> String {
    let params = manifest
        .get("exports")
        .and_then(Value::as_array)
        .and_then(|exports| {
            exports
                .iter()
                .find(|export| export.get("name").and_then(Value::as_str) == Some(function))
        })
        .and_then(|export| export.get("params").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    let args: Vec<Value> = params
        .iter()
        .map(|param| {
            let ty = param
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            default_for_type(ty)
        })
        .collect();
    serde_json::to_string(&args).unwrap_or_else(|_| "[]".to_string())
}

fn default_for_type(ty: &str) -> Value {
    match ty {
        "string" | "char" => Value::String(String::new()),
        "bool" => Value::Bool(false),
        "s8" | "s16" | "s32" | "s64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" => {
            Value::from(0)
        }
        ty if ty.starts_with("list") => Value::Array(Vec::new()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::ServiceRow;
    use super::*;
    use serde_json::json;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn plugin(name: &str, kind: &str, exports: &[&str]) -> PluginRow {
        PluginRow {
            name: name.into(),
            kind: kind.into(),
            runtime: "wasi-component".into(),
            language: "rust".into(),
            exports: exports.iter().map(|e| e.to_string()).collect(),
            served_on: None,
            service: (kind == "service").then(ServiceRow::default),
        }
    }

    fn app_with(plugins: Vec<PluginRow>) -> App {
        let mut app = App::new("demo".into(), "http://127.0.0.1:8787".into(), None, true);
        app.plugins = plugins;
        app
    }

    #[test]
    fn mouse_clicks_tabs_plugins_and_footer_buttons() {
        let mut app = app_with(vec![
            plugin("a", "library", &[]),
            plugin("b", "library", &[]),
        ]);

        // Tab bar starts at x=40; skip "Overview" (8) plus the 3-cell divider.
        app.handle_mouse(click(40 + 8 + 3, 4), 20);
        assert_eq!(app.tab, Tab::Manifest);

        // First plugin row is y=5 (header 4 + list border).
        app.handle_mouse(click(2, 5), 20);
        assert_eq!(app.selected, 0);

        let reload = KEY_HINTS.find("r reload").unwrap() as u16 + 1;
        assert_eq!(
            app.handle_mouse(click(reload, 18), 20),
            Some(Action::Reload("a".into()))
        );

        let rebuild = KEY_HINTS.find("R rebuild").unwrap() as u16 + 1;
        assert_eq!(
            app.handle_mouse(click(rebuild, 18), 20),
            Some(Action::Rebuild {
                reload: vec!["a".into(), "b".into()]
            })
        );
    }

    #[test]
    fn build_failure_persists_until_a_successful_build() {
        let mut app = app_with(vec![]);
        app.apply_done(false, "build failed; see the Output view".into(), true);
        assert_eq!(
            app.build_error.as_deref(),
            Some("build failed; see the Output view")
        );
        app.apply_done(true, "build ok".into(), true);
        assert!(app.build_error.is_none());
    }

    #[test]
    fn non_build_failure_does_not_set_build_error() {
        let mut app = app_with(vec![]);
        app.apply_done(false, "reload failed: notes".into(), false);
        assert!(app.build_error.is_none());
    }

    #[test]
    fn selection_wraps_and_clamps() {
        let mut app = app_with(vec![
            plugin("a", "library", &[]),
            plugin("b", "library", &[]),
        ]);
        assert_eq!(app.handle_key(key(KeyCode::Down)), None);
        assert_eq!(app.selected, 1);
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected, 0);
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn tabs_cycle_and_number_keys_jump() {
        let mut app = app_with(vec![]);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Manifest);
        app.handle_key(key(KeyCode::BackTab));
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.tab, Tab::Output);
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.tab, Tab::Call);
    }

    #[test]
    fn reload_targets_selected_plugin() {
        let mut app = app_with(vec![plugin("notes", "library", &[])]);
        assert_eq!(
            app.handle_key(key(KeyCode::Char('r'))),
            Some(Action::Reload("notes".into()))
        );
    }

    #[test]
    fn rebuild_reloads_only_stateless_plugins() {
        let mut app = app_with(vec![
            plugin("notes", "library", &[]),
            plugin("heartbeat", "service", &[]),
        ]);
        assert_eq!(
            app.handle_key(key(KeyCode::Char('R'))),
            Some(Action::Rebuild {
                reload: vec!["notes".into()]
            })
        );
    }

    #[test]
    fn service_toggle_starts_then_stops() {
        let mut app = app_with(vec![plugin("hb", "service", &[])]);
        assert_eq!(
            app.handle_key(key(KeyCode::Char('s'))),
            Some(Action::ServiceStart("hb".into()))
        );
        app.plugins[0].service = Some(ServiceRow {
            running: true,
            ..Default::default()
        });
        assert_eq!(
            app.handle_key(key(KeyCode::Char('s'))),
            Some(Action::ServiceStop("hb".into()))
        );
    }

    #[test]
    fn service_key_on_library_sets_error_status() {
        let mut app = app_with(vec![plugin("notes", "library", &[])]);
        assert_eq!(app.handle_key(key(KeyCode::Char('s'))), None);
        assert_eq!(app.status.as_ref().map(|s| s.0), Some(Level::Error));
    }

    #[test]
    fn call_form_builds_action_from_json_args() {
        let mut app = app_with(vec![plugin("notes", "library", &["greet"])]);
        app.handle_key(key(KeyCode::Char('3')));
        app.handle_key(key(KeyCode::Enter));
        assert!(app.call.editing);
        app.call.args = "[\"hi\"]".into();
        let action = app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            action,
            Some(Action::Call {
                plugin: "notes".into(),
                function: "greet".into(),
                args: vec![json!("hi")],
            })
        );
    }

    #[test]
    fn manifest_reload_keeps_user_args() {
        let mut app = app_with(vec![plugin("notes", "library", &["greet"])]);
        app.apply_manifest("notes", json!({"exports": []}));
        app.call.args = "[\"Ana\"]".into();
        app.apply_manifest("notes", json!({"exports": []}));
        assert_eq!(app.call.args, "[\"Ana\"]");
    }

    #[test]
    fn call_form_rejects_non_array_args() {
        let mut app = app_with(vec![plugin("notes", "library", &["greet"])]);
        app.tab = Tab::Call;
        app.call.editing = true;
        app.call.args = "{\"x\":1}".into();
        assert_eq!(app.handle_key(key(KeyCode::Enter)), None);
        assert_eq!(app.status.as_ref().map(|s| s.0), Some(Level::Error));
    }

    #[test]
    fn editing_swallows_global_shortcuts() {
        let mut app = app_with(vec![plugin("notes", "library", &["greet"])]);
        app.tab = Tab::Call;
        app.call.editing = true;
        app.call.args.clear();
        assert_eq!(app.handle_key(key(KeyCode::Char('q'))), None);
        assert!(!app.quit);
        assert_eq!(app.call.args, "q");
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.call.editing);
    }

    #[test]
    fn ctrl_c_quits_even_while_editing() {
        let mut app = app_with(vec![]);
        app.tab = Tab::Call;
        app.call.editing = true;
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit);
    }

    #[test]
    fn help_overlay_captures_keys_until_closed() {
        let mut app = app_with(vec![
            plugin("a", "library", &[]),
            plugin("b", "library", &[]),
        ]);
        app.handle_key(key(KeyCode::Char('?')));
        assert!(app.help);
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected, 0);
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.help);
    }

    #[test]
    fn snapshot_keeps_selection_by_name() {
        let mut app = app_with(vec![
            plugin("a", "library", &[]),
            plugin("b", "library", &[]),
        ]);
        app.select(1);
        app.apply_snapshot(Snapshot {
            plugins: vec![plugin("c", "library", &[]), plugin("b", "library", &[])],
            ..Default::default()
        });
        assert_eq!(app.selected_plugin().map(|p| p.name.as_str()), Some("b"));
        assert!(app.host_online);
    }

    #[test]
    fn selection_change_drops_stale_detail() {
        let mut app = app_with(vec![
            plugin("a", "library", &[]),
            plugin("b", "library", &[]),
        ]);
        app.apply_manifest("a", json!({"exports": []}));
        assert_eq!(app.manifest_for.as_deref(), Some("a"));
        app.select(1);
        assert!(app.manifest.is_none());
        assert!(app.logs.is_empty());
    }

    #[test]
    fn manifest_requests_are_deduplicated() {
        let mut app = app_with(vec![plugin("a", "library", &[])]);
        assert_eq!(app.take_manifest_request(), Some("a".to_string()));
        assert_eq!(app.take_manifest_request(), None);
        app.apply_manifest("a", json!({}));
        assert_eq!(app.take_manifest_request(), None);
    }

    #[test]
    fn logs_for_other_plugin_are_ignored() {
        let mut app = app_with(vec![plugin("a", "library", &[])]);
        app.apply_logs(
            "zzz",
            vec![LogLine {
                ts: 1,
                level: "info".into(),
                request_id: 1,
                message: "x".into(),
            }],
        );
        assert!(app.logs.is_empty());
    }

    #[test]
    fn output_buffer_is_capped() {
        let mut app = app_with(vec![]);
        for i in 0..(OUTPUT_CAP + 10) {
            app.push_line(OutputLine {
                source: "host".into(),
                level: Level::Info,
                text: format!("line {i}"),
            });
        }
        assert_eq!(app.output.len(), OUTPUT_CAP);
        assert_eq!(
            app.output.back().unwrap().text,
            format!("line {}", OUTPUT_CAP + 9)
        );
    }

    #[test]
    fn output_keeps_each_line() {
        let mut app = app_with(vec![]);
        for text in "a\nb".lines() {
            app.push_line(OutputLine {
                source: "build".into(),
                level: Level::Error,
                text: text.into(),
            });
        }
        assert_eq!(app.output.len(), 2);
        assert_eq!(app.output[1].text, "b");
    }

    #[test]
    fn default_args_follow_wit_param_types() {
        let manifest = json!({
            "exports": [
                {"name": "score", "params": [
                    {"name": "items", "type": "list<string>"},
                    {"name": "weight", "type": "u32"},
                    {"name": "label", "type": "string"},
                    {"name": "flag", "type": "bool"}
                ]}
            ]
        });
        assert_eq!(default_args(&manifest, "score"), "[[],0,\"\",false]");
        assert_eq!(default_args(&manifest, "missing"), "[]");
    }
}
