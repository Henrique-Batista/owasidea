use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, SharedString, Window,
    WindowBounds, WindowOptions,
};
use std::io::{Read, Write};
use std::net::TcpStream;

struct CounterWindow {
    heading: SharedString,
    result: SharedString,
}

/// Minimal HTTP/1.1 POST so the example needs no extra dependency. The host
/// API is loopback-only and short-lived, so a single request per window is
/// enough.
fn call(hook: &str, plugin: &str, function: &str, args: &str) -> String {
    let host = hook.trim_start_matches("http://").trim_end_matches('/');
    let (address, port) = host
        .rsplit_once(':')
        .map(|(address, port)| (address, port.parse::<u16>().unwrap_or(8787)))
        .unwrap_or((host, 8787));
    let body = format!(r#"{{"function":"{function}","args":{args}}}"#);
    let request = format!(
        "POST /plugin/{plugin}/call HTTP/1.1\r\nHost: {address}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let Ok(mut stream) = TcpStream::connect((address, port)) else {
        return "host unreachable".to_string();
    };
    if stream.write_all(request.as_bytes()).is_err() {
        return "host write failed".to_string();
    }
    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return "host read failed".to_string();
    }
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .unwrap_or_else(|| "empty host response".to_string())
}

impl Render for CounterWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .bg(rgb(0x1e1e2e))
            .size(px(640.0))
            .justify_center()
            .items_center()
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(self.heading.clone())
            .child(
                div()
                    .text_2xl()
                    .text_color(rgb(0x89dceb))
                    .child(self.result.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xa0a0b0))
                    .child("counter.increment(1) through OWASIDEA_HOST"),
            )
    }
}

fn main() {
    let host = std::env::var("OWASIDEA_HOST").unwrap_or_else(|_| "http://127.0.0.1:8810".into());
    let result = call(&host, "counter", "increment", "[1]");
    let heading: SharedString = format!("Counter via {host}").into();
    let result: SharedString = format!("increment(1) -> {result}").into();
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(640.0), px(360.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| CounterWindow {
                    heading: heading.clone(),
                    result: result.clone(),
                })
            },
        )
        .expect("failed to open the example window");
        cx.activate(true);
    });
}
