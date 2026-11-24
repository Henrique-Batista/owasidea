use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, SharedString, Window,
    WindowBounds, WindowOptions,
};

struct NativeUi {
    title: SharedString,
}

impl Render for NativeUi {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0x1e1e2e))
            .size(px(640.0))
            .justify_center()
            .items_center()
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(self.title.clone())
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xa0a0b0))
                    .child("Call the host API to load plugin data"),
            )
    }
}

fn main() {
    let host = std::env::var("OWASIDEA_HOST").unwrap_or_else(|_| "http://127.0.0.1:8787".into());
    let plugin = std::env::var("OWASIDEA_PLUGIN").unwrap_or_else(|_| "{{name}}".into());
    let title: SharedString = format!("{plugin} — {host}").into();
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(640.0), px(360.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| NativeUi {
                    title: title.clone(),
                })
            },
        )
        .expect("failed to open the native UI window");
        cx.activate(true);
    });
}
