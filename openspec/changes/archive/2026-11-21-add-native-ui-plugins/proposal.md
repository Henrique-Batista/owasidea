# Proposal

## Why

Owasidea only supports webview UI plugins: static web assets rendered inside a sandboxed iframe in the Tauri shell (`docs/UI_PLUGINS.md`). Teams that want a native, GPU-accelerated UI — such as Rust applications built with GPUI, the framework behind Zed — must either leave the Owasidea runtime or hand-roll their own process and packaging integration. This blocks the polyglot promise for native desktop UIs and, concretely, blocks the Forja learning project, which needs a GPUI desktop client consuming the same host-mediated plugin API as the web dashboards.

GPUI is currently published as `gpui` 0.2.2 (2025-10-22 on crates.io) and is still pre-1.0, so the integration must be version-pinned and isolated rather than woven into the host.

## What Changes

- Add a new plugin kind, `native-ui`, for UI binaries that run as trusted local processes instead of iframe assets.
- Add native UI build configuration (`native_build`, `native_entry`) and manifest generation with a new `runtime = "native"` marker plus per-binary SHA-256.
- Validate native UI declarations during `owasidea build`/`validate`: reject webview-only fields, missing binaries, unsupported targets, and remote/network entries.
- Start and supervise native UI processes in `owasidea run` and `owasidea dev`, injecting the host URL and plugin identity through environment variables.
- Expose native UI entries in `GET /plugins` and a generated `native-ui.json` index, so launchers and tooling can discover them.
- Package native UI binaries in desktop bundles (Tauri sidecars/resources) and in CLI/headless bundles.
- Generate a `native-ui` plugin template and a GPUI example app, with documentation in `docs/UI_PLUGINS.md` and `docs/CLI.md`.
- Clarify the trust boundary: unlike WASI plugins and iframe UIs, native UIs run with full user privileges; the docs and manifest must state that they are trusted code.

**BREAKING**: `type = "ui"` keeps its current webview meaning. No existing plugin changes behaviour; `runtime = "native"` is additive.

## Capabilities

### New Capabilities

- `native-ui-plugins`: declaration, validation, build, manifest, and discovery of native UI plugins.
- `native-ui-lifecycle`: start, health, restart, and shutdown supervision of native UI processes during `run` and `dev`.
- `native-ui-packaging`: inclusion of native UI binaries in desktop and CLI bundles, with integrity records.

### Modified Capabilities

None. No main specs exist yet in this repository, and the webview UI behaviour is unchanged.

## Impact

- **Code**: `crates/cli/src/config.rs` (plugin schema), `crates/cli/src/main.rs` (plugin scaffolding, build, run, dev), `crates/cli/src/validate.rs` and `verify.rs`, `crates/cli/src/bundle_cli.rs`, Tauri bundle flow, `crates/host/src/lib.rs` (discovery listing), new `crates/cli/src/native_ui.rs`.
- **Configuration**: new optional `native_build` / `native_entry` plugin fields; new manifest `runtime` value.
- **Packaging**: Tauri desktop bundles gain sidecar binaries; CLI bundles gain `native/` directory plus launcher flags.
- **Security**: native UIs are trusted local code and cannot be capability-restricted like WASI components or iframe bundles; this must be explicit in validation output and docs.
- **Docs/examples**: `docs/UI_PLUGINS.md`, `docs/CLI.md`, `docs/API.md`, `docs/PLUGIN_AUTHORING.md`, a new `examples/gpui-counter`, and a new CLI template.
- **Consumers**: the Forja learning project (separate repository) will consume `native-ui` for its GPUI client.
- **Compatibility**: the host and CLI remain wire-compatible with existing apps; apps without native UI plugins produce the same manifests and bundles as before.
