# Architecture

## Components

- `crates/host` — reusable Rust library: manifest loading, capability broker, Wasmtime runtime, HTTP API. Independent of Tauri.
- `crates/cli` — `owasidea` binary (clap), reuses host.
- `crates/desktop` — Tauri 2 shell (planned, embeds host).
- `ui` — frontend (vanilla TS + Vite), talks to host HTTP API only.
- `plugins/*` — example plugins (Rust/Go WASI components).
- `manifests/*.manifest.json` — generated, never hand-edit.
- `scripts/gen-manifest.py` — derives manifest from `wasm-tools component wit --json`.

## Runtime

- Wasmtime 50, Component Model, WASI 0.2 + 0.3 p3.
- Plugins are WASI components described by WIT.
- `runtime`: `wasi-component` (loaded by Wasmtime) or `js-wasm` (webview only).
- `type`: `library`, `executable`, `hybrid`, (planned `service`).

## Capability enforcement

Host exposes only host interfaces declared in manifest (`poc:host/host-kv`, `poc:host/host-log`). Tampered manifest fails instantiation. WASI preopens driven by manifest filesystem permissions. Network only when `wasi:sockets` imported.

## Data flow

UI/CLI → HTTP → host → instantiate component → typed call → response. Logs via per-plugin ring buffer. Executable plugins served by `wasmtime serve` sidecar and reverse-proxied.