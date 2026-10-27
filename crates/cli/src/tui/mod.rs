//! Interactive terminal dashboard shared by `owasidea dev` (default in a TTY)
//! and `owasidea tui` (attaches to a running host).
//!
//! The dashboard is a pure client of the host HTTP API. Background work
//! (polling, reloads, builds, calls) runs on tokio tasks or child processes and
//! reports back through one event channel, so the render loop never blocks.

mod api;
mod state;
mod view;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self as ct, KeyEventKind};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub use state::{Action, App, Level, OutputLine};

pub enum Event {
    Key(ratatui::crossterm::event::KeyEvent),
    Tick,
    Snapshot(Result<api::Snapshot, String>),
    Logs {
        plugin: String,
        result: Result<Vec<api::LogLine>, String>,
    },
    Manifest {
        plugin: String,
        result: Result<Value, String>,
    },
    Output(OutputLine),
    Done {
        ok: bool,
        text: String,
        /// Set when the app changed on disk (build/reload), so the manifest is fetched again.
        refresh_manifest: bool,
    },
    CallResult(Result<Value, String>),
}

pub type Sender = UnboundedSender<Event>;
pub type Receiver = UnboundedReceiver<Event>;

pub fn channel() -> (Sender, Receiver) {
    mpsc::unbounded_channel()
}

/// Where developer messages go. Plain `owasidea dev` prints them; the TUI
/// routes them into its Output view so they never corrupt the screen.
#[derive(Clone)]
pub enum Sink {
    Stdout,
    Tui(Sender),
}

impl Sink {
    pub fn info(&self, source: &str, text: impl Into<String>) {
        let text = text.into();
        match self {
            Sink::Stdout => println!("[{source}] {text}"),
            Sink::Tui(tx) => {
                let _ = tx.send(Event::Output(OutputLine {
                    source: source.to_string(),
                    level: Level::Info,
                    text,
                }));
            }
        }
    }

    pub fn error(&self, source: &str, text: impl Into<String>) {
        let text = text.into();
        match self {
            Sink::Stdout => eprintln!("[{source}] {text}"),
            Sink::Tui(tx) => {
                let _ = tx.send(Event::Output(OutputLine {
                    source: source.to_string(),
                    level: Level::Error,
                    text,
                }));
            }
        }
    }
}

/// Forwards a child's stdout and stderr into the event channel, line by line.
pub fn pipe_output(mut child: Child, source: &'static str, tx: Sender) -> Child {
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx.send(Event::Output(OutputLine {
                    source: source.to_string(),
                    level: Level::Info,
                    text: strip_ansi(&line),
                }));
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx.send(Event::Output(OutputLine {
                    source: source.to_string(),
                    level: Level::Error,
                    text: strip_ansi(&line),
                }));
            }
        });
    }
    child
}

/// Removes CSI escape sequences (colors, cursor moves) so child output
/// cannot disturb the dashboard layout.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    out
}

pub struct Session {
    pub app_name: String,
    pub host_url: String,
    pub ui_url: Option<String>,
    pub dev: bool,
    pub config_path: PathBuf,
    pub manifest_dir: PathBuf,
    /// Processes owned by the dashboard (host, Vite). Killed on exit.
    pub children: Vec<Child>,
}

