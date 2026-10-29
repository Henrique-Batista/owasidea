# Dependency consumption matrix — 1.2

Status: **validated** (2026-10-29, Linux, Wasmtime 50)

Provider WIT used by the disposable checks:

```wit
package val:provider@0.1.0;
interface api { double: func(value: u32) -> u32; }
```

| Consumer | Provider | Evidence |
| --- | --- | --- |
| Rust (`glue-rust`) | Go (`provider-go`) | `examples/interop` smoke: `call-provider("interop")` → `Go:interop` |
| Go (`consumer-go`) | Rust (`provider-rust`) | disposable app smoke: `run(21)` → `42` |
| TypeScript (jco) | Rust (`provider-rust`) | disposable app smoke: `run(21)` → `42` |
| C# (componentize-dotnet) | Rust (`provider-rust`) | disposable app smoke: `run(21)` → `42` |
| C (wasi-sdk + wit-bindgen) | Rust (`provider-rust`) | disposable app smoke: `run(21)` → `42` |
| C++ (wasi-sdk + wit-bindgen) | Rust (`provider-rust`) | disposable app smoke: `run(21)` → `42` |

Each disposable app declared the provider in
`[plugins.<consumer>].dependencies`, imported `val:provider/api@0.1.0` in its
world, built through `owasidea build`, and was exercised through the real host
`/plugin/<consumer>/call` plus `owasidea smoke --no-build`.

Notes:

- TypeScript consumers import the interface directly
  (`import { double } from 'val:provider/api@0.1.0'`); jco reports an
  `UNRESOLVED_IMPORT` warning because the module is a WIT component import, but
  emits the correct component import.
- C/C++ consumers use `wit-bindgen c` / `wit-bindgen cpp` over the `wit/`
  directory so `wit/deps/provider` resolves.
- C# consumers use the componentize-dotnet SDK and call
  `ConsumerCsharpWorld.wit.Imports.val.provider.v0_1_0.IApiImports.Double`.
- The disposable apps live under `/tmp/opencode` during validation and are not
  committed; `examples/interop` is the committed Rust→Go regression fixture.

The matrix proves consumer-side WIT imports for all six supported languages.
Provider-side support was already covered by Shipyard (Rust, Go, TypeScript, C#,
C, C++).
