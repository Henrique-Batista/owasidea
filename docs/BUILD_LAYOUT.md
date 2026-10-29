# Build layout — 1.3

`owasidea build` keeps generated contracts, resolved dependency packages and
final components under one directory at the app root:

```text
.owasidea/
  deps/
    providers.json                       provider -> packages index
    poc/test/0.1.0/test.wit              canonical provider packages
  build/
    test/test.component.wasm             final component
    test/wit/                            contract + resolved deps
  cache/
    cargo/test/                          CARGO_TARGET_DIR
    dotnet/test/                         dotnet --artifacts-path
  runtime-manifests/                     bundled manifests (rewritten paths)
```

`.owasidea/` is generated, ignored by git (`**/.owasidea/`) and safe to delete.

## Dependency resolution

A consumer declares runtime dependencies in `wasm-app.toml`:

```toml
[plugins.app-glue]
dependencies = ["catalog-go"]
```

Build order:

1. Every plugin contract is resolved (`wit_source`) before anything compiles.
2. Each plugin's own WIT package is published to `.owasidea/deps/`.
3. Each consumer gets `.owasidea/build/<consumer>/wit/`: its own contract plus
   the packages of its declared dependencies. Hand-copied provider packages are
   replaced by the canonical copy in the view.
4. The toolchain reads that view. No `wit/deps/<provider>` copy is needed in
   the plugin directory.

`dependencies` is runtime dispatch: the provider wasm is not copied into the
consumer. Only the WIT package (a few KB) is shared. `compose_with` is the
build-time alternative and still embeds the provider wasm.

## Toolchain matrix

| Language | WIT source used | Bindings/intermediates |
| --- | --- | --- |
| Go | `componentize-go -d .owasidea/build/<p>/wit` | Go module (`export_wit_world/`) |
| C# | `dotnet build -p:OwasideaWitDir=<view> --artifacts-path .owasidea/cache/dotnet/<p>` | `.owasidea/cache/dotnet/<p>/obj/` |
| C / C++ | `make WIT_DIR=<view> OUT=.owasidea/build/<p>/<p>.component.wasm` | `src/` or `gen/` from the Makefile |
| TypeScript | `OWASIDEA_WIT_DIR=<view> OWASIDEA_OUT=<component> npm run build` | `node_modules/` |
| Rust | mirrors resolved packages into the contract directory named by `Cargo.toml` | `src/bindings.rs`; cargo cache in `.owasidea/cache/cargo/<p>` |

Rust resolves `deps/` next to the WIT path compiled into the crate, and
`cargo component build` has no flag to override it. The CLI therefore merges
only the packages the contract imports into that directory and never replaces
an existing package. This is the one toolchain that still needs a generated
`deps/` beside the source contract. Keep it ignored.

C#, C/C++ and TypeScript templates ship the `OwasideaWitDir` / `WIT_DIR` /
`OWASIDEA_WIT_DIR` hooks. Older projects keep working: the CLI mirrors the
packages into their existing `wit/deps` and still centralizes the final
component.

## Bundles

The app keeps `manifests/*.json` pointing at `.owasidea/build/...`. Before
staging a bundle, the CLI writes `runtime-manifests/` with each `component`
rewritten to `../plugins/<name>/<name>.component.wasm`, matching the staged
`plugins/` payload. `owasidea verify dist/` therefore reads the same manifests
the host will read.

## Migration from 1.2

- Delete hand-copied provider packages from `plugins/<consumer>/wit/deps/`;
  the build resolves them from `.owasidea/deps/`.
- C#: add the `OwasideaWitDir` property and
  `<Wit Include="$(OwasideaWitDir)" .../>` to the `.csproj` (new scaffolds
  already do).
- C/C++: use `WIT_DIR` and `OUT` in the Makefile (optional; the fallback still
  works).
- TypeScript: use the scaffolded `scripts/build.mjs` (optional; the fallback
  still works).
- Scripts that read `plugins/<name>/<name>.component.wasm` must read
  `.owasidea/build/<name>/<name>.component.wasm` or the manifest `component`.

## Limits

- Bindings stay where each toolchain expects them: Rust `src/bindings.rs`, Go
  packages, C/C++ `src`/`gen`, TypeScript `node_modules`.
- `node_modules/`, Go's module cache and Cargo's registry cache are not moved.
- Rust projects with hand-written `wit/deps` keep those files; the CLI does not
  rewrite existing packages.
- `owasidea clean` does not exist; remove `.owasidea/` to force a cold build.
