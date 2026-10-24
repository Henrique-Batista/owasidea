# AGENTS.md — wasm-app framework POC

This is a **new** POC for the framework/CLI/UI. The feasibility POC lives at
`/home/pablitux/Projetos/extism-poc` and is the reference implementation.
Read this file before writing code. Do not re-litigate the transport choice.

## Read first

- `/home/pablitux/Projetos/extism-poc/README.md` — validated capabilities, commands, known gaps.
- `/home/pablitux/Projetos/extism-poc/host/src/lib.rs` — the real host: manifest loading, capability enforcement, routes, persistent service session.
- `/home/pablitux/Projetos/extism-poc/scripts/gen-manifest.py` — how manifests are derived from real WIT.
- `/home/pablitux/Projetos/extism-poc/ui/index.html` — current dashboard and API usage.
- `/home/pablitux/Projetos/extism-poc/forks/tailcat-wasi/` — only if the task touches the tailcat plugin.
- `/home/pablitux/Projetos/extism-poc/docs/` — `WASI_0_3.md`, `TAILCAT_WASI.md`, `FORK_POLICY.md`.

The reference repo is **not a git repository** and contains large generated
artifacts (37 MB component, browser `main.wasm`). Do not copy artifacts
blindly; copy source, scripts, WIT, and tests.

## Hard architecture constraints

- Desktop shell: **Tauri 2**; host is a reusable Rust library (`host`) embedded by the desktop app.
- Runtime: **Wasmtime 50 Component Model**, WASI 0.2 + WASI 0.3 p3.
- **Extism is rejected.** Do not add it.
- Headless plugins are **WASI components**, not `js-wasm`.
- `js-wasm` is only for the Tauri webview/UI micro-frontend, never for headless services.
- WIT contracts are mandatory. Manifests are **generated from the component's real WIT**, not hand-written.
- Plugins are bundled at build time. No plugin download at runtime.
- Plugins are untrusted: least-privilege capabilities are part of the framework, not a later feature.
- Do not add a JS runtime (QuickJS/Javy/Node) to the host.

## What is already proven (do not redo)

- Rust + Go components load in the same Wasmtime host; typed exports work.
- `wasi:http` inbound/outbound works for executable plugins (`chat-go`).
- WASI filesystem preopens and host KV/log work.
- Capability enforcement catches undeclared host imports.
- WASI 0.3 async TCP and UDP work; async component exports work.
- Real `tailscale/tailcat` runs as a WASI 0.3 component with DERP + direct UDP + UPnP/NAT-PMP/PCP.
- `stop-service` drops the component instance; next start gets fresh keys/address.

## New POC scope

Build the smallest end-to-end framework that answers:

1. What does a developer type to create/build/run an app? (CLI)
2. What does the user see and manipulate? (UI)
3. How does the host discover, validate, load, call, and sandbox plugins?
4. How are manifests and bundles produced reproducibly?
5. How does a plugin developer iterate? (dev mode/hot reload)

Non-goals for this POC: production security hardening, marketplace, remote
registry, cloud deployment, plugin signing, full IDE integration.

## Proposed layout

```
wasm-app-framework/
├── crates/
│   ├── host/              runtime, manifests, capability broker, HTTP API
│   ├── cli/               `wasm-app` binary (clap), reuses host as a library
│   └── desktop/           Tauri shell, embeds host
├── ui/                    Tauri frontend (framework choice open)
├── plugins/               example plugins
│   ├── notes/             Rust library + WIT
│   ├── chat-go/           Go executable + wasi:http
│   └── tailcat-wasi/      optional, only if artifact exists
├── manifests/             generated, do not hand-edit
├── templates/             `plugin new` templates
├── docs/                  ADRs, CLI spec, plugin author guide
├── tests/                 integration + UI smoke + CLI
├── wasm-app.toml          app config
└── AGENTS.md
```

Keep `crates/host` independent of Tauri. The CLI and desktop must both call the
same library. The UI must use the host HTTP API, not link Wasmtime directly.

## CLI contract v0

Binary name is a placeholder decision; use `wasm-app` unless the user chooses
another name. All commands must support `--json` for CI/agents where it makes
sense.

