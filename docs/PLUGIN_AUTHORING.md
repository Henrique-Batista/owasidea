# Plugin Authoring Guide

## Rust (cargo-component)

1. Create `plugins/<name>/Cargo.toml` with `[workspace]` and component metadata.
2. Add WIT under `wit/` and deps under `wit/deps/host`.
3. Implement `Guest` trait, `export!`.
4. Build: `cargo component build --release`.
5. Copy `.wasm` to `plugins/<name>/<name>.component.wasm`.
6. Run `owasidea build`; it generates the manifest from the real component WIT.

## C/C++ (wasi-sdk + wasm-tools + wit-bindgen)

1. Scaffold: `owasidea plugin new <name> --lang c|cpp`.
2. Annotate the source with `// owasidea:...` markers (see `docs/SOURCE_GENERATORS.md`).
   `owasidea wit generate` (and `owasidea build`) regenerates `wit/<name>.wit`.
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
