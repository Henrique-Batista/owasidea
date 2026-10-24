# Usage Guide

## Prerequisites

- Rust toolchain + `cargo-component` + `wasm-tools`
- Go 1.27+ (for Go plugins)
- Node 20+ / pnpm (for UI)
- `wasmtime` CLI (for executable plugins)
- Tauri system deps (optional, desktop)

Install Rust WASI targets:

```bash
rustup target add wasm32-wasip1 wasm32-wasip2
cargo install cargo-component wasm-tools
```

## Configure

`wasm-app.toml` at project root:

```toml
[app]
name = "my-app"
version = "0.1.0"
identifier = "dev.example.myapp"

[host]
addr = "127.0.0.1:8787"

[plugins.notes]
path = "plugins/notes"
language = "rust"
type = "library"

[dev]
watch = true
hot_reload = true
```

## Build

```bash
./scripts/build-all.sh
```

Builds Rust/Go plugins, copies `.wasm`, regenerates `manifests/*.manifest.json` from WIT.

## Run host

```bash
./scripts/run-host.sh
# http://127.0.0.1:8787
```

Or via CLI:

```bash
owasidea run
owasidea dev   # host + (UI dev server separately)
```

## Run UI

```bash
cd ui && npm install && npm run dev
# http://localhost:5173
```

## Installing the CLI

From a local checkout:

```bash
cargo install --path crates/cli
# now available as `owasidea`
owasidea --help
```

From a git repository:

```bash
cargo install --git https://github.com/owasidea/owasidea owasidea-cli
```

Once published to crates.io:

```bash
cargo install owasidea-cli
```

## CLI

Global flags: `--config`, `--manifest-dir`, `--http-addr`, `--json`.

```bash
owasidea doctor
owasidea init ./my-app
owasidea plugin new notes --lang rust --type library
owasidea build
owasidea inspect notes --json
```

## Environment variables

- `OWASIDEA_MANIFESTS` — manifests directory (default `../manifests`).
- `HOST_ADDR` — bind address (default `127.0.0.1:8787`).
- `WASMTIME_BIN` — path to wasmtime CLI.
- `TAILCAT_WASI_GATEWAY` / `TAILCAT_WASI_SELF_IP` — tailcat networking hints.

## Testing

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```