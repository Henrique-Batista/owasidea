# notes

A library component that stores notes in the host key/value store. Shows the
`kv` capability and state that outlives a page reload (and a hot reload).

```text
notes/
├── wasm-app.toml                 kv = ["notes:read-write"], smoke checks, UI test command
├── plugins/notes/                Rust component; WIT imports poc:host/host-kv
└── ui/                           Vite + TypeScript UI
```

## Run it

```bash
owasidea build
owasidea smoke --no-build
owasidea test --no-build       # Playwright: add, ignore blanks, reload, clear
owasidea dev                   # dashboard; `r` reloads the notes component
```

## Component API

| Export | Signature | Notes |
| --- | --- | --- |
| `add-note` | `(text: string) -> u32` | returns the new count; blank text is ignored |
| `list-notes` | `() -> string` | notes joined with `\n` |
| `clear-notes` | `() -> u32` | returns 0 |

## Test points

- **Capability:** the build fails when `kv` is removed from `wasm-app.toml`
  while the component still imports `host-kv`:
  `plugin notes: host-kv WIT import and configured kv permission must both be present`.
- **Persistence:** the UI test reloads the page and the note is still there.
  State lives in the host store (`OWASIDEA_DATA_DIR`), not in the browser.
- **Contract:** smoke checks call `add-note`/`list-notes` by their WIT names.
  Underscore aliases do not exist.

Playwright UI tests need `npm install` once at the repository root; use
`owasidea test --no-ui` to skip them.