/// Runs the dashboard until the user quits. Takes ownership of the terminal.
pub async fn run(session: Session, tx: Sender, rx: Receiver) -> Result<()> {
    let client = api::Client::new(&session.host_url)?;
    let mut app = App::new(
        session.app_name.clone(),
        session.host_url.clone(),
        session.ui_url.clone(),
        session.dev,
    );
    let mut terminal = ratatui::init();
    // Clear without a cursor-position query: stale shell output would
    // otherwise survive in cells the first frame does not repaint.
    ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All)
    )
    .context("failed to clear terminal")?;
    let stop_input = Arc::new(AtomicBool::new(false));
    let input = spawn_input_thread(tx.clone(), stop_input.clone());
    let ticker = spawn_ticker(tx.clone());
    let runner = Runner {
        client,
        tx: tx.clone(),
        host_url: session.host_url.clone(),
        ui_url: session.ui_url.clone(),
        config_path: session.config_path.clone(),
        manifest_dir: session.manifest_dir.clone(),
    };

    let result = event_loop(&mut terminal, &mut app, rx, &runner).await;

    stop_input.store(true, Ordering::SeqCst);
    ticker.abort();
    let _ = input.join();
    ratatui::restore();

    let mut children = session.children;
    for child in children.iter_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    mut rx: Receiver,
    runner: &Runner,
) -> Result<()> {
    runner.tick(app);
    loop {
        terminal
            .draw(|frame| view::draw(frame, app))
            .context("failed to draw dashboard")?;
        let Some(event) = rx.recv().await else {
            break;
        };
        match event {
            Event::Key(key) => {
                let before = app.selected;
                if let Some(action) = app.handle_key(key) {
                    runner.dispatch(app, action);
                }
                if app.selected != before {
                    runner.fetch_logs(app);
                }
            }
            Event::Tick => runner.tick(app),
            Event::Snapshot(Ok(snapshot)) => app.apply_snapshot(snapshot),
            Event::Snapshot(Err(_)) => app.apply_offline(),
            Event::Logs { plugin, result } => {
                if let Ok(logs) = result {
                    app.apply_logs(&plugin, logs);
                }
            }
            Event::Manifest { plugin, result } => match result {
                Ok(manifest) => app.apply_manifest(&plugin, manifest),
                Err(error) => app.set_status(Level::Error, format!("manifest {plugin}: {error}")),
            },
            Event::Output(line) => app.push_line(line),
            Event::Done {
                ok,
                text,
                refresh_manifest,
            } => {
                app.busy = None;
                app.set_status(if ok { Level::Info } else { Level::Error }, text);
                if refresh_manifest {
                    app.forget_manifest();
                }
            }
            Event::CallResult(result) => app.apply_call_result(result),
        }
        if app.quit {
            break;
        }
        if let Some(name) = app.take_manifest_request() {
            runner.fetch_manifest(name);
        }
    }
    Ok(())
}

fn spawn_input_thread(tx: Sender, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match ct::poll(Duration::from_millis(100)) {
                Ok(true) => match ct::read() {
                    Ok(ct::Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        if tx.send(Event::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    })
}

fn spawn_ticker(tx: Sender) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tx.send(Event::Tick).is_err() {
                break;
            }
        }
    })
}

struct Runner {
    client: api::Client,
    tx: Sender,
    host_url: String,
    ui_url: Option<String>,
    config_path: PathBuf,
    manifest_dir: PathBuf,
}

