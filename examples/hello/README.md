# hello

The smallest complete Owasidea app: one Rust library component and a plain
TypeScript UI that calls it through the host HTTP API.

```text
hello/
├── wasm-app.toml          app config, smoke checks, UI test command
├── plugins/greeter/       Rust component (WIT contract in wit/greeter.wit)
├── ui/                    Vite + TypeScript UI (no framework)
└── tests/hello.spec.ts    Playwright UI tests
```

## Run it

```bash
cd examples/hello
owasidea build      # compile the component, regenerate manifests from WIT, build the UI
owasidea smoke      # temporary host + declarative API checks from wasm-app.toml
owasidea test       # build + smoke + Playwright UI tests
owasidea dev        # host + UI + watcher in the interactive dashboard (press ? for keys)
```

Requirements: `rustc`, `cargo-component`, `wasm-tools`, `node`/`npm`
(`owasidea doctor` reports what is missing).

## Test points

| Layer | What is checked | Where |
| --- | --- | --- |
| Manifest | exports `greet(string)` and `sum(s32, s32)` are derived from WIT | `manifests/greeter.manifest.json` |
| Host API | `greet` returns the exact string; `sum` returns `42` | `[[smoke.calls]]` |
| UI | greeting round-trip, numeric round-trip, error state on HTTP 404 | `tests/hello.spec.ts` |
| Dev loop | editing `plugins/greeter/src` and running `R` in `owasidea dev` rebuilds and reloads | manual, see `docs/CLI.md` |

## Change it

1. Edit `plugins/greeter/wit/greeter.wit` to add or change an export.
2. Implement it in `plugins/greeter/src/lib.rs`.
3. Add a `[[smoke.calls]]` entry in `wasm-app.toml`.
4. Run `owasidea test`.
