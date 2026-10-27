# {{name}} UI plugin

Language/framework: {{lang}}
Runtime: js-wasm (webview-only)

Build a static browser bundle into `dist/` with an `index.html` entry, then configure this plugin in `wasm-app.toml` with `type = \"ui\"`, `runtime = \"js-wasm\"`, and an optional `ui_build = [...]` command. The framework loads it in a sandboxed iframe.
