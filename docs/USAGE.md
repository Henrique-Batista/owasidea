# Usage Guide

For a complete end-to-end walkthrough read [`GET_STARTED.md`](GET_STARTED.md).
For a real six-language app see [`examples/shipyard`](../examples/shipyard).

## Prerequisites

- Rust toolchain + `cargo-component` + `wasm-tools`
- Go 1.27+ + `componentize-go` as a Go tool dependency
- Node 20+ + npm (UI and TypeScript plugins)
- Optional: .NET 10 + wasi workload (C#), wasi-sdk (C/C++), Tauri 2 system deps
- Python 3 (example smoke scripts)

Install Rust WASI targets and component tools:

```bash
rustup target add wasm32-wasip1 wasm32-wasip2
cargo install cargo-component wasm-tools
```

## Configure an app

`wasm-app.toml` at the app root:

```toml
[app]
name = "my-app"
version = "0.1.0"
identifier = "dev.example.myapp"

[host]
addr = "127.0.0.1:8787"

[plugins.my-core]
path = "plugins/my-core"
language = "rust"
type = "library"
kv = "my-core:read-write"

[desktop]
path = "desktop"
ui = "ui"

[dev]
watch = true
hot_reload = true
```

Relative paths resolve against the config file's directory, so the app can be
built from anywhere:

```bash
owasidea --config examples/shipyard/wasm-app.toml build
```

## Build

```bash
owasidea build
owasidea validate
```

`build` compiles plugins, installs missing npm dependencies, auto-detects
wasi-sdk for C/C++, generates manifests from WIT, runs composition, signs when
`[signing]` is configured, and packages UI-plugin assets. `validate` checks
checksums and permission drift.
Generated manifests and `.wasm` files should not be committed.

## Run

```bash
owasidea run       # host only, current build
owasidea dev       # build + host + UI + watcher
owasidea test      # build + API smoke + optional UI tests
owasidea smoke     # build + API smoke only
```

`dev` prints the UI URL, host URL, loaded plugins, and watch roots. Stateless
components hot-reload. Stateful services must be stopped before reloading.

## Run the UI separately

```bash
cd ui && npm install && npm run dev
```

Set `OWASIDEA_HOST` if the host is not at the default address, and include the
Vite origin in `OWASIDEA_ALLOWED_ORIGINS` on the host when running outside
`owasidea dev`.

## Build the desktop bundle

```bash
owasidea build
owasidea bundle
```

`bundle` builds the UI and runs `cargo tauri build` in the desktop crate selected
by `[desktop].path`. The Tauri config must include manifests and component files
in `bundle.resources`; see `examples/shipyard/desktop/tauri.conf.json`.

At runtime the shell should resolve `$RESOURCE/manifests` and set
`OWASIDEA_DATA_DIR` to the platform app-data directory. The reference shell does
both.

## Install the CLI

```bash
cargo install --path crates/cli
owasidea --help
```

Or use the checkout without installing:

```bash
cargo run -p owasidea-cli -- --help
```

## Reference commands

```bash
cd examples/shipyard
owasidea doctor --install-wasi-sdk
owasidea build
owasidea smoke
owasidea test
owasidea dev
owasidea bundle
```
