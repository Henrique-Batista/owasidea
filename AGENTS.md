# AGENTS.md — Owasidea 1.3

Owasidea is a polyglot application framework: one Rust/Wasmtime host loads
independent WASI components (Rust, Go, TypeScript, C#, C, C++) and serves a
desktop (Tauri 2) or headless/CLI app. Plugins are untrusted; the generated
manifest is the capability boundary.

This is the **1.x product**, not the old feasibility POC. The reference POC at
`/home/pablitux/Projetos/extism-poc` is historical; do not copy its artifacts.

## Read first

- `README.md` — product overview and commands.
- `docs/GET_STARTED.md` — install, scaffold, build, run, bundle.
- `docs/RUNTIME_MODEL.md` — UI/plugin routes, plugin-to-plugin, KV, network,
  and when the sandbox runs.
- `docs/AI_AGENTS.md` — deterministic agent/CI playbook.
- `examples/shipyard/AGENTS.md` — canonical end-to-end app.
- `docs/adr/` — accepted decisions (runtime, bridge, streaming).

## Product scope

- Targets: **desktop (Tauri 2)** and **CLI/headless** (`owasidea bundle
  --target cli`).
- Languages: Rust, Go, TypeScript/JavaScript, C#, C, C++ (see
  `docs/PLUGIN_AUTHORING.md`).
- Plugin kinds: `library`, `service`, `executable` (`wasi:http`), `ui`
  (`js-wasm`, webview only).
- Developer experience is the priority: one CLI, generated manifests, clear
  errors, fast `dev` loop, observability in the TUI.
- Non-goals: remote registry/marketplace, hosted accounts, cloud deployment,
  a JS runtime in the host, Extism, plugin signing in the default path (it is
  opt-in), full IDE integration.

## Hard architecture constraints

- Desktop shell: Tauri 2; the host is a reusable Rust library (`crates/host`)
  embedded by the desktop app. Keep `crates/host` independent of Tauri.
- Runtime: Wasmtime 50 Component Model, WASI 0.2 + WASI 0.3 p3.
- Headless plugins are WASI components. `js-wasm` is only for the webview/UI
  micro-frontend, never for headless services.
- WIT contracts are mandatory. Manifests are generated from the real component
  WIT (`wasm-tools component wit --json`), never hand-written.
- Plugins are bundled at build time. No plugin download at runtime.
- Capabilities are deny-by-default: filesystem, sockets, HTTP, KV and host
  interfaces must be declared and are enforced by the host.
- The UI uses the host HTTP API, never links Wasmtime directly.
- Do not add QuickJS/Javy/Node to the host.
- Do not add a remote registry. Plugins are source repositories + build
  artifacts.

## Layout

```
crates/
  host/       runtime, manifests, capability broker, HTTP API
  cli/        `owasidea` binary; reuses host as a library
  owasidea/   `#[owasidea::*]` attribute markers for generated WIT
examples/
  shipyard/   full reference app (6 languages, services, UI, Tauri)
  hello/ notes/ ticker/   minimal apps
plugins/      focused plugin examples used by the root demo
ui/           vanilla TypeScript + Vite dashboard
manifests/    generated, never edit
.owasidea/    generated build store: deps, components, caches (never edit)
templates/    `plugin new` / `init` templates
docs/         guides, reference, ADRs, plans
tools/tui-lab/  TUI framework prototypes (not part of the workspace)
```

## Commands

```text
owasidea init <dir> [--preset minimal|desktop|full]
owasidea doctor [--fix|--install-all]
owasidea plugin new <name> --lang rust|go|typescript|csharp|c|cpp
owasidea build [--no-sign] [--fail-on-widen]
owasidea dev [--no-build] [--desktop] [--no-tui]
owasidea run
owasidea test [--no-build] [--no-smoke] [--no-ui]
owasidea smoke
owasidea inspect <plugin> [--json]
owasidea call <plugin>.<fn> [--args ...]
owasidea logs <plugin> [--follow]
owasidea validate [--json]
owasidea verify <dist>
owasidea bundle [--target desktop|cli] [--bundles deb,rpm,...]
owasidea tui
owasidea config render
owasidea client --lang ts -o <file>
owasidea schema | explain | status | latest | sbom | signing plan
```

Global flags: `--config`, `--manifest-dir`, `--http-addr`, `--json`.
Every command that matters to CI supports `--json`; exit non-zero on failure.

## Config (`wasm-app.toml`)

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

Permissions belong in the generated manifest/policy flow, not ad-hoc CLI flags:
`kv = ["ns:read-write"]`, `fs = ["reports:read-write"]`,
`http_outbound = ["api.example.com:443"]`,
`network_outbound = ["192.0.2.10:443"]`,
`network_inbound = ["127.0.0.1:8080"]`. HTTP/network values are exact
`host:port`/`IP:port` pairs, never URLs or wildcards. The effective permissions must be visible in `inspect`, `validate`, the UI
and `permissions.lock.json`.

Plugin manifests are **unsigned**; SHA-256 checksums are always checked.
Integrity comes from the build pipeline and package/code signing of the release,
not from per-plugin Ed25519 signatures. There is no `[signing]` plugin section.

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
scripts/test-examples.sh hello notes ticker
cd examples/shipyard && owasidea build && owasidea smoke
```

