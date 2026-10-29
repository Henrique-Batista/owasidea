//! Rendering for the dashboard. Everything is derived from `App`; the view
//! holds no state of its own.

use super::api::PluginRow;
use super::state::{App, Level, Tab, KEY_HINTS};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Tabs, Wrap};
use ratatui::Frame;

const ACCENT: Color = Color::Cyan;
const OK: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;
const DIM: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(5),
        Constraint::Length(4),
    ])
    .areas(frame.area());
    draw_header(frame, header, app);
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Length(40), Constraint::Min(20)]).areas(body);
    draw_plugins(frame, list_area, app);
    draw_detail(frame, detail_area, app);
    draw_footer(frame, footer, app);
    if app.help {
        draw_help(frame, frame.area());
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = format!(" owasidea · {} ", app.app_name);
    let block = Block::bordered()
        .title(title)
        .border_style(Style::default().fg(ACCENT));
    let (dot, dot_color) = if app.host_online {
        ("●", OK)
    } else {
        ("●", BAD)
    };
    let ui = app.ui_url.as_deref().unwrap_or("-");
    let line1 = Line::from(vec![
        Span::styled(dot, Style::default().fg(dot_color)),
        Span::raw(format!(" host {}", app.host_url)),
        Span::styled("   ui ", Style::default().fg(DIM)),
        Span::raw(ui.to_string()),
    ]);
    let flag = |on: bool| {
        if on {
            Span::styled("on", Style::default().fg(OK))
        } else {
            Span::styled("off", Style::default().fg(DIM))
        }
    };
    let mut line2 = vec![
        Span::styled("build ", Style::default().fg(DIM)),
        Span::raw(app.build_id.clone().unwrap_or_else(|| "-".into())),
        Span::styled("   plugins ", Style::default().fg(DIM)),
        Span::raw(app.plugins.len().to_string()),
    ];
    if app.dev {
        line2.push(Span::styled("   watch ", Style::default().fg(DIM)));
        line2.push(flag(app.watch));
        line2.push(Span::styled("   hot-reload ", Style::default().fg(DIM)));
        line2.push(flag(app.hot_reload));
    }
    if !app.host_online {
        line2.push(Span::styled(
            "   host offline — retrying",
            Style::default().fg(BAD),
        ));
    }
    let text = Text::from(vec![line1, Line::from(line2)]);
    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn plugin_item(plugin: &PluginRow, selected: bool) -> ListItem<'static> {
    let (state, color) = match &plugin.service {
        Some(service) if service.running => ("running", OK),
        Some(service) if service.last_error.is_some() => ("error", BAD),
        Some(_) => ("stopped", WARN),
        None => ("loaded", OK),
    };
    let marker = if selected { "▶ " } else { "  " };
    let name_style = if selected {
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
    };
    let title = Line::from(vec![
        Span::raw(marker),
        Span::styled(plugin.name.clone(), name_style),
        Span::raw(" "),
        Span::styled("●", Style::default().fg(color)),
        Span::styled(format!(" {state}"), Style::default().fg(color)),
    ]);
    let meta = Line::from(Span::styled(
        format!(
            "    {} · {} · {}",
            plugin.kind, plugin.language, plugin.runtime
        ),
        Style::default().fg(DIM),
    ));
    ListItem::new(Text::from(vec![title, meta]))
}

