# Plugin Authoring Guide

## Rust (cargo-component)

1. Create `plugins/<name>/Cargo.toml` with `[workspace]` and component metadata.
2. Add WIT under `wit/` and deps under `wit/deps/host`.
3. Implement `Guest` trait, `export!`.
4. Build: `cargo component build --release`.
5. Copy `.wasm` to `plugins/<name>/<name>.component.wasm`.
6. Generate manifest: `python3 scripts/gen-manifest.py --component ... --name ... --language rust --type library ...`.

## C/C++ (wasi-sdk + wasm-tools + wit-bindgen)

1. Scaffold: `owasidea plugin new <name> --lang c|cpp`.
2. Annotate the source with `// owasidea:...` markers (see `docs/SOURCE_GENERATORS.md`).
   `owasidea wit generate` (and `owasidea build`) regenerates `wit/<name>.wit`.
3. Build the generated bindings and implementation with the scaffolded `Makefile`
   (`make`), which runs `wit-bindgen` and `clang`/`clang++`.
4. Copy the resulting `*.component.wasm` if needed and generate the manifest with
   `--language c` or `--language cpp`.

## C# (.NET WASI)

Supported target frameworks: net8.0, net9.0, net10.0 and future net11.0.

Notes:
- .NET WASI produces a **command component** (`wasi:cli/run`), not a WIT library.
- The framework builds it as an executable plugin, but serving HTTP requires
  `wasi:http/incoming-handler` (not provided by the default template), so
  `/app/{name}` will not work for the default template.
- For WIT-based libraries (callable via `/call`), use Rust/Go/TypeScript (jco).

1. Install .NET SDK + wasi workload + wasi-sdk.
2. Build with `dotnet publish -c Release -r wasi-wasm`.
3. The framework copies `dotnet.wasm` and generates the manifest.
4. Run via host; HTTP proxy may not apply (command component).

## TypeScript/JavaScript (jco)

1. Install `@bytecodealliance/jco`.
2. `jco componentize src/index.ts --wit wit --world name -o name.component.wasm`.
3. Generate manifest.

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
- `max_memory_mb` is enforced via Wasmtime StoreLimits.