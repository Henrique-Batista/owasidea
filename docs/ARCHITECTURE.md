# Architecture

## Components

- `crates/host` — reusable Rust library: manifest loading, capability broker,
  Wasmtime runtime, HTTP API, and SSE logs. Independent of Tauri.
- `crates/cli` — `owasidea` binary (clap), reuses host and owns build,
  composition, validation, dev-watch, signing, and registry flows.
- `crates/desktop` — generic Tauri 2 shell for the repository root app.
- `crates/macros` — Rust source-generator attributes.
- `examples/shipyard` — full reference app with its own plugins, UI, Tauri shell,
  manifests, scripts, and tests.
- `manifests/*.manifest.json` — generated, never hand-edit.
- `scripts/gen-manifest.py` — legacy helper used by the root demo scripts.
  `owasidea build` performs the same derivation in Rust.

## Runtime

- Wasmtime 50, Component Model, WASI 0.2 + 0.3 p3.
- Plugins are WASI components described by WIT.
- `runtime`: `wasi-component` (loaded by Wasmtime) or `js-wasm` (sandboxed
  webview only; never loaded by the host).
- `type`: `library`, `executable`, `hybrid`, `service`, or `ui`.
- Library calls instantiate per request from a component compiled once at load
  (artifacts are cached on disk and reused across restarts). Service lifecycle
  keeps one instance until stop. Executable HTTP components use embedded
  `wasmtime-wasi-http` `ProxyPre`, not a child process.

## App model

`wasm-app.toml` is source/build configuration: app identity, host address,
plugin paths/languages/types, permissions, composition, and the optional desktop
shell. Paths are relative to the config file's directory.

`owasidea build`:

1. Compiles ordinary plugins with their native toolchains.
2. Installs missing npm dependencies and auto-detects wasi-sdk/adapter.
3. Compiles `compose_with` clients and links provider components with
   `wasm-tools compose`.
4. Derives imports/exports from `wasm-tools component wit --json`.
5. Writes manifests with checksums, permission objects, resource limits, and
   state metadata.
6. Signs manifests when `[signing]` is configured.
7. Copies and hashes UI-plugin static bundles into `ui/public/plugins/`.

The same config drives the developer workflow: `[smoke]` contains declarative
API assertions executed by `owasidea smoke`/`owasidea test`, `[test].ui` is the
argv for the browser suite, and `[desktop].output` copies Tauri artifacts into
the app directory.

## Capability enforcement

The host verifies actual component imports against manifest declarations before
instantiation. WASI filesystem access is limited to canonicalized declared
preopens; sockets default-deny and require explicit outbound hosts/IPs or local
inbound bind IPs. `wasi:http` outbound uses exact host allowlists. Environment is
not inherited; only the tailcat integration receives its explicitly selected
variables. Ed25519 signatures bind the manifest and component. See
`docs/SECURITY.md` for production policy and remaining OS-level limits.

Mutable host state is rooted at `OWASIDEA_DATA_DIR` when set, otherwise `./data`.
KV databases live at `<root>/<namespace>/kv.sqlite`. Relative filesystem
permissions resolve under the same root. Desktop shells set the environment
variable to the platform app-data directory before starting the host.

## Desktop packaging

The Tauri shell embeds the host and serves the built frontend from
`frontendDist`. Manifests and component `.wasm` files are declared explicitly in
`bundle.resources`, preserving the layout:

```
$RESOURCE/manifests/<name>.manifest.json
$RESOURCE/plugins/<name>/<name>.component.wasm
```

Tauri platform configs (`tauri.windows.conf.json`) select NSIS and `.ico` while
the base config owns resources. At startup the shell resolves
`$RESOURCE/manifests`, sets `OWASIDEA_DATA_DIR`, and runs
`owasidea_host::run`. The webview talks to the host over loopback HTTP;
it never links Wasmtime or receives native plugin capabilities.

## Data flow

UI/CLI → HTTP → host → instantiate component → typed call → response. Executable
HTTP components use embedded `wasi:http` inbound handling. Stateful services
retain one instance until stop/reload. Logs stream via SSE. UI plugins are copied
into the app's static assets and loaded through a sandboxed iframe after per-file
integrity checks.

The reference flow is documented in
[`examples/shipyard/README.md`](../examples/shipyard/README.md).

## Module conventions

- New CLI commands: `crates/cli/src/<area>.rs`, registered with
  `mod <area>;` in `crates/cli/src/main.rs`. No global refactor.
- New host features: `crates/host/src/<area>.rs`, registered with
  `mod <area>;` in `crates/host/src/lib.rs`.
- Prefer one item = one new module/file. Avoid growing `main.rs`/`lib.rs`.
- Unit tests in the module; integration tests in `crates/*/tests/`.
- Do not move existing code unless required by the item.
- Examples: `crates/cli/src/config.rs`; host capability broker in `lib.rs`.

## CLI templates (AX-01)

Plugin/init templates live in `crates/cli/templates/` and are embedded at compile time via `include_str!` (see `crates/cli/src/templates.rs`). Placeholders use `{{name}}` style and are rendered by `templates::render`.