fn draw_plugins(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered().title(" Plugins ");
    if app.plugins.is_empty() {
        let message = if app.host_online {
            "no plugins loaded"
        } else {
            "waiting for host…"
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .plugins
        .iter()
        .enumerate()
        .map(|(index, plugin)| plugin_item(plugin, index == app.selected))
        .collect();
    frame.render_widget(List::new(items).block(block), area);
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let [tabs_area, content] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
    let titles: Vec<&str> = Tab::ALL.iter().map(|tab| tab.title()).collect();
    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .divider(" │ ");
    frame.render_widget(tabs, tabs_area);

    let block = Block::bordered();
    match app.tab {
        Tab::Overview => draw_overview(frame, content, app, block),
        Tab::Manifest => draw_manifest(frame, content, app, block),
        Tab::Call => draw_call(frame, content, app, block),
        Tab::Logs => {
            let lines: Vec<Line> = app
                .logs
                .iter()
                .map(|entry| {
                    let color = level_color(&entry.level);
                    Line::from(vec![
                        Span::styled(format!("{:<5} ", entry.level), Style::default().fg(color)),
                        Span::styled(format!("#{} ", entry.request_id), Style::default().fg(DIM)),
                        Span::raw(entry.message.clone()),
                    ])
                })
                .collect();
            let empty = if app.selected_plugin().is_none() {
                "no plugin selected"
            } else {
                "no log entries yet"
            };
            draw_tail(frame, content, block, lines, empty);
        }
        Tab::Output => {
            let lines: Vec<Line> = app
                .output
                .iter()
                .map(|line| {
                    let color = match line.level {
                        Level::Error => BAD,
                        Level::Info => source_color(&line.source),
                    };
                    Line::from(vec![
                        Span::styled(format!("[{}] ", line.source), Style::default().fg(color)),
                        Span::raw(line.text.clone()),
                    ])
                })
                .collect();
            draw_tail(frame, content, block, lines, "no output yet");
        }
    }
}

fn draw_tail(frame: &mut Frame, area: Rect, block: Block, lines: Vec<Line>, empty: &'static str) {
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(empty)
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }
    let visible = area.height.saturating_sub(2) as usize;
    let offset = lines.len().saturating_sub(visible);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((offset as u16, 0)),
        area,
    );
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App, block: Block) {
    let mut lines: Vec<Line> = Vec::new();
    match app.selected_plugin() {
        Some(plugin) => {
            lines.push(kv("name", &plugin.name));
            lines.push(kv("type", &plugin.kind));
            lines.push(kv("language", &plugin.language));
            lines.push(kv("runtime", &plugin.runtime));
            if let Some(served) = &plugin.served_on {
                lines.push(kv("served on", served));
            }
            if let Some(service) = &plugin.service {
                lines.push(kv(
                    "service",
                    if service.running {
                        "running"
                    } else {
                        "stopped"
                    },
                ));
                if let Some(address) = &service.address {
                    lines.push(kv("address", address));
                }
                lines.push(kv("restarts", &service.restarts.to_string()));
                if let Some(error) = &service.last_error {
                    lines.push(Line::from(Span::styled(
                        format!("last error: {error}"),
                        Style::default().fg(BAD),
                    )));
                }
            }
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "exports",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            if plugin.exports.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (none)",
                    Style::default().fg(DIM),
                )));
            }
            for export in &plugin.exports {
                lines.push(Line::from(format!("  • {export}")));
            }
        }
        None => lines.push(Line::from("No plugin selected.")),
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "last reloads",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if app.last_reload.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none yet)",
            Style::default().fg(DIM),
        )));
    }
    for record in app.last_reload.iter().take(8) {
        let (label, color) = if record.ok { ("ok ", OK) } else { ("err", BAD) };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(label, Style::default().fg(color)),
            Span::raw(format!(" {} (HTTP {})", record.plugin, record.http_status)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        area,
    );
}

fn draw_manifest(frame: &mut Frame, area: Rect, app: &App, block: Block) {
    let text = match (&app.manifest, app.selected_plugin()) {
        (Some(manifest), _) => {
            serde_json::to_string_pretty(manifest).unwrap_or_else(|_| "invalid manifest".into())
        }
        (None, Some(plugin)) if app.host_online => format!("loading manifest for {}…", plugin.name),
        (None, _) => "no manifest available".to_string(),
    };
    frame.render_widget(
        Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        area,
    );
}

