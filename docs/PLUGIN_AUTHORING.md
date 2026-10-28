# Plugin Authoring Guide

## Rust (cargo-component)

Rust plugins generate their contract from source markers, so a plugin does not
need a `wit/` directory or a WIT file:

1. Scaffold with `owasidea plugin new <name> --lang rust`.
2. Mark the contract in `src/lib.rs`:
   - `#[owasidea::world("<name>")]` on the component struct.
   - `#[owasidea::export]` on every function that belongs to the contract.
   - `#[owasidea::host_log]` / `#[owasidea::host_kv]` on the struct to import
     those host interfaces (`kv` permission still comes from `wasm-app.toml`).
3. Implement `Guest`, `export!`.
4. Run `owasidea build`. It renders `<name>.wit` into `.owasidea/wit`
   (an artifact; gitignored), compiles the component against it, and derives the
   manifest from the real component. The generated WIT is regenerated on every
   build, and the build fails if the markers and a hand-written WIT disagree.
5. Rust names are converted to WIT names: `start_service` becomes
   `start-service`, and `some_arg` becomes `some-arg`.

`Cargo.toml` points cargo-component at the artifact:

```toml
[package.metadata.component.target]
path = ".owasidea/wit"
world = "<name>"
```

`wit_source` in `wasm-app.toml` selects the source:

| Value | Behavior |
| --- | --- |
| `attributes` | Render the contract from markers into `.owasidea/wit`; no WIT file is read or committed. Works for Rust, Go, TypeScript and C#. |
| `wit` | Use a hand-written `wit/`; if markers exist, the build fails when they diverge. |
| `auto` (default) | Use `wit/` when it exists, otherwise render from markers. |

A `wit/` directory next to an attribute plugin is an error: remove it or set
`wit_source = "wit"`. Committing `.owasidea/wit` is never necessary. Go and
C/C++ use `// owasidea:world` / `// owasidea:export`; TypeScript uses the
`@owasidea-*` JSDoc tags; C# uses `[OwasideaWorld("library")]` and
`[OwasideaExport]` plus `Attributes.cs`.

The attribute mapper covers scalars, `String`/`&str`, `Vec`, slices, arrays,
`Option`, `Result`, tuples, `Box`/`Arc`/`Rc`, and kebab-cased named types. Rust
records, enums, variants, flags and resources are not generated yet: for those,
keep a WIT contract and set `wit_source = "wit"` (the build can still check it
against markers).

## C/C++ (wasi-sdk + wasm-tools + wit-bindgen)

1. Scaffold: `owasidea plugin new <name> --lang c|cpp`.
2. Annotate the source with `// owasidea:world` / `// owasidea:export` markers
   (see `docs/SOURCE_GENERATORS.md`). `owasidea build` regenerates
   `.owasidea/wit/<name>.wit`; the Makefile reads that artifact.
3. Build the generated bindings and implementation with the scaffolded `Makefile`
   (`make`), which runs `wit-bindgen` and `clang`/`clang++`. Set `WASI_SDK_PATH`
   and `WASI_PREVIEW1_ADAPTER` for your installed toolchain; templates contain no
   developer-specific absolute paths.
4. Build output must be `<plugin-name>.component.wasm`; `owasidea build` derives
   the manifest from it.

On Windows, use wasi-sdk for Windows and a `make` from MSYS2, or override the
build command:

```toml
[plugins.fingerprint-c]
build = ["C:/msys64/usr/bin/make.exe"]
```

The example Makefiles select `clang.exe`/`clang++.exe` when `OS=Windows_NT`.

## C# (.NET WASI)

Supported target frameworks: net8.0, net9.0, net10.0 and future net11.0.

Notes:
- .NET WASI produces a **command component** (`wasi:cli/run`), not a WIT library.
- The framework builds it as an executable plugin, but serving HTTP requires
  `wasi:http/incoming-handler` (not provided by the default template), so
  `/app/{name}` will not work for the default template.
- For WIT-based libraries (callable via `/call`), use Rust/Go/TypeScript (jco).

1. Install .NET SDK + wasi workload + wasi-sdk. Set `WASI_SDK_PATH` when needed.
   The project needs the platform LLVM runtime package:
   `runtime.linux-x64.microsoft.dotnet.ilcompiler.llvm` on Linux,
   `runtime.win-x64.microsoft.dotnet.ilcompiler.llvm` on Windows.