Required commands:

```text
wasm-app init <dir>                 scaffold app + wasm-app.toml
wasm-app doctor                     check rustc, cargo-component, wasm-tools,
                                    Go, componentize-go, wasmtime, Tauri deps
wasm-app plugin new <name> \
  --lang rust|go [--type library|executable]
wasm-app build                      build plugins, generate manifests, copy .wasm
wasm-app dev                        host + UI + watch + hot reload
wasm-app run                        run bundled/current app without watch
wasm-app inspect <plugin>           show WIT, exports, imports, permissions
wasm-app bundle                     produce Tauri app/bundle
```

CLI rules:

- `dev` must print the UI URL, host URL, loaded plugins, and watch paths.
- Every UI action must have a CLI/HTTP equivalent; the UI is not the only API.
- `build` must regenerate manifests from WIT; never accept a stale manifest silently.
- `build` must fail if a component imports a host interface not declared in policy.
- `dev` hot reload only for stateless library plugins in v0; stateful service
  plugins require an explicit restart (say so in UI/CLI).
- `--manifest-dir`, `--http-addr`, `--config`, and `--json` are global flags.
- Exit non-zero on build/validation errors; no interactive prompts in CI mode.

## Config (`wasm-app.toml`)

Keep source/build config separate from generated capabilities:

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

Permissions belong to the generated manifest/policy flow, not to ad-hoc CLI
flags. The user must be able to inspect effective permissions in the UI.

## Runtime/framework requirements

- Manifest model: name, version, language, `abi`, `runtime`, `type`, component,
  exports, imports (WASI/host/plugins), permissions, state, composition, metadata.
- `runtime`: `wasi-component` or `js-wasm`.
- `type`: `library`, `executable`, `hybrid`, and eventually `service`.
- Manifest generation: run `wasm-tools component wit --json` against the real
  component; derive every WASI/host import; support `async func`.
- Capability enforcement: only expose host interfaces declared by the manifest.
  Current reference only enforces host interfaces; do not claim WASI isolation
  is complete until it is.
- Generic invocation: v0 may keep typed handlers (`greet`, `complete`), but the
  framework needs a path to call arbitrary exports from WIT signatures.
- Stateful services: persistent instance + explicit lifecycle (`start`, `stop`,
  `status`) and host-driven progress. Do not model them as per-call exports.
- Logs: per-plugin ring buffer plus a streaming endpoint (SSE preferred) for UI.
- Hot reload: watch `.wasm`, validate new manifest, re-instantiate; keep a
  failure message if the new component does not instantiate.
- Resource limits (`fuel`, epoch timeout, memory) are in manifests today but not
  wired; wire them or label them clearly as not enforced.

## UI requirements (first vertical slice)

Screens/panels:

1. **App overview** — host status, app name/version, plugin count, runtime kinds.
2. **Plugins** — list with type/runtime/language, loaded/error state, exports.
3. **Plugin detail** — generated manifest, WIT imports/exports, permissions,
   logs, actions/forms derived from signatures.
4. **Services** — start/stop/status for service plugins; show address/state/events.
5. **Dev mode** — watched files, last reload result, reload button.

UI rules:

- Load from host API; no hardcoded plugin names or fake data.
- Distinguish `wasi-component` from `js-wasm` visibly.
- Show validation errors and failed reloads, not just success.
- UI actions must be idempotent/repeat-safe; never leave a service in an
  unknown state without a visible status.
- For the tailcat example, keep the service lifecycle and inbox/events panels;
  do not make the whole UI tailcat-specific.
- Prefer a small framework with a build step already familiar to the user;
  choice of React/Svelte/Solid/vanilla is an open decision. Do not add a large
  JS runtime to the host because of the UI.

## Build and verification

Reference commands (run from `extism-poc` only to validate the reference):

