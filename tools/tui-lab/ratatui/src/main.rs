use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Tabs, Wrap};
use ratatui::{DefaultTerminal, Frame};

const TABS: [&str; 5] = ["Overview", "Plugins", "Logs", "Metrics", "Tests"];

struct App {
    tab: usize,
    build_error: Option<&'static str>,
    plugins: Vec<(&'static str, &'static str, bool)>,
    logs: Vec<(&'static str, &'static str, &'static str)>,
    tests: Vec<(&'static str, bool, &'static str)>,
}

impl App {
    fn new() -> Self {
        Self {
            tab: 0,
            build_error: Some("plugins/audit-go: componentize-go failed (exit 1) — see Logs"),
            plugins: vec![
                ("core-rust", "library", true),
                ("audit-go", "library", false),
                ("watchtower-rust", "service", true),
                ("board-svelte", "ui", true),
            ],
            logs: vec![
                ("12:04:01", "INFO", "host ready on 127.0.0.1:8787"),
                ("12:04:02", "INFO", "loaded core-rust (wasmtime 50)"),
                (
                    "12:04:03",
                    "WARN",
                    "audit-go: rebuild failed, keeping previous component",
                ),
                (
                    "12:04:03",
                    "ERROR",
                    "build: exit 1; last output: go: no such tool componentize-go",
                ),
                ("12:04:04", "INFO", "service watchtower-rust started"),
            ],
            tests: vec![
                ("smoke:greet", true, "3 ms"),
                ("smoke:sum", true, "2 ms"),
                ("ui:round-trip", false, "timeout 30s"),
            ],
        }
    }
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();
    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('r') => app.build_error = None,
                KeyCode::Char(c @ '1'..='5') => app.tab = c as usize - '1' as usize,
                KeyCode::Left => app.tab = app.tab.saturating_sub(1),
                KeyCode::Right => app.tab = (app.tab + 1).min(TABS.len() - 1),
                _ => {}
            }
        }
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "Owasidea Dev",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  build #128  "),
        Span::styled("● host", Style::default().fg(Color::Green)),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, rows[0]);

    let tabs = Tabs::new(TABS)
        .select(app.tab)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(tabs, rows[1]);

    if let Some(error) = app.build_error {
        let banner = Rect {
            x: rows[2].x,
            y: rows[2].y,
            width: rows[2].width,
            height: 2.min(rows[2].height),
        };
        frame.render_widget(
            Paragraph::new(error).style(Style::default().fg(Color::White).bg(Color::Red)),
            banner,
        );
        let body = Rect {
            y: rows[2].y + banner.height,
            height: rows[2].height - banner.height,
            ..rows[2]
        };
        draw_tab(frame, body, app);
    } else {
        draw_tab(frame, rows[2], app);
    }

    let footer = Paragraph::new("1-5 tabs  ←/→ move  r rebuild  q quit")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, rows[3]);
}

fn draw_tab(frame: &mut Frame, area: Rect, app: &App) {
    match app.tab {
        0 => overview(frame, area, app),
        1 => plugins(frame, area, app),
        2 => logs(frame, area, app),
        3 => metrics(frame, area),
        _ => tests(frame, area, app),
    }
}

fn overview(frame: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let left = Paragraph::new(vec![
        Line::from("App      hello"),
        Line::from("Plugins  4 loaded, 1 failing"),
        Line::from("Services 1 running"),
        Line::from("Build    128 · 8.9s"),
        Line::from(""),
        Line::from(Span::styled("Next steps", Style::default().fg(Color::Cyan))),
        Line::from("  fix audit-go build"),
        Line::from("  press r to retry"),
    ])
    .block(Block::default().borders(Borders::ALL).title("Status"))
    .wrap(Wrap { trim: true });
    frame.render_widget(left, cols[0]);

    let logs = app.logs.iter().map(|(ts, level, msg)| {
        ListItem::new(Line::from(vec![
            Span::styled(*ts, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(*level, level_style(level)),
            Span::raw(" "),
            Span::raw(*msg),
        ]))
    });
    frame.render_widget(
        List::new(logs).block(Block::default().borders(Borders::ALL).title("Latest logs")),
        cols[1],
    );
}

fn plugins(frame: &mut Frame, area: Rect, app: &App) {
    let items = app.plugins.iter().map(|(name, kind, ok)| {
        ListItem::new(Line::from(vec![
            Span::styled(
                if *ok { "● " } else { "✕ " },
                Style::default().fg(if *ok { Color::Green } else { Color::Red }),
            ),
            Span::styled(*name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("  {kind}")),
            Span::styled(
                if *ok { "  loaded" } else { "  error" },
                Style::default().fg(if *ok { Color::Green } else { Color::Red }),
            ),
        ]))
    });
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Plugins")),
        area,
    );
}

fn logs(frame: &mut Frame, area: Rect, app: &App) {
    let lines = app.logs.iter().map(|(ts, level, msg)| {
        Line::from(vec![
            Span::styled(*ts, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(format!("{level:<5}"), level_style(level)),
            Span::raw(" "),
            Span::raw(*msg),
        ])
    });
    frame.render_widget(
        Paragraph::new(lines.collect::<Vec<_>>())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Logs · all plugins"),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn metrics(frame: &mut Frame, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("core-rust p95 latency · 1.1 ms / 20 ms"),
            )
            .gauge_style(Style::default().fg(Color::Green))
            .ratio(0.055),
        rows[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("host memory · 42 MB / 256 MB"),
            )
            .gauge_style(Style::default().fg(Color::Yellow))
            .ratio(0.164),
        rows[1],
    );
    let text = Paragraph::new(vec![
        Line::from("requests  1 284   errors  3   fuel used  12%"),
        Line::from("calls/s   41.2    p50 0.9 ms   p95 1.1 ms"),
        Line::from("top       core-rust 72% · changelog-ts 18% · audit-go 10%"),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Observability"),
    )
    .wrap(Wrap { trim: true });
    frame.render_widget(text, rows[2]);
}

fn tests(frame: &mut Frame, area: Rect, app: &App) {
    let items = app.tests.iter().map(|(name, ok, detail)| {
        ListItem::new(Line::from(vec![
            Span::styled(
                if *ok { "PASS " } else { "FAIL " },
                Style::default()
                    .fg(if *ok { Color::Green } else { Color::Red })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{name:<20} {detail}")),
        ]))
    });
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Tests")),
        area,
    );
}

fn level_style(level: &str) -> Style {
    match level {
        "ERROR" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "WARN" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Blue),
    }
}