2. Build with `dotnet publish -c Release -r wasi-wasm`.
3. The framework copies `dotnet.wasm` and generates the manifest.
4. Run via host; HTTP proxy may not apply (command component).

## TypeScript/JavaScript (jco)

1. Install `@bytecodealliance/jco`.
2. `jco componentize src/index.ts --wit wit --world name -o name.component.wasm`.
3. Generate manifest.

## Verify the scaffolds compile (FIX-03)

`scripts/test-templates.sh` scaffolds a temporary app
(`owasidea init --preset minimal`), creates Rust, Go, and TypeScript library
plugins with `owasidea plugin new`, runs `owasidea build`, and validates every
generated manifest (name, runtime, `greet` export, component file, checksum).

```bash
cargo build -p owasidea-cli
scripts/test-templates.sh                              # skip missing toolchains
OWASIDEA_TEMPLATE_STRICT=1 scripts/test-templates.sh   # fail on missing toolchain
OWASIDEA_TEMPLATE_LANGS=rust,go scripts/test-templates.sh
```

Environment:

- `OWASIDEA_BIN`: CLI binary. Defaults to `$CARGO_TARGET_DIR/debug/owasidea`
  when `CARGO_TARGET_DIR` is set, otherwise `target/debug/owasidea`.
- `OWASIDEA_TEMPLATE_LANGS`: comma-separated list, default
  `rust,go,typescript`.
- `OWASIDEA_TEMPLATE_STRICT=1`: a missing toolchain is an error instead of a
  skip.
- `OWASIDEA_TEMPLATE_KEEP=1`: keep the temporary app and print its path.
- `OWASIDEA_TEMPLATE_BUILD=1 cargo test -p owasidea-cli --test cli
  templates_build`: run the same script from the Rust integration test. Without
  the variable the test is skipped, so the default `cargo test` stays fast.

CI runs the script in the fast job with all three languages and strict mode.
The TypeScript scaffold does not need a global `jco`: `npm install` brings the
`@bytecodealliance/jco` devDependency used by `npm run build`.

## Go (componentize-go)

1. `go.mod` with `tool github.com/bytecodealliance/componentize-go` and
   `module wit_component` (the generated glue imports
   `wit_component/export_wit_world`).
2. Implement the WIT exports in `export_wit_world/wit_bindings.go`.
3. Build with `owasidea build`. On the first build it generates `wit_exports.go`
   with `componentize-go bindings`, runs `go mod tidy`, then runs
   `componentize-go build`; later builds reuse `wit_exports.go`. A Go module
   with `componentize-go.toml` (for example `go.wasmcloud.dev/component`) owns
   its own world and is built with `componentize-go build` without `wit/`.
4. Copy `main.wasm` to `*.component.wasm`.
5. Generate manifest with `--language go`.

To regenerate the glue after changing WIT, delete `wit_exports.go` (or run
`go tool componentize-go -d wit -w <pkg>/<world> bindings` manually) and build
again.

## Manifest rules

- Never hand-edit manifests; regenerate from WIT.
- Host rejects undeclared host imports.
- `max_fuel` is enforced via Wasmtime fuel metering.
- `timeout_ms` is enforced as per-request timeout.
- `max_memory_mb` is enforced via Wasmtime StoreLimits.
- `max_instances` limits concurrent component stores per plugin.
- WASI sockets are denied unless `permissions.network.outbound` lists exact host
  names/IPs or `permissions.network.inbound` lists exact local bind IPs.
- Never use wildcard HTTP/network permissions in a production manifest.
- Sign release manifests after finalizing WIT, permissions, component, and UI
  asset hashes. Production hosts must set `OWASIDEA_REQUIRE_SIGNATURES=true`
  and `OWASIDEA_PUBLIC_KEY`.

## Stateful services

Declare `type = "service"` and export `start-service` (zero arguments or one
string argument) plus optional `service-status: func() -> bool`. The host keeps
the same component instance between lifecycle calls. `stop` drops the instance;
reload while running is rejected and requires an explicit stop/restart.

## Webview UI plugins