fn draw_call(frame: &mut Frame, area: Rect, app: &App, block: Block) {
    let mut lines: Vec<Line> = Vec::new();
    let plugin = app.selected_plugin();
    let exports = plugin.map(|p| p.exports.clone()).unwrap_or_default();
    lines.push(Line::from(Span::styled(
        "exports  (←/→ choose, Enter edit args, Enter run, Esc stop)",
        Style::default().fg(DIM),
    )));
    if exports.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(DIM),
        )));
    }
    for (index, export) in exports.iter().enumerate() {
        let selected = index == app.call.export;
        let style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{} {export}", if selected { "▶" } else { " " }),
            style,
        )));
    }
    lines.push(Line::default());
    let cursor = if app.call.editing { "▏" } else { "" };
    let args_style = if app.call.editing {
        Style::default().fg(WARN)
    } else {
        Style::default()
    };
    lines.push(Line::from(vec![
        Span::styled("args  ", Style::default().fg(DIM)),
        Span::styled(format!("{}{cursor}", app.call.args), args_style),
    ]));
    lines.push(Line::default());
    match &app.call.result {
        Some(Ok(output)) => {
            lines.push(Line::from(Span::styled("result", Style::default().fg(OK))));
            for line in output.lines() {
                lines.push(Line::from(format!("  {line}")));
            }
        }
        Some(Err(error)) => {
            lines.push(Line::from(Span::styled("error", Style::default().fg(BAD))));
            lines.push(Line::from(format!("  {error}")));
        }
        None => lines.push(Line::from(Span::styled(
            "no result yet",
            Style::default().fg(DIM),
        ))),
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered().border_style(if app.build_error.is_some() {
        Style::default().fg(BAD)
    } else {
        Style::default().fg(DIM)
    });
    let status = if let Some(error) = &app.build_error {
        Line::from(Span::styled(
            format!("BUILD FAILED  {error}"),
            Style::default().fg(BAD).add_modifier(Modifier::BOLD),
        ))
    } else {
        match (&app.busy, &app.status) {
            (Some(busy), _) => {
                Line::from(Span::styled(format!("⟳ {busy}"), Style::default().fg(WARN)))
            }
            (None, Some((level, text))) => Line::from(Span::styled(
                text.clone(),
                Style::default().fg(match level {
                    Level::Info => OK,
                    Level::Error => BAD,
                }),
            )),
            (None, None) => Line::from(Span::styled(
                if app.tab == Tab::Call && app.call.editing {
                    "editing args — Enter runs the export, Esc stops editing"
                } else {
                    "ready"
                },
                Style::default().fg(DIM),
            )),
        }
    };
    let keys = if app.tab == Tab::Call && app.call.editing {
        ""
    } else {
        KEY_HINTS
    };
    let text = Text::from(vec![
        status,
        Line::from(Span::styled(keys, Style::default().fg(DIM))),
    ]);
    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let rows = [
        ("↑ / ↓  or  j / k", "select plugin"),
        ("Tab / Shift+Tab  or  1-5", "switch view"),
        ("PgUp / PgDn", "scroll manifest and overview"),
        ("r", "reload selected plugin"),
        ("s", "start / stop selected service"),
        ("R", "rebuild app, then reload stateless plugins"),
        ("o", "open UI (or host) in the browser"),
        ("Call view: ← / →", "choose export"),
        ("Call view: Enter", "edit args; Enter runs; Esc stops"),
        (
            "? / click",
            "toggle this help; mouse clicks tabs, plugins and footer buttons",
        ),
        ("q / Ctrl+C", "quit (stops dev children)"),
    ];
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::styled(format!("{keys:<30}"), Style::default().fg(ACCENT)),
                Span::raw(*what),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "press ? or Esc to close",
        Style::default().fg(DIM),
    )));
    let width = 70.min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(" Keys ")
                .border_style(Style::default().fg(ACCENT)),
        ),
        popup,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<11}"), Style::default().fg(DIM)),
        Span::raw(value.to_string()),
    ])
}

