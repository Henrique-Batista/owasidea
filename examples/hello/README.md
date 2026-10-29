# hello

The smallest complete Owasidea app: one Rust library component and a plain
TypeScript UI that calls it through the host HTTP API.

```text
hello/
├── wasm-app.toml          app config, smoke checks, UI test command
├── plugins/greeter/       Rust component; contract generated from #[owasidea::export]
├── ui/                    Vite + TypeScript UI (no framework)
└── tests/hello.spec.ts    Playwright UI tests
```

## Run it

```bash
cd examples/hello
owasidea build      # generate .owasidea/wit, compile, derive the manifest, build the UI
owasidea smoke      # temporary host + declarative API checks from wasm-app.toml
owasidea test       # build + smoke + Playwright UI tests
owasidea dev        # host + UI + watcher in the interactive dashboard (press ? for keys)
```

Requirements: `rustc`, `cargo-component`, `wasm-tools`, `node`/`npm`
(`owasidea doctor` reports what is missing). For the Playwright UI test, run
`npm install` once at the repository root (or use `owasidea test --no-ui`).

## Test points

| Layer | What is checked | Where |
| --- | --- | --- |
| Contract | `#[owasidea::export]` markers render `.owasidea/wit/greeter.wit` (no WIT file in the repo) | `plugins/greeter/src/lib.rs` |
| Manifest | exports `greet(string)` and `sum(s32, s32)` are derived from the real component | `manifests/greeter.manifest.json` |
| Host API | `greet` returns the exact string; `sum` returns `42` | `[[smoke.calls]]` |
| UI | greeting round-trip, numeric round-trip, error state on HTTP 404 | `tests/hello.spec.ts` |
| Dev loop | editing `plugins/greeter/src` and running `R` in `owasidea dev` rebuilds and reloads | manual, see `docs/CLI.md` |

## Change it

1. Add `#[owasidea::export]` to a function in `plugins/greeter/src/lib.rs`.
2. Run `owasidea build`; `.owasidea/wit/greeter.wit` is regenerated and the
   manifest follows the real component. Do not edit the generated WIT.
3. Add a `[[smoke.calls]]` entry in `wasm-app.toml`.
4. Run `owasidea test`.