```bash
./scripts/build-all.sh
BUILD_TAILCAT_WASI=1 ./scripts/build-all.sh   # slow; clones/vendors upstream
./scripts/run-host.sh                         # http://127.0.0.1:8787
./scripts/run-desktop.sh
./scripts/test-e2e.sh
./scripts/test-wasi-sockets.sh
./scripts/test-tailcat-wasi.sh
./scripts/test-tailcat-service.sh
./scripts/test-tailcat-direct.sh
./scripts/test-tailcat-portmap.sh
```

New repo verification (target):

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test -p wasm-app-host <test_name>
wasm-app doctor
wasm-app build
wasm-app dev
# UI smoke: Playwright against the dev URL
```

Test requirements:

- Manifest derivation tests: sync and async exports, WASI imports, duplicate/unsupported signatures.
- Capability tests: tampered manifest must fail instantiation; undeclared host import must fail.
- CLI tests: `build`, `inspect --json`, `doctor`; use `assert_cmd` or equivalent.
- Integration: notes + chat-go + one service plugin; call from HTTP/UI.
- UI smoke: plugin list, manifest view, call action, error state.
- Tailcat tests are optional/slow; do not put them in the default `cargo test`.

## Known traps (hard-earned from the reference POC)

- **`componentize-go` must be a Go tool dependency** in `go.mod`:
  `tool github.com/bytecodealliance/componentize-go`, with Go 1.27.1 and the
  matching `require`. Without it: `go: no such tool "componentize-go"`.
- Use the componentize-go binary/toolchain it downloads; stock Go may not support
  async. The reference build notes say `/usr/sbin/go does not support async`.
- WASI p3 components with async `Store` require `call_async`, never `call`.
- `start-service`/`stop-service`: stop by dropping the component instance. Calling
  the guest `stop-server` with pending async sockets hung and tripped a Wasmtime
  50 debug assertion.
- Tailcat UDP adapter: `wasiudp` p3 `udp-socket.send`/`receive`; `Close` uses a
  self-addressed wake datagram and a read lock. Do not replace with a plain
  blocking UDP socket; it will stall.
- Tailcat `errClosed` must be `net.ErrClosed`; wireguard-go only treats that as a
  clean shutdown.
- Tailcat component build is slow (~minutes), clones upstream, vendors deps,
  applies 11 vendor patches. `build-all.sh` reuses the artifact unless
  `BUILD_TAILCAT_WASI=1`.
- Tailcat `send-text` should reuse a persistent client per target; creating and
  closing a client per message causes server-side WireGuard handshake retries.
- Tailcat port mapping needs `TAILCAT_WASI_GATEWAY`/`TAILCAT_WASI_SELF_IP`.
  Linux auto-detects from `/proc/net/route` + UDP route probe; other hosts must
  set both. `portmap=` empty means the router did not answer, not necessarily a bug.
- Fake gateway tests bind UDP 5351 (PXP) and 1900/49152 (UPnP). Do not run
  portmap cases in parallel.
- `test-tailcat-portmap.sh` starts its own hosts on 8795–8797; the desktop app
  may hold 8787. Use `HOST_ADDR`/`BASE` to isolate tests.
- Cross-NAT hole punching between arbitrary NATs is **not validated**. Direct
  UDP is validated on LAN/STUN; do not claim more.
- Do not commit 26–37 MB generated `.wasm` artifacts unless the user explicitly
  asks for a self-contained demo. Prefer build scripts and `.gitignore`.

## Working agreement

- Make the vertical slice work end-to-end before generalizing.
- Keep the host API stable and documented; UI and CLI are both clients.
- Every new capability needs a test and a manifest field or explicit “not enforced” label.
- When docs and code disagree, trust the executable source (`host/src/lib.rs`,
  scripts, WIT) and update the docs.
- Leave the reference POC untouched unless the task is specifically to fix it.
- Record non-obvious decisions in `docs/adr/` or here.

## Open decisions to confirm with the user

- Project/package name and CLI binary name.
- UI framework (React/Svelte/Solid/vanilla TS) and whether UI code lives in `desktop/` or root `ui/`.
- Config filename (`wasm-app.toml` proposed).
- First service plugin to expose beyond tailcat (or tailcat only).
- Whether `js-wasm` remains a supported plugin kind or a webview-only fallback.
- Permission UX: file pickers vs manifest text vs both.
