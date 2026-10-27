# notes

Rust library component that stores notes in the host key/value store.

- `add-note(text) -> u32`: appends a note and returns the new count (blank text is ignored).
- `list-notes() -> string`: notes joined with `\n`.
- `clear-notes() -> u32`: removes every note.

Permission: `kv = "notes:read-write"` (declared in `wasm-app.toml`).