See [`UI_PLUGINS.md`](UI_PLUGINS.md) for Yew, Blazor, Uno, and other static
browser/WASM bundles. UI plugins use `runtime = "js-wasm"`, `type = "ui"`; they
are not headless components and receive no WASI imports.

## Complete reference implementation

[`examples/shipyard`](../examples/shipyard) contains working, freshly built
plugins in every supported language. Use it as the source of truth for build
files and output naming:

| Plugin | Language | Notes |
|---|---|---|
| `core-rust` | Rust | interface export, host KV/log, filesystem preopen |
| `audit-go` | Go | `componentize-go bindings` generated package + host log |
| `changelog-ts` | TypeScript | local `jco` build, top-level export |
| `policy-csharp` | C# | componentize-dotnet interface export + host log |
| `fingerprint-c` | C | wit-bindgen C + wasi-sdk Makefile |
| `metrics-cpp` | C++ | wit-bindgen C++ + wasi-sdk Makefile |
| `pipeline-go` | Go | imports a Rust WIT interface and is composed at build time |
| `watchtower-rust` | Rust | persistent service lifecycle |
| `webhook-go` | Go | executable `wasi:http/incoming-handler` |

Build output must be `plugins/<name>/<name>.component.wasm`. If your Makefile
uses the WIT world name (`fingerprint.component.wasm`), the CLI will stop with
`component not found after build`; set an explicit plugin output name.

## Build-time composition

Provider WIT:

```wit
package shipyard:core@0.1.0;

interface readiness {
    score: func(changes: list<string>, checks-passed: u32, checks-total: u32) -> s32;
    verdict: func(score: s32) -> string;
}
```

Client WIT:

```wit
package shipyard:pipeline@0.1.0;

world pipeline {
    import shipyard:core/readiness@0.1.0;
    export run: func(version: string, changes: list<string>, checks-passed: u32, checks-total: u32) -> string;
}
```

Config:

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

`owasidea build` builds non-composed plugins first, then stages each dependency
under a single-dot filename and runs `wasm-tools compose`. This is required
because `wasm-tools` derives a package name from the definition path and rejects
names such as `core-rust.component.wasm`.

The composed manifest must have `imports.plugins: []`. If it still lists plugin
imports, the host refuses to instantiate it with:
`declares unresolved plugin imports; compose dependencies at build time`.

## HTTP call names

The generic dispatcher uses exact WIT names. Kebab-case stays kebab-case:

```bash
# WIT: save-report
curl -X POST localhost:8790/plugin/core-rust/call \
  -H 'content-type: application/json' \
  -d '{"function":"save-report","args":["v1.0.0","# report"]}'
```

`save_report` is not an alias. The same applies to `load-report`,
`next-version`, and any other hyphenated export.

## Permissions follow imports

| WIT import | Config |
|---|---|
| `poc:host/host-kv@0.1.0` | `kv = "<namespace>:read-write"` |
| `poc:host/host-log@0.1.0` | no permission needed |
| `wasi:filesystem/types` + `preopens` | `fs = ["reports:read-write"]` |
| `wasi:http/outgoing-handler` | `http_outbound = ["api.example.com"]` |
| `wasi:sockets/tcp` or `udp` | `network_outbound` / `network_inbound` |

If you add host KV to WIT but forget `kv` in config, `owasidea build` fails
intentionally. If you add a permission without the corresponding WIT import,
the manifest will advertise a capability the component never uses; remove it.

## UI scaffolds (AX-02)

`owasidea plugin new <name> --lang svelte|react|vue|yew|blazor|uno --type ui` generates a
webview UI plugin with `base: './'`, `ui_entry = "index.html"`, and a matching `ui_build`.

- Svelte/React/Vue: Vite-based, `ui_build = ["npm", "run", "build"]`.
- Yew: Trunk, `ui_build = ["trunk", "build", "--release"]` (requires Rust + trunk).
- Blazor: `ui_build = ["dotnet", "publish", "-c", "Release", "-o", "dist"]` (requires .NET).
- Uno: `ui_build = ["dotnet", "build"]` (requires .NET).

Use `window.__OWASIDEA_HOST__` (or `OWASIDEA_HOST`) instead of hardcoding host URLs.
