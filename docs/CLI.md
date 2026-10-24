# CLI Reference

Binary: `owasidea`. All commands support `--json` where applicable.

## Global flags

- `--config <path>` (default `wasm-app.toml`)
- `--manifest-dir <path>` (default `manifests`)
- `--http-addr <addr>` (default `127.0.0.1:8787`)
- `--json`

## Commands

### `init <dir>`

Scaffold app directory with `wasm-app.toml`, `plugins/`, `manifests/`, `ui/`, `docs/`.

### `doctor`

Check toolchain: rustc, cargo, cargo-component, wasm-tools, go, node, pnpm.

### `plugin new <name> --lang rust|go [--type library|executable]`

Scaffold a plugin directory.

### `build`

Run `scripts/build-all.sh` (build plugins, generate manifests, copy `.wasm`). Fails on build/validation errors.

### `dev`

Print UI URL, host URL, loaded plugins, watch paths; run host.

### `run`

Run bundled/current app without watch.

### `inspect <plugin>`

Show manifest (WIT imports/exports, permissions). `--json` for raw.

### `bundle`

Produce Tauri bundle (not implemented in v0).