# Owasidea for AI agents

This is the deterministic playbook for an agent that must build, extend, debug,
or verify an Owasidea app. It assumes shell access in this repository.

**Default reference app:** `examples/shipyard`. When instructions conflict, trust
the executable source (`crates/host/src/lib.rs`, `crates/cli/src/main.rs`, WIT,
and the example build scripts) over prose.

---

## 0. Non-negotiable invariants

1. **Never hand-edit `manifests/*.manifest.json`.** They are generated from the
   real component WIT. Run `owasidea build`.
2. **Never add Extism or a JavaScript runtime to the host.** Headless plugins are
   WASI components. `js-wasm` is webview-only.
3. **WIT is the API.** If a function is not exported in WIT, `/call` cannot see
   it. Kebab-case WIT names stay kebab-case in HTTP (`next-version`,
   `save-report`). Rust plugins generate WIT from `#[owasidea::world]` and
   `#[owasidea::export]` into `.owasidea/wit`; do not commit that directory.
4. **Paths in `wasm-app.toml` are relative to the config file**, not the current
   working directory.
5. **UI-plugin output goes through `owasidea build`.** It hashes files, writes a
   `webview-v1` manifest, and copies assets into `ui/public/plugins/`.
6. **A bundled app needs both manifests and components.** `owasidea bundle`
   generates `bundle.resources` from `wasm-app.toml` (manifests, components and
   `runtime.json`); adding a plugin needs no manual config edit. `--target cli`
   stages a headless bundle.
7. **Do not put `node_modules`, source trees, or UI caches in Tauri resources.**
8. **Services are persistent instances.** Stop before reload; reload while
   running returns `409`.
9. **Use exact, least-privilege permissions.** No wildcards.
10. **Generated artifacts are disposable.** `.wasm`, manifests, `target/`, `bin/`,
    `obj/`, `dist/`, and `node_modules/` are not source.

---

## 1. Repository map

```
crates/host/                 Wasmtime runtime, manifest verification, HTTP API
crates/cli/                  `owasidea` CLI; reuses host as a library
crates/owasidea/             Rust attribute markers for source-generated WIT
examples/shipyard/           real end-to-end reference app
  wasm-app.toml              app config (plugins, permissions, desktop)
  plugins/*/                 one directory per plugin
  ui/                        product dashboard (vanilla TS + Vite)
  desktop/                   Tauri 2 shell for the example
  manifests/                 generated, ignored by git
  tests/ui.spec.ts           Playwright end-to-end tests
docs/GET_STARTED.md          human guide
docs/API.md                  HTTP API
docs/PLUGIN_AUTHORING.md     language-specific authoring notes
docs/RUNTIME_MODEL.md        routes, plugin-to-plugin, KV, network, sandbox timing
docs/UI_PLUGINS.md           isolated UI contract
docs/SECURITY.md             threat model and production policy
```

---

## 2. Fast path: build and verify the reference app

```bash
npm install                          # Playwright runner (repo root)
cd examples/shipyard
owasidea doctor --install-wasi-sdk   # optional, C/C++
owasidea build                       # compile, compose, generate + sign
owasidea smoke                       # temporary host + declarative API checks
owasidea test                        # build + smoke + Playwright
```

Expected build facts:

- 12 plugin manifests: 10 WASI + 2 UI.
- `examples/shipyard/manifests/pipeline-go.manifest.json` has `composition.strategy = "build-time"` and no `imports.plugins`.
- `examples/shipyard/.owasidea/build/*/*.component.wasm` exist for the 10 headless plugins.
- `examples/shipyard/ui/public/plugins/{board-svelte,map-react}/` contain `index.html`, `assets/*`, and are hashed into their manifests.

If you need the CLI without installing it:

```bash
cargo run -p owasidea-cli -- --config examples/shipyard/wasm-app.toml build
```

Use `OWASIDEA_DATA_DIR` so the build/run does not litter the repository with
`data/`:

```bash
export OWASIDEA_DATA_DIR=/tmp/owasidea-data
```

For C/C++:

```bash
owasidea doctor --install-wasi-sdk
```

`owasidea build` auto-detects `/opt/wasi-sdk`, `~/.local/wasi-sdk`, and the
reactor adapter in the Cargo registry, then injects `WASI_SDK_PATH` and
`WASI_PREVIEW1_ADAPTER` into the plugin build. Explicit environment variables
still win.

For repeated Rust builds, use a target directory on a large filesystem:

```bash
export CARGO_TARGET_DIR=/tmp/owasidea-target
```

---

## 3. Run and inspect

```bash
# Host only, including the config address
owasidea --config examples/shipyard/wasm-app.toml run

# Host + Vite + watcher (prints UI URL, host URL, plugins, watch roots)
owasidea --config examples/shipyard/wasm-app.toml dev

# Machine-readable manifest
owasidea --config examples/shipyard/wasm-app.toml inspect core-rust --json

# Check checksums and capability drift
owasidea --config examples/shipyard/wasm-app.toml validate

# Toolchain status
owasidea doctor --json
```

Overrides:

```bash
owasidea --config examples/shipyard/wasm-app.toml --http-addr 127.0.0.1:8890 run
owasidea --config examples/shipyard/wasm-app.toml --manifest-dir /tmp/manifests run
```

`--manifest-dir` is relative to the config directory by default.

---

## 4. HTTP API contract

Base URL is `[host].addr` or `--http-addr`.

| Operation | Request |
|---|---|
| Health | `GET /health` → `ok` |
| Overview | `GET /overview` |
| List headless plugins | `GET /plugins` |
| List UI plugins | `GET /ui-plugins` |
| Manifest | `GET /plugin/{name}/manifest` |
| Logs | `GET /plugin/{name}/logs` |
| Log stream | `GET /plugin/{name}/logs/stream` (SSE, structured JSON data: `event: log`, `data: {...}`) |
| Reload | `POST /plugin/{name}/reload` |
| Generic call | `POST /plugin/{name}/call` `{ "function": "...", "args": [...] }` |
| Service start | `POST /plugin/{name}/service/start` `{ "region_json": "" }` |
| Service status | `GET /plugin/{name}/service/status` |
| Service stop | `POST /plugin/{name}/service/stop` |
| Executable plugin | `* /app/{name}/...` |
| Metrics | `GET /metrics` |
| Dev status | `GET /dev/status` |

`/call` accepts JSON for primitives, lists, options, results, records, tuples,
enums, and flags. It resolves both top-level exports and functions inside
exported interfaces.

Example:

```bash
curl -sS -X POST http://127.0.0.1:8790/plugin/core-rust/call \
  -H 'content-type: application/json' \
  -d '{"function":"score","args":[["feat: add","fix: bug"],9,10]}'
# {"result":44}
```

For a `result<T,E>` export, the body is `{"result":{"ok":...}}` or
`{"result":{"err":...}}`.

---

## 5. Add a new plugin (agent recipe)

1. Pick language and type. Use an existing Shipyard plugin as the template.
2. Create `plugins/<name>/`:
   - Rust: `src/lib.rs` with `#[owasidea::world]` / `#[owasidea::export]`; the
     contract is generated into `.owasidea/wit`
   - Other languages: `wit/<name>.wit`
   - language source
   - build config (`Cargo.toml`, `go.mod`, `package.json`, `.csproj`, `Makefile`)
3. Add `[plugins.<name>]` in `wasm-app.toml` with `path`, `language`, `type`.
4. Add permissions only if WIT imports them (Rust: `#[owasidea::host_kv]`):
   - `poc:host/host-kv` → `kv = ["<namespace>:read-write"]`
   - `poc:host/host-log` → no config needed
   - `wasi:filesystem` → `fs = ["<guest-path>:read-write"]`
   - `wasi:http/outgoing-handler` → `http_outbound = ["exact.host:443"]`
   - `wasi:sockets` → `network_outbound = ["host:port"]`, `network_inbound = ["IP:port"]`