fn level_color(level: &str) -> Color {
    match level.to_ascii_lowercase().as_str() {
        "error" => BAD,
        "warn" | "warning" => WARN,
        "debug" => DIM,
        _ => OK,
    }
}

fn source_color(source: &str) -> Color {
    match source {
        "host" => ACCENT,
        "ui" => Color::Magenta,
        "build" => WARN,
        "reload" => OK,
        "watch" | "dev" => Color::Blue,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::{PluginRow, ReloadRecord, ServiceRow, Snapshot};
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn sample_app() -> App {
        let mut app = App::new(
            "shipyard".into(),
            "http://127.0.0.1:8787".into(),
            Some("http://127.0.0.1:5173".into()),
            true,
        );
        app.apply_snapshot(Snapshot {
            build_id: Some("abc123".into()),
            watch: true,
            hot_reload: true,
            plugins: vec![
                PluginRow {
                    name: "core-rust".into(),
                    kind: "library".into(),
                    runtime: "wasi-component".into(),
                    language: "rust".into(),
                    exports: vec!["ping".into(), "score".into()],
                    served_on: None,
                    service: None,
                },
                PluginRow {
                    name: "watchtower-rust".into(),
                    kind: "service".into(),
                    runtime: "wasi-component".into(),
                    language: "rust".into(),
                    exports: vec![],
                    served_on: None,
                    service: Some(ServiceRow {
                        running: true,
                        address: Some("127.0.0.1:9000".into()),
                        ..Default::default()
                    }),
                },
            ],
            last_reload: vec![ReloadRecord {
                plugin: "core-rust".into(),
                ok: true,
                http_status: 200,
            }],
        });
        app
    }

    #[test]
    fn overview_shows_plugins_and_urls() {
        let app = sample_app();
        let screen = render(&app, 110, 28);
        assert!(screen.contains("owasidea · shipyard"));
        assert!(screen.contains("http://127.0.0.1:8787"));
        assert!(screen.contains("http://127.0.0.1:5173"));
        assert!(screen.contains("core-rust"));
        assert!(screen.contains("watchtower-rust"));
        assert!(screen.contains("running"));
        assert!(screen.contains("watch"));
        assert!(screen.contains("ok  core-rust (HTTP 200)"));
    }

    #[test]
    fn footer_lists_shortcuts() {
        let screen = render(&sample_app(), 110, 28);
        assert!(screen.contains("r reload"));
        assert!(screen.contains("q quit"));
    }

    #[test]
    fn offline_host_is_visible() {
        let app = App::new("demo".into(), "http://127.0.0.1:1".into(), None, false);
        let screen = render(&app, 100, 20);
        assert!(screen.contains("waiting for host"));
        assert!(screen.contains("host offline"));
    }

    #[test]
    fn call_view_renders_exports_and_args() {
        let mut app = sample_app();
        app.tab = Tab::Call;
        app.call.editing = true;
        app.call.args = "[90]".into();
        let screen = render(&app, 110, 28);
        assert!(screen.contains("▶ ping"));
        assert!(screen.contains("[90]"));
    }

    #[test]
    fn help_overlay_renders() {
        let mut app = sample_app();
        app.help = true;
        let screen = render(&app, 110, 40);
        assert!(screen.contains("Keys"));
        assert!(screen.contains("rebuild app"));
    }

    #[test]
    fn logs_tail_shows_latest_entries() {
        let mut app = sample_app();
        app.tab = Tab::Logs;
        app.logs = (0..40)
            .map(|i| super::super::api::LogLine {
                ts: i,
                level: "info".into(),
                request_id: i,
                message: format!("entry-{i}"),
            })
            .collect();
        let screen = render(&app, 110, 16);
        assert!(screen.contains("entry-39"));
        assert!(!screen.contains("entry-0 "));
    }
}