impl Runner {
    /// Polls the host and the selected plugin's logs. Results come back as
    /// events, so a slow host never stalls key handling.
    fn tick(&self, app: &App) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.snapshot().await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Snapshot(result));
        });
        self.fetch_logs(app);
    }

    fn fetch_logs(&self, app: &App) {
        let Some(plugin) = app.selected_plugin().map(|p| p.name.clone()) else {
            return;
        };
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.logs(&plugin).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Logs { plugin, result });
        });
    }

    fn fetch_manifest(&self, plugin: String) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.manifest(&plugin).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Manifest { plugin, result });
        });
    }

    fn dispatch(&self, app: &mut App, action: Action) {
        match action {
            Action::Reload(name) => {
                app.busy = Some(format!("reloading {name}"));
                let client = self.client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let (ok, text) = match client.reload(&name).await {
                        Ok(_) => (true, format!("reloaded {name}")),
                        Err(error) => (false, format!("reload {name} failed: {error:#}")),
                    };
                    let _ = tx.send(Event::Done {
                        ok,
                        text,
                        refresh_manifest: true,
                    });
                });
            }
            Action::ServiceStart(name) => self.service_action(app, name, true),
            Action::ServiceStop(name) => self.service_action(app, name, false),
            Action::Rebuild { reload } => self.rebuild(app, reload),
            Action::Call {
                plugin,
                function,
                args,
            } => {
                app.busy = Some(format!("calling {plugin}.{function}"));
                let client = self.client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .call(&plugin, &function, args)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::CallResult(result));
                    let _ = tx.send(Event::Done {
                        ok: true,
                        text: format!("called {plugin}.{function}"),
                        refresh_manifest: false,
                    });
                });
            }
            Action::OpenUi => {
                let url = self.ui_url.clone().unwrap_or_else(|| self.host_url.clone());
                match open_url(&url) {
                    Ok(()) => app.set_status(Level::Info, format!("opened {url}")),
                    Err(error) => {
                        app.set_status(Level::Error, format!("could not open {url}: {error}"))
                    }
                }
            }
        }
    }

    fn service_action(&self, app: &mut App, name: String, start: bool) {
        app.busy = Some(format!(
            "{} {name}",
            if start { "starting" } else { "stopping" }
        ));
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = if start {
                client.service_start(&name).await
            } else {
                client.service_stop(&name).await
            };
            let (ok, text) = match result {
                Ok(output) => (
                    true,
                    format!(
                        "{} {name}{}",
                        if start { "started" } else { "stopped" },
                        if output.is_empty() {
                            String::new()
                        } else {
                            format!(": {output}")
                        }
                    ),
                ),
                Err(error) => (false, format!("{name}: {error:#}")),
            };
            let _ = tx.send(Event::Done {
                ok,
                text,
                refresh_manifest: false,
            });
        });
    }

    /// Runs `owasidea build` as a child, streams its output, then reloads the
    /// stateless plugins so the host picks up the new component bytes.
    fn rebuild(&self, app: &mut App, reload: Vec<String>) {
        app.busy = Some("building app".to_string());
        let tx = self.tx.clone();
        let client = self.client.clone();
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                app.busy = None;
                app.set_status(Level::Error, format!("cannot locate owasidea: {error}"));
                return;
            }
        };
        let spawned = Command::new(exe)
            .arg("--config")
            .arg(&self.config_path)
            .arg("--manifest-dir")
            .arg(&self.manifest_dir)
            .arg("build")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                app.busy = None;
                app.set_status(Level::Error, format!("build did not start: {error}"));
                return;
            }
        };
        let mut child = pipe_output(child, "build", tx.clone());
        tokio::spawn(async move {
            let status = tokio::task::spawn_blocking(move || child.wait()).await;
            if !matches!(&status, Ok(Ok(s)) if s.success()) {
                let _ = tx.send(Event::Done {
                    ok: false,
                    text: "build failed; see the Output view".to_string(),
                    refresh_manifest: true,
                });
                return;
            }
            let mut failed = Vec::new();
            for name in &reload {
                if client.reload(name).await.is_err() {
                    failed.push(name.clone());
                }
            }
            let (ok, text) = if failed.is_empty() {
                (
                    true,
                    format!("build ok; reloaded {} plugin(s)", reload.len()),
                )
            } else {
                (
                    false,
                    format!("build ok; reload failed: {}", failed.join(", ")),
                )
            };
            let _ = tx.send(Event::Done {
                ok,
                text,
                refresh_manifest: true,
            });
        });
    }
}

fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// `owasidea tui` refuses to run without a terminal: a dashboard that cannot
/// draw would otherwise hang or spew escape codes into a log file.
pub fn require_terminal() -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "the TUI needs an interactive terminal; use `owasidea status --watch` or `--json` in scripts and CI"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_sink_routes_messages_to_output() {
        let (tx, mut rx) = channel();
        let sink = Sink::Tui(tx);
        sink.info("dev", "changed plugins/a");
        sink.error("dev", "reload failed");
        match rx.try_recv().unwrap() {
            Event::Output(line) => {
                assert_eq!(line.source, "dev");
                assert_eq!(line.level, Level::Info);
                assert_eq!(line.text, "changed plugins/a");
            }
            _ => panic!("expected output"),
        }
        match rx.try_recv().unwrap() {
            Event::Output(line) => assert_eq!(line.level, Level::Error),
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn strip_ansi_keeps_text_only() {
        assert_eq!(
            strip_ansi("\u{1b}[2m2026-10-27\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m host"),
            "2026-10-27  INFO host"
        );
        assert_eq!(strip_ansi("plain\rtext"), "plaintext");
    }

    #[test]
    fn require_terminal_rejects_piped_stdio() {
        // `cargo test` runs without a TTY on stdin, which is the case we guard.
        if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            assert!(require_terminal().is_err());
        }
    }
}
