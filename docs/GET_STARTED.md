# Get Started with Owasidea

Owasidea builds a desktop app out of independent WebAssembly components. Each
plugin has its own language, build tool, WIT contract, manifest, and capability
set. The host is a reusable Rust library; the desktop shell is Tauri 2; the UI
talks to the host over HTTP and never links Wasmtime.

The reference application is [`examples/shipyard`](../examples/shipyard). It is a
real end-to-end app: Rust, Go, TypeScript, C#, C, and C++ components, build-time
composition, two isolated UI frameworks, persistence, a stateful service, an
executable `wasi:http` plugin, and Tauri packages (`.deb` / NSIS `.exe`).

Start here if you want to understand the whole system. The example README is the
short tour; this document is the manual.

---

## 1. Mental model

```
Tauri webview (React/Svelte/vanilla)
        │  HTTP + SSE
        ▼
Owasidea host (Rust, Wasmtime 50)
        │  WIT contracts
        ├── Rust component        (library, service)
        ├── Go component          (library, executable, composed)
        ├── TypeScript component  (jco)
        ├── C# component          (componentize-dotnet)
        ├── C component           (wasi-sdk + wit-bindgen)
        └── C++ component         (wasi-sdk + wit-bindgen)
```

Four files matter for every headless plugin:

| File / concept | Role |
|---|---|
| `wit/<name>.wit` | The contract: exports, imports, types, and whether a function is async. |
| Component `.wasm` | The compiled WASI Component Model artifact. |
| `wasm-app.toml` | Source/build config: path, language, type, permissions, composition. |
| `manifests/<name>.manifest.json` | Generated from the real WIT. Never hand-edit. |

The host verifies the component's actual imports against the generated manifest,
then exposes only the declared host interfaces. KV, host log, filesystem
preopens, networking, and HTTP are all deny-by-default.

Plugin types:

| Type | How the host calls it |
|---|---|
| `library` | `POST /plugin/{name}/call` with a WIT function and JSON args. A fresh instance is used per call, but the component is compiled once and cached. |
| `service` | `POST /plugin/{name}/service/start`, `GET .../status`, `POST .../stop`. One instance persists. |
| `executable` | Mounted at `/app/{name}`; the component exports `wasi:http/incoming-handler`. |
| `hybrid` | Both a stateful session and an HTTP entry point. |
| `ui` | `runtime = "js-wasm"`; a static browser bundle served to the Tauri webview in a sandboxed iframe. Never loaded by Wasmtime. |

---

## 2. Prerequisites

Required for the reference app:

| Tool | Version / purpose |
|---|---|
| Rust | `rustc` + `cargo`; `rustup target add wasm32-wasip1 wasm32-wasip2` |
| `cargo-component` | `cargo install cargo-component` |
| `wasm-tools` | `cargo install wasm-tools`; manifest generation and composition |
| Go | 1.27.1+ with `componentize-go` as a Go tool dependency |
| Node.js | 20+ and npm (UI and TypeScript plugins) |
| Python 3 | helper scripts and smoke tests |

Per-language extras:

| Language | Extra tooling |
|---|---|
| TypeScript | `npm install` in the plugin; `@bytecodealliance/jco` is a local dev dependency |
| C# | .NET SDK 10 + `dotnet workload install wasi-experimental` |
| C / C++ | [wasi-sdk](https://github.com/WebAssembly/wasi-sdk) and a reactor adapter |
| Tauri bundle | Tauri 2 system dependencies (`webkit2gtk-4.1`, `gtk3`, `libsoup3`, etc.) and `cargo-tauri` |
| Yew / Blazor / Uno UI plugins | the framework's own build tool (`trunk`, `dotnet publish`, ...) |

Versions, per-OS install commands, `doctor --fix` / `tools.lock` behavior and
the limitations of each optional toolchain are documented in
[`OPTIONAL_TOOLCHAINS.md`](OPTIONAL_TOOLCHAINS.md). A missing optional
toolchain only blocks the plugins written in that language.

The host itself is Rust + Wasmtime 50. It does not embed Node, QuickJS, or a
JavaScript runtime.

Windows developers: use PowerShell and see [`WINDOWS.md`](WINDOWS.md) for the
MSVC/MSYS2 prerequisites, NSIS bundle config, and cross-compilation boundaries.

### Install the CLI

```bash
cargo install --path crates/cli
npm install            # Playwright runner for `owasidea test`
npx playwright install # browser binaries
owasidea --help
```

For a one-off checkout without installing:

```bash
cargo run -p owasidea-cli -- --help
```

### C/C++ environment

`owasidea` auto-detects wasi-sdk at `/opt/wasi-sdk` or `~/.local/wasi-sdk`. To
install wasi-sdk 34 and copy the reactor adapter when available:

```bash
owasidea doctor --install-wasi-sdk
```

Then `owasidea build` injects `WASI_SDK_PATH` and `WASI_PREVIEW1_ADAPTER` for
C/C++ plugins automatically. You can still override either with an environment
variable.

If the adapter is missing, locate the copy from the Cargo registry:

```bash
export WASI_PREVIEW1_ADAPTER="$(find "$HOME/.cargo/registry/src" \
  -path '*wasi-preview1-component-adapter-provider-*/artefacts/wasi_snapshot_preview1.reactor.wasm' \
  | head -1)"
```

The adapter ships in the Rust registry when `wasm-tools`/Wasmtime dependencies
are present. `owasidea doctor` reports the main toolchain plus wasi-sdk and the
reactor adapter.

---

## 3. Run the reference app

```bash
cd examples/shipyard
owasidea doctor --install-wasi-sdk   # optional, C/C++ only
owasidea build                       # compiles, composes, generates + signs manifests
owasidea smoke                       # temporary host + every API assertion
owasidea test                        # build + smoke + Playwright UI tests
owasidea dev                         # build + host + Vite + watcher
```

`owasidea build` generates a local demo signing key under `keys/` when missing
and signs every manifest because `[signing].generate = true`. `keys/` is
gitignored; use a protected CI key for real releases.

Open the URL printed by `owasidea dev` (Shipyard uses
`http://127.0.0.1:5180` while the host is on `http://127.0.0.1:8790`). `dev` is a
foreground process; press `Ctrl+C` to stop Vite and the host. The Workbench calls every language through
the host API; Inventory shows generated manifests; Runtime drives the service
and the executable plugin; Isolated UI verifies and mounts the Svelte and React
bundles.

To build the desktop package:

```bash
owasidea bundle                              # all targets from config
owasidea bundle --bundles deb                # only .deb
owasidea bundle --bundles deb,rpm,appimage   # explicit Linux targets
owasidea bundle --bundles nsis,msi           # Windows
owasidea bundle --profile staging --bundles deb   # overlay + dist/staging
```

Profiles (`dev`, `staging`, `prod`) merge an optional `wasm-app.<profile>.toml`
over `wasm-app.toml` and write artifacts to `dist/<profile>`; the base config is
never edited. See [`CLI.md`](CLI.md#bundle-profiles).

AppImage uses `linuxdeploy`. The CLI enables extract-and-run and disables strip
automatically. If linuxdeploy fails on a host with VMware gdk-pixbuf libraries,
hide the vendor directory from the bundler (no root, no system change):

```bash
owasidea bundle --bundles appimage --isolate-host-libs
```

The flag requires `bwrap`. See
[`APPIMAGE.md`](APPIMAGE.md) for details.

The example targets Linux `deb` and, through
`desktop/tauri.windows.conf.json`, Windows NSIS. Change `bundle.targets` for
rpm/AppImage or another platform. The Tauri shell sets
`OWASIDEA_REQUIRE_SIGNATURES=true`, resolves the bundled `trust/owasidea.pub`,
and rejects unsigned or tampered manifests at startup.

The Linux `.deb` contains the Tauri binary, generated manifests, component
`.wasm` files, and the trust key. Windows NSIS/MSI packages use the same
resources. If a target fails after others were produced (for example AppImage
without a working `linuxdeploy`), the successful artifacts are still copied to
`dist/` and the command exits non-zero so CI can notice the partial failure.

---

## 4. Create a new app from zero

```bash
owasidea init my-app
cd my-app
owasidea doctor
```

`init` creates `wasm-app.toml`, `plugins/`, `manifests/`, `ui/`, and `docs/`.
The generated UI is the generic Owasidea dashboard; replace `ui/` when you want a
product-specific frontend.

### 4.1 Configure the app

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

[dev]
watch = true
hot_reload = true
ui_addr = "127.0.0.1:5180"

[signing]
key = "keys/owasidea.key"
public_key = "keys/owasidea.pub"
generate = true
require = true
```

`ui_addr` fixes the Vite dev address so `owasidea dev`, docs and Playwright all
use the same URL. If it is occupied, `dev` fails instead of silently choosing
another port. `[signing]` makes build/bundle sign manifests; `generate = true`
is a demo convenience, not a production policy.

Paths in `wasm-app.toml` are relative to the config file. This means
`owasidea --config examples/shipyard/wasm-app.toml build` works from any
working directory.

### 4.2 Add plugins

```bash
owasidea plugin new my-core --lang rust --type library
owasidea plugin new my-audit --lang go --type library
owasidea plugin new my-notes --lang typescript --type library
owasidea plugin new my-policy --lang csharp --type library
owasidea plugin new my-native --lang c --type library
owasidea plugin new my-metrics --lang cpp --type library
owasidea plugin new my-service --lang rust --type service
owasidea plugin new my-webhook --lang go --type executable
owasidea plugin new my-board --lang svelte --type ui
```

`plugin new` writes the scaffold under the config file's directory and registers
the plugin in `wasm-app.toml`. TypeScript and UI plugins need their own
`npm install`.

The `plugin new` templates are intentionally minimal. For a complete working
example of each language, including build scripts and Makefiles, copy the shape
from `examples/shipyard/plugins/*`.

### 4.3 Write a contract

A library plugin's WIT is the API:

```wit
package example:my-core@0.1.0;

interface readiness {
    score: func(changes: list<string>, checks-passed: u32, checks-total: u32) -> s32;
    save-report: func(key: string, content: string) -> result<string, string>;
}

world my-core {
    import poc:host/host-kv@0.1.0;
    import poc:host/host-log@0.1.0;

    export readiness;
}
```

The host's generic dispatcher resolves both top-level exports and functions
inside exported interfaces:

```bash
curl -sS -X POST http://127.0.0.1:8787/plugin/my-core/call \
  -H 'content-type: application/json' \
  -d '{"function":"score","args":[["feat: add"],9,10]}'
```

Use the exact WIT name. Kebab-case stays kebab-case: `save-report`, not
`save_report`.

Supported JSON argument types include `bool`, all integer widths, floats,
`string`, `char`, `list<T>`, `option<T>`, `result<T,E>`, records, tuples, enums,
and flags. See `docs/API.md` and the host's `json_to_val` implementation.

### 4.4 Build and run

```bash
owasidea build
owasidea validate
owasidea run
owasidea dev
```

`build` does all of this:

1. Compiles each configured plugin with its own toolchain.
2. Installs missing npm dependencies for TypeScript/UI plugins.
3. Auto-detects wasi-sdk + reactor adapter for C/C++.
4. Runs source generators when a plugin uses `owasidea` annotations.
5. Composes plugins that declare `compose_with` **after** ordinary plugins.
6. Runs `wasm-tools component wit --json` to derive imports/exports.
7. Writes a manifest with a SHA-256 checksum.
8. Signs manifests when `[signing]` is configured.
9. Copies verified UI-plugin assets into `ui/public/plugins/<name>/`.

`test` builds, starts a temporary host, runs the declarative `[smoke]` checks,
then executes `[test].ui` (Playwright in the example). `smoke` runs only the API
checks. `run` starts only the host. `dev` builds first, then starts the host, the
Vite dev server, and a recursive watcher. The watcher hot-reloads stateless components; stateful
services must be stopped first. `dev` prints the UI URL, host URL, loaded
plugins, and watch roots.

### 4.5 Declarative tests

A project does not need shell scripts to test itself. `owasidea test` builds,
starts a temporary host with a temporary `OWASIDEA_DATA_DIR`, runs the checks in
`[smoke]`, stops the host, then runs `[test].ui`:

```toml
[test]
ui = ["npx", "playwright", "test", "--config", "playwright.config.ts"]

[[smoke.calls]]
plugin = "core-rust"
function = "score"
args = [["feat: add"], 9, 10]
min = 0
max = 100

[[smoke.http]]
path = "/app/webhook-go/health"
expect_contains = "shipyard-webhook"

[[smoke.services]]
plugin = "watchtower-rust"
```

`owasidea smoke` runs only the API half. The framework also checks `/health`,
every configured headless plugin, and the UI plugin count.

### 4.6 Manifests and permissions

The generated manifest is the security boundary. The host compares the actual
component imports with it and fails startup on an undeclared interface.

```toml
[plugins.my-core]
path = "plugins/my-core"
language = "rust"
type = "library"
kv = "my-core:read-write"              # host KV namespace + access
fs = ["reports:read-write"]            # WASI preopen
http_outbound = ["api.example.com"]    # exact host allowlist
network_outbound = ["192.0.2.10"]      # exact IP/hostname allowlist
network_inbound = ["127.0.0.1"]        # exact local bind IPs
persistent = true
```

Rules:

- Wildcards are rejected for HTTP and network permissions.
- FS preopens are created under `OWASIDEA_DATA_DIR` when that variable is set
  (desktop bundles do this); otherwise they are relative to the process working
  directory.
- A component that imports `poc:host/host-kv` must have a matching `kv`
  permission or `build` fails.
- Resource limits (`max_fuel`, `timeout_ms`, `max_memory_mb`, `max_instances`)
  are generated today and enforced by the host store.
- Never edit a manifest by hand. Rebuild it from WIT.

---

## 5. Isolated UI plugins

UI plugins are static browser bundles. The Rust host never instantiates them and
the webview never gets a privileged bridge.

```toml
[plugins.my-board]
path = "plugins/my-board"
language = "svelte"
type = "ui"
runtime = "js-wasm"
ui_entry = "index.html"
ui_build = ["npm", "run", "build"]
```

The build output must be in `plugins/my-board/dist/`. Vite configs should set
`base: './'` so assets resolve inside the iframe. `owasidea build` copies the
output to `ui/public/plugins/my-board/`, hashes every file, and writes a
`webview-v1` manifest.

`owasidea build` also writes `ui/public/ui-plugins.json` next to those assets.
The dashboard:

1. Reads `ui-plugins.json` from the same static bundle as the assets.
2. Fetches every file and checks its SHA-256 against that index.
3. Checks the aggregate bundle digest.
4. Mounts the entry in an iframe with
   `sandbox="allow-scripts allow-forms"` (no same-origin, no popups, no
   top-navigation).

Shipping the index with the assets avoids a build-skew false negative: a
desktop binary built before the latest `owasidea build` still verifies the
bundle it actually contains. `GET /ui-plugins` exposes the same metadata from
the signed manifests for other clients.

The dashboard and the UI plugin share no JavaScript state, and an isolated
plugin has **no host bridge**. It cannot call `POST /plugin/{name}/call`
directly: the iframe has an opaque origin and the host CORS allowlist does not
include `null`. To give a plugin backend data, proxy the calls through the
dashboard with an explicit allowlist, or keep the isolated plugin
presentational. The route mapping for non-isolated frontends is documented in
[`API.md`](API.md).

The example ships two different frameworks:

- `plugins/board-svelte` — Svelte 4 release board.
- `plugins/map-react` — React 18 architecture map.

Yew, Blazor, and Uno use the same static-bundle contract; set the framework's
build command in `ui_build` and point `ui_entry` at the produced HTML.

---

## 6. Build-time composition between languages

Interop has two layers:

1. **API composition** — one UI or CLI orchestrates multiple plugins through
   HTTP. This is what the Shipyard workbench does across six languages.
2. **Component composition** — `wasm-tools compose` links a client component to
   a provider component so the import disappears before the host sees it.

Example:

```toml
[plugins.pipeline-go]
path = "plugins/pipeline-go"
language = "go"
type = "library"
compose_with = ["core-rust"]
kv = "shipyard-pipeline:read-write"
fs = ["reports:read-write"]
persistent = true
```

`pipeline-go` imports `shipyard:core/readiness@0.1.0` in WIT. `owasidea build`:

1. Builds `core-rust` first.
2. Builds `pipeline-go` as a client component.
3. Copies each dependency to a simple staged filename.
4. Runs `wasm-tools compose` to produce `pipeline-go.component.wasm`.
5. Generates the manifest from the composed component, which now has no plugin
   imports.

Dependencies are sorted so ordinary plugins build before composed clients. For a
chain of composed plugins, build the providers first or invoke `owasidea build`
twice.

---

## 7. Stateful services and executable HTTP plugins

Service contract:

```wit
world watchtower {
    export start-service: func() -> result<string, string>;
    export service-status: func() -> bool;
}
```

```bash
curl -X POST localhost:8790/plugin/watchtower-rust/service/start \
  -H 'content-type: application/json' -d '{"region_json":""}'
curl localhost:8790/plugin/watchtower-rust/service/status
curl -X POST localhost:8790/plugin/watchtower-rust/service/stop
```

Start is idempotent; stop drops the instance. Hot reload returns `409` while the
service is running. The generic service path uses `start-service`, optional
`service-status`, and host-driven progress is intentionally not modeled as a
per-call export.

Executable plugin contract:

```wit
world webhook {
    export wasi:http/incoming-handler@0.2.0;
}
```

With `type = "executable"` the host mounts it at `/app/<name>` and forwards the
method, path, query, headers, body, and streaming response. The Go example uses
`wasihttp.HandleFunc`; Rust/TypeScript/C# executable components can export the
same WASI interface.

---

## 8. Package with Tauri

Add a desktop shell to the app (the Shipyard example has one at
`examples/shipyard/desktop`) and point `wasm-app.toml` at it:

```toml
[desktop]
path = "desktop"
ui = "ui"
```

The Tauri config should include:

```json
{
  "build": { "frontendDist": "../ui/dist" },
  "app": {
    "security": {
      "csp": "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost http://127.0.0.1:8790; frame-src 'self'"
    }
  },
  "bundle": {
    "active": true,
    "targets": ["deb"],
    "resources": {
      "../manifests/": "manifests/",
      "../plugins/core-rust/core-rust.component.wasm": "plugins/core-rust/core-rust.component.wasm"
    }
  }
}
```

The desktop `main.rs` should:

1. Resolve manifests from `$RESOURCE/manifests` when bundled, or `../manifests`
   during development.
2. Set `OWASIDEA_DATA_DIR` to the platform app-data directory.
3. Start `owasidea_host::run(manifests, addr)` on a background task.
4. Let the webview talk to `http://127.0.0.1:8790`.

Then:

```bash
owasidea build       # regenerate manifests + components
owasidea sign <name> --manifest-dir manifests --key keys/owasidea.key
owasidea bundle      # npm run build in ui/, cargo tauri build in desktop/
```

With `[signing]` in `wasm-app.toml`, `owasidea build` and `owasidea bundle` sign
every manifest automatically. The example uses `generate = true` for a local demo
key; production should point `key` at a CI secret and set `generate = false`.

`bundle` uses `[desktop] path` and `[desktop] ui`, prints the artifact location,
and fails clearly if the Tauri config or UI is missing.

Important packaging details:

- Include only component `.wasm` files and manifests as Tauri resources. Do not
  bundle `node_modules`, source trees, or UI-plugin build caches.
- When you add a plugin, add its component to `bundle.resources`; otherwise the
  installed app has a manifest that points at a missing component.
- Keep the host on loopback. The Tauri webview and the host share the machine;
  this is not a remote authentication boundary.
- Production release builds require signed manifests unless explicitly disabled.
  See `docs/SECURITY.md`.

### 8.1 Updates (Tauri updater)

The desktop shells ship the Tauri 2 updater. Configure it in `wasm-app.toml`
and let `bundle` produce the manifest:

```toml
[updater]
channel = "stable"
endpoint = "https://releases.example.com/{{channel}}/latest.json"
pubkey = "keys/owasidea-updater.pub"
```

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/owasidea-updater.key)"
owasidea bundle --bundles appimage,nsis   # writes *.sig + dist/latest.json
owasidea latest --artifacts dist --check  # validate before publishing
```

`[updater]` is projected into `plugins.updater` in the generated Tauri config;
`OWASIDEA_UPDATE_CHANNEL`, `OWASIDEA_UPDATER_ENDPOINT`, and
`OWASIDEA_UPDATER_PUBKEY` override it without editing files. The private key is
never committed or embedded. Full flow, channels, local-server testing, and
rollback: [`UPDATER.md`](UPDATER.md); release checklist: [`RELEASE.md`](RELEASE.md).

---

## 9. Verify

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
owasidea --config examples/shipyard/wasm-app.toml validate
owasidea --config examples/shipyard/wasm-app.toml smoke
owasidea --config examples/shipyard/wasm-app.toml test
owasidea verify dist/
```

`owasidea bundle` stages the runtime payload (`manifests/`, `plugins/*.component.wasm`,
`ui/public/`, `trust/owasidea.pub`) next to the installers, so
`owasidea verify dist/` checks the exact manifests, component hashes,
signatures and UI asset digests that ship. It fails with `NO_MANIFESTS` when
pointed at a directory that has no manifests, instead of silently passing.

The Shipyard smoke test asserts:

- 9 WASI plugins + 2 isolated UI plugins load.
- Every language responds through `/call`.
- The composed Go→Rust pipeline returns a scored verdict.
- Host KV and the `reports/` preopen persist a report.
- The service starts, reports `running`, and stops.
- The executable webhook answers `/health` and accepts an event.

The Playwright suite exercises the real UI against the real host: workbench
calls, isolated UI hash verification + iframe mount, runtime controls, the
plain-JS SHA-256 fallback used when `crypto.subtle` is unavailable, and the
inventory/logs panels.

---

## 10. Troubleshooting

| Symptom | Fix |
|---|---|
| `go: no such tool "componentize-go"` | Keep `tool github.com/bytecodealliance/componentize-go` and a matching `require` in `go.mod`; do not let `go mod tidy` drop the tool. |
| `npm build failed` for a TypeScript plugin | Run `npm install` in that plugin directory. |
| `jco: command not found` | `npm install` in the TypeScript plugin; the build script uses the local `jco`. |
| C/C++ `WASI_SDK_PATH` missing | Install wasi-sdk and export `WASI_SDK_PATH`; export `WASI_PREVIEW1_ADAPTER` too. |
| C# `dotnet` build fails | `dotnet workload install wasi-experimental`; check the `net10.0` target and componentize-dotnet package version. |
| Host starts but UI says offline | Check `[host] addr`, the Vite `OWASIDEA_HOST` value, and CSP `connect-src`. |
| CORS error in the browser | `dev` sets allowed origins automatically. For a manual Vite port, set `OWASIDEA_ALLOWED_ORIGINS` on the host. |
| UI dev port already in use | `owasidea dev` uses `[dev].ui_addr` (Shipyard: 5180) and fails clearly instead of silently picking another port. Stop the stale Vite process. |
| `409` on reload | Stop the stateful service, reload, then start it again. |
| `checksum mismatch` after a rebuild | Restart the host (or let `dev` reload the plugin). A running host keeps the old component bytes. |
| UI plugin integrity error | Re-run `owasidea build`; rebuild the Tauri app so `ui/dist` and the hashed manifest match. |
| Bundle starts but plugins are missing | Check that `bundle.resources` includes every component and that manifests resolve `$RESOURCE/manifests`. |
| `component not found after build` | The language build did not produce `<plugin-name>.component.wasm`; align the Makefile/script output name with the plugin name. |

---

## 11. Command reference

```text
owasidea init <dir>                 scaffold app + wasm-app.toml
owasidea doctor [--json] [--install-wasi-sdk]
owasidea plugin new <name> \
  --lang rust|go|typescript|javascript|csharp|c|cpp|yew|blazor|uno|svelte|react \
  --type library|executable|service|ui
owasidea build [--no-sign]          compile, compose, generate + sign manifests
owasidea test [--no-build] [--no-smoke] [--no-ui]
owasidea smoke [--no-build]         declarative API checks
owasidea validate                   checksums, capabilities, UI manifests
owasidea run                        host only (current build)
owasidea dev [--no-build]           build + host + UI + watcher + hot reload
owasidea inspect <plugin> [--json]  manifest + WIT
owasidea bundle [--no-build]        build + sign + Tauri package
owasidea latest [--check]           generate/validate dist/latest.json (updater)
owasidea wit generate [path]        regenerate WIT from source annotations
owasidea keygen --out <dir>         Ed25519 signing key
owasidea sign <plugin> --key <key>  sign a release manifest
owasidea registry search <q> --url <index>
owasidea registry install <name> --version <v> --url <index> --public-key <key>
```

Global flags: `--config`, `--manifest-dir`, `--http-addr`, `--json`.
`--manifest-dir` is resolved relative to the config file's directory.

Next: read [`examples/shipyard/README.md`](../examples/shipyard/README.md) for
the concrete architecture, [`docs/AI_AGENTS.md`](AI_AGENTS.md) for a
machine-oriented playbook, [`docs/WINDOWS.md`](WINDOWS.md) for Windows and
cross-compilation boundaries, or [`docs/APPIMAGE.md`](APPIMAGE.md) for AppImage
packaging and [`docs/CLEAN_BUILD.md`](CLEAN_BUILD.md) for clean-build options, including bubblewrap and chroots.