5. For a `wasi:http/incoming-handler` export, set `type = "executable"`.
6. For `start-service`/`service-status`, set `type = "service"`.
7. For a UI bundle, set `type = "ui"`, `runtime = "js-wasm"`, `ui_entry`, and
   optional `ui_build`.
8. Run `owasidea build`.
9. Run `owasidea validate`.
10. Add the new component to `examples/shipyard/desktop/tauri.conf.json` if the
    app is bundled.
11. Add a smoke assertion and, for UI, a Playwright assertion.

### Language-specific build outputs

The CLI expects `<plugin-name>.component.wasm` in the plugin directory.

| Language | Build command | Output note |
|---|---|---|
| Rust | `cargo component build --release` | copy `target/wasm32-wasip1/release/<crate>.wasm` |
| Go | `go tool componentize-go -d wit -w <pkg>/<world> build -o main.wasm`; SDK worlds use `go tool componentize-go build` | copy to `<name>.component.wasm` |
| TypeScript | `jco componentize ... -o <name>.component.wasm` | local `jco` in `node_modules` |
| C# | `dotnet build -c Release --source ""` | publish output `bin/Release/net10.0/wasi-wasm/publish/<name>.wasm` |
| C | `wit-bindgen c` + wasi-sdk clang + `wasm-tools component new` | Makefile must output `<name>.component.wasm` |
| C++ | `wit-bindgen cpp` + wasi-sdk clang++ + `wasm-tools component new` | same |

### Go gotcha

`go mod tidy` may remove the `tool` directive. The final `go.mod` must contain:

```go
require github.com/bytecodealliance/componentize-go v0.5.1
tool github.com/bytecodealliance/componentize-go
```

For an HTTP component, also require `go.wasmcloud.dev/component` and the
`go.bytecodealliance.org/pkg` pseudo-version used by the working example.

### Composition recipe

Provider WIT:

```wit
package example:core@0.1.0;
interface readiness { score: func(...) -> s32; }
```

Client WIT:

```wit
package example:pipeline@0.1.0;
world pipeline {
    import example:core/readiness@0.1.0;
    export run: func(...) -> string;
}
```

Config:

```toml
[plugins.pipeline-go]
compose_with = ["core-rust"]
```

Runtime dependencies use generated WIT bindings instead:

```toml
[plugins.app-glue]
dependencies = ["catalog-go"]
```

The same edge cannot be both `dependencies` and `compose_with`. Declarations may
cycle; a runtime re-entry fails with `OWASIDEA_E_DEPENDENCY_REENTRANT_CALL`.

`owasidea build` builds ordinary plugins first, then composes. Dependency
components are staged under single-dot filenames because `wasm-tools` derives a
package name from the path and rejects `core-rust.component.wasm`.

---

## 6. UI plugin recipe

```bash
owasidea plugin new my-panel --lang svelte --type ui
cd plugins/my-panel
npm install
npm run build          # must write dist/index.html
```

Config:

```toml
[plugins.my-panel]
path = "plugins/my-panel"
language = "svelte"
type = "ui"
runtime = "js-wasm"
ui_entry = "index.html"
ui_build = ["npm", "run", "build"]
```

Vite config: `base: './'`. The host copies `dist/` to
`ui/public/plugins/my-panel/`; Vite includes `public/` in `ui/dist` for Tauri.

The parent dashboard must verify `sha256` and every `files[path]` digest before
mounting the iframe. The example implementation is in
`examples/shipyard/ui/src/main.ts` (`verifyUiPlugin`).

---

## 7. Bundle recipe

```bash
owasidea --config examples/shipyard/wasm-app.toml build
owasidea --config examples/shipyard/wasm-app.toml bundle
owasidea --config examples/shipyard/wasm-app.toml bundle --bundles deb
```

