# Design

## Context

See `proposal.md` for motivation and the three delta specs for required behaviour.

Current Owasidea UI support is webview-only (`docs/UI_PLUGINS.md`): `owasidea build` runs `ui_build`, copies static output to `ui/public/plugins/<name>/`, and writes a `js-wasm` manifest whose asset digests the dashboard verifies before creating an iframe. The host (`crates/host/src/lib.rs`) loads every `*.manifest.json`, compiles its `component`, and exposes it through `GET /plugins`; UI plugins are deliberately excluded from WIT loading.

The CLI already models per-plugin build commands (`ui_build`, `build`) as argv arrays, stages runtime payloads (`crates/cli/src/dist_stage.rs`), and supports Tauri desktop and CLI bundles (`crates/cli/src/bundle_cli.rs`). Those are the seams this change extends.

Constraints:

- GPUI is pre-1.0 (`gpui` 0.2.2, published 2025-10-22) and may break between releases, so it must be pinned and isolated to templates/examples.
- The host must not attempt to load native UI binaries as WASI components.
- Native UI processes run with full user privileges; they cannot be sandboxed like WASI components or iframe UIs.
- Tauri packaging requires sidecar names with the target triple and a matching `externalBin` entry.

## Goals / Non-Goals

**Goals:**

- One declarative plugin kind that builds, runs, discovers, verifies, and bundles native UI binaries.
- Zero change for apps without native UI plugins.
- A working GPUI reference example and scaffold in this repository.
- Clear trust disclosure at every surface (build output, index, docs).

**Non-Goals:**

- Embedding a native window inside the Tauri window, or sharing a single event loop between GPUI and Tauri.
- Hot reload of native UI code in `dev` v1 (manual restart only).
- Cross-compiling native UI binaries.
- Sandboxing, capability brokering, or IPC beyond the host's existing loopback HTTP API.
- Support for every native UI toolkit; only one scaffold (GPUI) ships in this change.

## Decisions

### Decision: native UI as a supervised sidecar process

The native UI runs as its own executable, started by the CLI, communicating with the host over the existing loopback HTTP API (`OWASIDEA_HOST`). GPUI therefore never shares a process, window, or event loop with Tauri.

Alternatives rejected:

- **Embed GPUI in the Tauri window** — requires taking over the native window handle and reconciling two event loops; fragile across platforms and Tauri versions.
- **Make GPUI the application shell** — replaces the host and webview dashboard, breaking every existing app and the capability model.
- **Render the native UI to WASM/canvas** — no longer native, and GPUI has no stable wasm target.

### Decision: `native-ui` type, `native` runtime

`type = "ui"` keeps meaning webview; `runtime = "native"` is a new marker. This preserves ABI compatibility (`webview-v1` vs the new `native-v1`) and lets the CLI branch before any WIT work. Validation rejects mixing webview fields (`ui_entry`, `ui_build`, `bridge`) with native fields.

### Decision: one `native-ui.json` index, not WIT manifests

Per-plugin manifests would land in the directory the host scans and force the loader to special-case them. Instead the build writes `native-ui.json`:

```json
{
  "schema": "native-v1",
  "trust": "unrestricted-local-process",
  "plugins": [
    { "name": "board", "type": "native-ui", "runtime": "native",
      "binary": "plugins/board/target/release/board",
      "sha256": "…" }
  ]
}
```

The host reads this file at startup and merges entries into `GET /plugins` with no component, no exports, and no route. Missing or malformed index is non-fatal: the host logs a warning and serves the remaining plugins.

Alternatives rejected:

- **Per-plugin `*.manifest.json`** — collides with `load_manifests`, which expects a component; requires loader exceptions.
- **Embed in `wasm-app.toml` only** — the host only reads generated artifacts, and bundles need a self-describing record.

### Decision: path-safety rules match existing UI assets

`native_entry` is resolved with the same canonicalisation used for UI assets (`config_render::safe_resolve`): must stay inside the plugin directory, must exist after `native_build`, and must be a regular file. Build commands run in the plugin directory with no shell, exactly like `ui_build`.

### Decision: CLI waits for host readiness, then spawns children

`owasidea run` starts the host as today, probes `GET /health` (or the HTTP port) until it accepts connections with a 30 s deadline, then spawns every native UI with `OWASIDEA_HOST`, `OWASIDEA_PLUGIN`, and `OWASIDEA_MANIFESTS`. Logs are forwarded line-by-line with a `[name]` prefix. The supervisor owns the children: it reaps them on exit and on SIGINT/SIGTERM, records unexpected exits, and never restarts them in `run`.

For `dev`, the existing dashboard (`crates/cli/src/tui`) gains a native UI section backed by the same supervisor. Restart is explicit; the watcher does not restart native binaries.

### Decision: Tauri sidecars for desktop, plain copies for CLI

Desktop bundling copies each binary to `<desktop>/binaries/<name>-<target-triple>` and adds it to the generated Tauri config under `bundle.externalBin`. The Tauri shell resolves sidecars at runtime; the CLI bundle instead copies to `native/<name>/`, rewrites `native_entry` in the bundled config, and lets the launcher start them through the normal `owasidea run` path.

Cross-compilation is rejected when native UI plugins exist: the binary produced on the host cannot be assumed to match the requested target.

### Decision: pin GPUI and keep it out of the core workspace

The scaffold uses `gpui = "=0.2.2"` (latest crates.io release at access time) and pins `Cargo.lock` in the example. Plugin directories are excluded from the root workspace (`exclude = ["plugins/*"]`); the example gets its own `[workspace]` table so `cargo build` from the plugin directory never mutates the framework workspace. Zed's split crates and unofficial mirrors are not used.

## Risks / Trade-offs

- **[GPUI 0.2.2 is frozen and pre-1.0]** → Pin the exact version, isolate it in the template/example, and document the upgrade path; the learning project pins the same version.
- **[Native UI binaries escape the capability model]** → Validation, build output, index, and docs all state `trust = unrestricted`; never present native UI as sandboxed.
- **[Tauri sidecar naming/version drift]** → Generate the sidecar name from the host target triple at bundle time and cover it with an integration test that asserts the filename and `externalBin` entry.
- **[Linux GPU/display dependencies make CI flaky]** → Example smoke tests run headless (`--no-native-ui`) and only compile the GPUI binary in CI; manual run instructions cover display requirements.
- **[Process leaks on crash]** → Use a single supervisor owner, `kill_on_drop`, and a process-group kill on Unix; test Ctrl+C behaviour.
- **[Index tampering]** → `owasidea verify` checks every digest against `native-ui.json`; bundling fails on missing binaries.
- **[Two UI models confuse authors]** → Documentation adds a decision table (webview vs native) and keeps `type = "ui"` unchanged.

## Migration Plan

1. Land schema, build, discovery, and tests; native plugins remain unused by existing apps.
2. Ship the scaffold and GPUI example behind documentation.
3. Add run/dev supervision and bundles.
4. Announce `native-ui` as additive; no migration is required. Rollback is removing native UI entries and the generated index; apps without them are unaffected.

## Open Questions

None blocking. Whether to add hot reload, more native toolkits, or a shared IPC bridge can be decided in later changes.
