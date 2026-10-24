# ADR 0001 — Project name and UI approach

- Name: Owasidea (CLI binary `owasidea`, crates `owasidea-host`/`owasidea-cli`).
- UI v0: vanilla TypeScript + Vite default, host exposes generic HTTP API.
  Framework-agnostic goal retained: future UIs may compile to WASM (Yew, Blazor, Uno)
  and ship as `js-wasm` webview plugins; host does not impose a framework.
- Config file: `wasm-app.toml`.
- Service plugins in v0: none beyond optional tailcat; lifecycle tested via notes/chat-go.