# Examples

Small, complete Owasidea apps. Each one is a real app directory: its own
`wasm-app.toml`, components, UI, smoke checks and Playwright tests. Use them as
starting points, or as fixtures when you change the framework.

| Example | Shows | Plugins | Tests |
| --- | --- | --- | --- |
| [`hello`](hello/README.md) | the smallest app: one Rust library, a plain TypeScript UI, and a contract generated from attributes | `greeter` (library) | 2 smoke calls, 3 UI tests |
| [`notes`](notes/README.md) | host key/value store and the `kv` capability | `notes` (library, `kv`) | 4 smoke calls, 4 UI tests |
| [`ticker`](ticker/README.md) | stateful service lifecycle and the 409 reload rule | `ticker` (service) | service smoke, 4 UI tests |
| [`interop`](interop/README.md) | Rust glue calls a Go provider through generated WIT bindings; two KV namespaces and a declared dependency cycle | `glue-rust`, `provider-go` | 4 smoke calls |
| [`shipyard`](shipyard/README.md) | the reference app: 12 plugins in 6 languages, glue UI, Tauri packaging | see README | the full suite |

## Run an example

```bash
cargo build -p owasidea-cli           # once
cd examples/hello
owasidea build
owasidea smoke --no-build
owasidea test --no-build
owasidea dev                          # interactive dashboard: press ? for keys
```

`owasidea dev` opens the dashboard in a terminal. Without a terminal (CI, pipes)
it prints the plain log instead; `--no-tui` forces that.

## Run every example

```bash
scripts/test-examples.sh              # build + smoke + Playwright for hello, notes, ticker
scripts/test-examples.sh notes        # one example
```

Each example uses its own host port (8801, 8802, 8803, 8790 for shipyard) and UI
port (5201, 5202, 5203, 5180), so they can run side by side.

## Requirements

- Rust with `cargo-component` and `wasm-tools` (`owasidea doctor` checks them).
- Node.js and npm (UI build and Playwright).
- Playwright browsers: `npx playwright install chromium`.

Shipyard also needs Go with `componentize-go`, .NET, and wasi-sdk for its C/C++
plugins. `interop` needs Go with `componentize-go`; `hello`, `notes` and `ticker`
need only the Rust toolchain.
