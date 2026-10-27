# AGENTS.md — Shipyard reference app

Read `docs/AI_AGENTS.md` and this file before changing the example. Shipyard is
the canonical end-to-end app; keep it buildable and keep the framework smoke and
Playwright tests passing.

## Build contract

```bash
cd examples/shipyard
owasidea doctor --install-wasi-sdk   # optional, C/C++
owasidea build                       # compile, compose, generate + sign
owasidea smoke                       # temporary host + declarative API checks
owasidea test                        # build + smoke + Playwright
owasidea dev                         # build + host + Vite + watcher
owasidea bundle                      # build + sign + UI + Tauri -> dist/
```

All orchestration is in the `owasidea` CLI. The example intentionally has no
shell scripts. Build configuration, signing, smoke assertions and the UI test
command live in `wasm-app.toml`.

`owasidea build` compiles ordinary plugins first, then plugins with
`compose_with`. It installs missing npm dependencies, auto-detects wasi-sdk and
the reactor adapter, signs manifests when `[signing]` is configured, and copies
UI assets into `ui/public/plugins/`.

Use `OWASIDEA_DATA_DIR` for manual runs. Use `CARGO_TARGET_DIR` on constrained
disks.

## Files that are generated — never edit

- `manifests/*.manifest.json`
- `plugins/*/*.component.wasm`
- `plugins/*/*.client.wasm`
- `plugins/*/target/`, `bin/`, `obj/`, `gen/`, `dist/`
- `ui/dist/`, `ui/public/plugins/`, `ui/.owasidea/`
- `plugins/*/node_modules/`, `ui/node_modules/`
- `desktop/target/`
- `dist/` (bundle artifacts)
- `keys/` (demo signing key; never commit)

## Adding a plugin

1. Add a new directory under `plugins/`; do not reuse an existing plugin's name.
2. Add its WIT contract. Use exact kebab-case names.
3. Add `[plugins.<name>]` in `wasm-app.toml` with path/language/type.
4. Add KV/fs/http/network permissions only for WIT imports.
5. Run `owasidea build`; run `owasidea test` for the full suite. Signing needs
   `owasidea keygen --out keys` once (the example does not auto-generate keys).
6. Packaging resources are generated from `wasm-app.toml`; no manual
   `desktop/tauri.conf.json` edit is needed for a new plugin.
7. Add assertions under `[[smoke.calls]]`, `[[smoke.http]]` or
   `[[smoke.services]]` in `wasm-app.toml`.
8. For UI, add a Playwright test in `tests/ui.spec.ts`.

## Regenerating Go bindings

When a Go plugin's WIT changes, regenerate its bindings and keep the result:

```bash
cd plugins/audit-go
go tool componentize-go -d wit -w shipyard:audit/audit bindings
gofmt -w .
go mod tidy
# re-add the tool directive if tidy removed it
```

`pipeline-go` uses `-w shipyard:pipeline/pipeline`. Its `shipyard_core_readiness`
package comes from `wit/deps/core/core.wit`.

## Language output names

The CLI expects `plugins/<name>/<name>.component.wasm`:

- Rust: copy `target/wasm32-wasip1/release/<crate>.wasm`.
- Go: copy `main.wasm`.
- TypeScript: `jco componentize ... -o <name>.component.wasm`.
- C#: copy the publish output.
- C/C++: Makefiles must write `<name>.component.wasm` (the plugin name, not the
  WIT world name).

## Windows

Native Windows uses the same CLI commands. `desktop/tauri.windows.conf.json`
selects NSIS and `icons/icon.ico`. C/C++ needs wasi-sdk for Windows plus
MSYS2 `make`, or a per-plugin `build` override:

```toml
[plugins.fingerprint-c]
build = ["C:/msys64/usr/bin/make.exe"]
```

Cross-compiling the Windows installer from Linux is not supported by
`owasidea bundle`; use a Windows runner. See `docs/WINDOWS.md`.

## Known traps in this app

- `pipeline-go` depends on `core-rust`. It must be composed, not treated as a
  standalone Go component. Its manifest must have `imports.plugins: []`.
- `core-rust` and `pipeline-go` need `kv` because the composed Rust core imports
  `poc:host/host-kv`; without it `build` rejects the manifest.
- `core-rust` writes to the `reports` preopen. Tests set `OWASIDEA_DATA_DIR`, so
  reports land under that temp directory.
- `/call` names are WIT names: `save-report`, `load-report`, `next-version`.
  Underscore aliases do not exist.
- UI plugin Vite configs must use `base: './'` so iframe assets resolve.
- After changing UI plugin assets, run `owasidea build` **and** `owasidea bundle`
  so the hashes in `manifests/` match `ui/dist`.
- The Tauri resource map is explicit. A new plugin that is not in the map will
  make the installed app fail to start its host.
- A long-running host keeps the component bytes it loaded at startup. Rebuild +
  restart (or hot reload) before re-running browser tests, or `/call` returns
  `checksum mismatch`.
- `[dev].ui_addr` is fixed at `127.0.0.1:5180`. If the port is occupied,
  `owasidea dev` fails clearly; stop the stale Vite process.

## Acceptance checklist

- [ ] `owasidea build` exits 0 and signs 11 manifests.
- [ ] `owasidea smoke` exits 0.
- [ ] `owasidea test` runs smoke + Playwright 5/5.
- [ ] `owasidea validate` says OK.
- [ ] `pipeline-go` manifest has no `imports.plugins`.
- [ ] `core-rust` saves/loads a report under `OWASIDEA_DATA_DIR`.
- [ ] `watchtower-rust` start/status/stop works.
- [ ] `/app/webhook-go/health` returns `ok: true`.
- [ ] `.deb` contains `manifests/`, `trust/owasidea.pub`, and all component files.
- [ ] Host starts with `OWASIDEA_REQUIRE_SIGNATURES=true` and the bundled key.
