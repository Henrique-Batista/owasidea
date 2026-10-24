# Plugin Authoring Guide

## Rust (cargo-component)

1. Create `plugins/<name>/Cargo.toml` with `[workspace]` and component metadata.
2. Add WIT under `wit/` and deps under `wit/deps/host`.
3. Implement `Guest` trait, `export!`.
4. Build: `cargo component build --release`.
5. Copy `.wasm` to `plugins/<name>/<name>.component.wasm`.
6. Generate manifest: `python3 scripts/gen-manifest.py --component ... --name ... --language rust --type library ...`.

## Go (componentize-go)

1. `go.mod` with `tool github.com/bytecodealliance/componentize-go`.
2. Write `main.go` implementing WIT exports.
3. Build: `go tool componentize-go build`.
4. Copy `main.wasm` to `*.component.wasm`.
5. Generate manifest with `--language go`.

## Manifest rules

- Never hand-edit manifests; regenerate from WIT.
- Host rejects undeclared host imports.
- `max_fuel` is enforced via Wasmtime fuel metering.
- `timeout_ms` is enforced as per-request timeout.
- `max_memory_mb` is declared but not enforced yet.