Manifests are unsigned and checked by SHA-256. Release integrity comes from the
build pipeline and OS code signing of the installer; there is no plugin
`keygen`/`sign` step.

`bundle` reads `[desktop]` from the app config:

```toml
[desktop]
path = "desktop"
ui = "ui"
```

It runs `npm run build` in `ui`, then `cargo tauri build` in `desktop`. If
`[desktop].output` is set (Shipyard uses `dist`), built artifacts are copied
there.

The Tauri shell must resolve resources and app data:

```rust
let manifests = app.path().resource_dir()?.join("manifests"); // bundled
let data_dir = app.path().app_data_dir()?;
std::fs::create_dir_all(&data_dir)?;
std::env::set_var("OWASIDEA_DATA_DIR", &data_dir);
owasidea_host::run(manifests, "127.0.0.1:8790".into()).await?;
```

Inspect the Linux `.deb` without `dpkg`:

```bash
ar x Shipyard_0.1.0_amd64.deb
tar tzf data.tar.gz | grep -E 'manifests/|plugins/|trust/|usr/bin/'
```

On Windows, Tauri writes an NSIS installer under
`target\release\bundle\nsis\`; the same `dist` copy contains the `.exe`.

---

## 8. Verification checklist

Run these before declaring success:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
owasidea --config examples/shipyard/wasm-app.toml validate
owasidea --config examples/shipyard/wasm-app.toml smoke
owasidea --config examples/shipyard/wasm-app.toml test
```

For bundle work:

```bash
owasidea --config examples/shipyard/wasm-app.toml bundle
ls examples/shipyard/dist
```

Acceptance criteria for the reference app:

- [ ] `owasidea smoke` exits 0.
- [ ] `owasidea test` runs smoke + 5/5 Playwright tests.
- [ ] `/plugins` returns 10 headless plugins (or only the glue on a production host).
- [ ] `/ui-plugins` returns 2 UI plugins with per-file hashes.
- [ ] `pipeline-go` returns a composed verdict.
- [ ] `core-rust` saves and loads a report under `OWASIDEA_DATA_DIR`.
- [ ] `watchtower-rust` start/status/stop works.
- [ ] `/app/webhook-go/health` returns `ok: true`.
- [ ] The `.deb` contains `manifests/` and `plugins/*/*.component.wasm`.

---

## 9. Debugging matrix

| Failure | Inspect |
|---|---|
| Component import rejected | `crates/host/src/lib.rs` `validate_component_imports`; compare manifest `imports` to `wasm-tools component wit <file>` |
| `/call` says export not found | export list in `GET /plugin/{name}/manifest`; use exact kebab name |
| Service won't reload | `POST .../service/stop`, then reload |
| UI plugin hash mismatch | re-run `owasidea build`; rebuild Tauri so `ui/dist` matches the manifest |
| `checksum mismatch` after rebuild | restart the host or hot-reload the plugin; the host loaded the previous bytes |
| CORS blocked | host `OWASIDEA_ALLOWED_ORIGINS` includes the Vite origin |
| Bundle missing components | `tauri.generated.conf.json` `bundle.resources` (regenerated by `owasidea bundle`) |
| Host data written to repo | set `OWASIDEA_DATA_DIR`; desktop shell does this automatically |
| `wasm-tools compose` package error | stage dependencies as `<name>.wasm` (single dot) |
| Go tool missing after tidy | re-add `tool` + `require` in `go.mod` |

---

## 10. Prompt template for agents

```text
You are working in the Owasidea repository.
Read docs/AI_AGENTS.md and examples/shipyard/AGENTS.md.
Use examples/shipyard as the reference. Do not hand-edit generated manifests.
Run `owasidea --config examples/shipyard/wasm-app.toml build`, then
`owasidea test`.
If you add a plugin, it is packaged automatically from `wasm-app.toml`; add it
to the smoke/Playwright coverage. Report exact commands, exit codes, and
artifact paths.
```
