# ticker

A stateful service component with a UI that starts, stops and reloads it.
Use it to test the service lifecycle and the rule that a running service cannot
be reloaded.

```text
ticker/
├── wasm-app.toml                 type = "service", smoke service check, UI test command
├── plugins/ticker/               Rust service (start-service, service-status)
└── ui/                           Vite + TypeScript UI with start/stop/reload buttons
```

## Run it

```bash
owasidea build
owasidea smoke --no-build      # start -> status running -> stop
owasidea test --no-build       # Playwright lifecycle tests
owasidea dev                   # `s` starts/stops the service from the dashboard
```

## Lifecycle

| Action | Endpoint | Result |
| --- | --- | --- |
| start | `POST /plugin/ticker/service/start` | `running`; a second start is a no-op |
| status | `GET /plugin/ticker/service/status` | `{"running": bool, ...}` |
| stop | `POST /plugin/ticker/service/stop` | drops the instance; state resets |
| reload | `POST /plugin/ticker/reload` | HTTP 409 while running, 200 after stop |

## Test points

- Start and stop move the status shown in the UI.
- Repeating start keeps the service running.
- Reload while running is refused with HTTP 409 (`docs/CLI.md`, `dev`).
- Reload after stop succeeds.

Playwright UI tests need `npm install` once at the repository root; use
`owasidea test --no-ui` to skip them.