For a CLI bundle:

```bash
owasidea bundle --target cli --no-build
dist/cli/run.sh
curl http://127.0.0.1:8787/health
```

Test requirements:

- Manifest derivation: sync/async exports, WASI imports, duplicate/unsupported
  signatures.
- Capability tests: tampered manifest fails; undeclared import fails.
- CLI tests: `build`, `inspect --json`, `doctor`, `bundle --target cli`.
- Integration: notes + chat-go + one service plugin, called over HTTP.
- UI smoke: plugin list, manifest view, call action, error state.
- Keep Tailcat tests optional/slow; do not put them in the default `cargo test`.

## Working agreement

- Make the vertical slice work end-to-end before generalizing.
- Every new capability needs a test and a manifest field or an explicit
  “not enforced” label.
- Keep the host API stable and documented; UI and CLI are both clients.
- When docs and code disagree, trust the executable source and fix the docs.
- Do not commit generated `.wasm`, `dist/`, manifests, `node_modules` or keys.
- Record non-obvious decisions in `docs/adr/` or here.
- Module conventions (B0.7): new CLI commands go in `crates/cli/src/<area>.rs`,
  new host features in `crates/host/src/<area>.rs`, wired with `mod <area>;`.
  One item = one module/file where possible. Do not append hundreds of lines to
  `main.rs`/`lib.rs`. Unit tests in the module; integration tests in
  `crates/*/tests/`.

## Known traps

- **`componentize-go` must be a Go tool dependency** in `go.mod`:
  `tool github.com/bytecodealliance/componentize-go`, with the matching Go
  requirement. Without it: `go: no such tool "componentize-go"`.
- Use the componentize-go toolchain it downloads; stock Go may not support async.
- WASI p3 components with async `Store` require `call_async`, never `call`.
- `start-service`/`stop-service`: stop by dropping the component instance.
  Calling the guest `stop-server` with pending async sockets hung and tripped a
  Wasmtime 50 debug assertion.
- Tailcat UDP adapter: `wasiudp` p3 `udp-socket.send`/`receive`; `Close` uses a
  self-addressed wake datagram and a read lock. Do not replace with a blocking
  UDP socket; it will stall.
- Tailcat `errClosed` must be `net.ErrClosed`; wireguard-go only treats that as
  a clean shutdown.
- Tailcat `send-text` should reuse a persistent client per target; creating and
  closing a client per message causes WireGuard handshake retries.
- Tailcat port mapping needs `TAILCAT_WASI_GATEWAY`/`TAILCAT_WASI_SELF_IP`.
  Linux auto-detects; other hosts must set both. `portmap=` empty means the
  router did not answer, not necessarily a bug.
- Fake gateway tests bind UDP 5351 (PXP) and 1900/49152 (UPnP). Do not run
  portmap cases in parallel.
- Fake gateway tests and the desktop app may both want 8787; use `HOST_ADDR`/
  `BASE` to isolate.
- Cross-NAT hole punching between arbitrary NATs is **not validated**. Direct
  UDP is validated on LAN/STUN; do not claim more.
- Do not commit 26–37 MB generated `.wasm` artifacts unless the user explicitly
  asks for a self-contained demo.
