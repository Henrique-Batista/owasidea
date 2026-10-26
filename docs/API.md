# HTTP API Reference

Base URL: `http://127.0.0.1:8787` (configurable). CORS is restricted to the
Tauri app origin and local Vite origins by default. All JSON unless noted.

## How a webview frontend calls a plugin

There is no generated RPC client and no per-function route. The mapping comes
from the generated manifest:

| Manifest | HTTP route | Payload |
| --- | --- | --- |
| `name` | `/plugin/{name}/...` | path segment, URL-encoded |
| `exports[].name` | `POST /plugin/{name}/call` | `{ "function": "<export>", "args": [ ... ] }` |
| `type = "executable"` | `/app/{name}/{*path}` | raw `wasi:http` request |
| `type = "service"` | `/plugin/{name}/service/{start,stop,status}` | lifecycle only |
| `type = "ui"`, `runtime = "js-wasm"` | none | static assets in a sandboxed iframe |

`args` is positional and matches the WIT signature in `exports[].signature`
(for example `func(version: string, changes: list<string>) -> string`).

Each export also carries structured type information: `exports[].params` is an array of
`{ "name", "type" }` (in WIT parameter order) and `exports[].results` is an array of
`{ "name", "type" }` (named results preserve their WIT name; a single unnamed result
uses `name: "result"`). Supported type spellings: primitives (`bool`, `u8`..`u64`,
`s8`..`s64`, `f32`, `f64`, `char`, `string`), `list<T>`, `option<T>`, `result<T,E>`
(`_` for a void side, bare `result` when both are void), `tuple<...>`, `record <name>`,
`enum <name>`, `flags <name>`, `variant <name>`, `resource`, and `handle`.
The host converts each JSON value to the WIT type and rejects arity/type
mismatches with HTTP 400.


### Type grammar (CT-02)

```
type := primitive
     | "list<" type ">"
     | "option<" type ">"
     | "result"
     | "result<" side "," side ">"
     | "tuple<" type ("," type)+ ">"
     | "record" (" " | "<") name (">")?
     | "enum" (" " | "<") name (">")?
     | "flags" (" " | "<") name (">")?
     | "variant" (" " | "<") name (">")?
     | "resource"
     | "handle"
side := "_" | type
primitive := bool | u8 | u16 | u32 | u64 | s8 | s16 | s32 | s64 | f32 | f64 | char | string | void
```

Examples: `u32`, `list<string>`, `option<u32>`, `result<string,string>`,
`result<_,string>`, `result<u32,_>`, `result`, `tuple<u32,string>`,
`record point`, `enum color`, `flags perms`, `variant shape`, `resource`, `handle`.
Invalid spellings rejected by `owasidea build` with a path such as
`plugin notes: invalid type at exports[0].params[1].type: ...`.
 The response is `{ "result": ... }` for a single
return value or `{ "results": [...] }` for multiple values; `result`/`results`
wrappers such as `ok`/`err` are preserved.

Discovery for a frontend:

1. `GET /plugins` for names, types, runtimes, and export names.
2. `GET /plugin/{name}/manifest` for full signatures, permissions, and imports.
3. `POST /plugin/{name}/call` for library calls, or call `/app/{name}` for an
   executable plugin.

Isolated UI plugins (`js-wasm`) receive no host bridge. They run in an iframe
with `sandbox="allow-scripts allow-forms"` (opaque origin), so a direct
`fetch` to the host fails CORS by design. If a UI plugin needs backend data,
the dashboard must proxy it through a validated `postMessage` bridge with an
explicit per-plugin allowlist; do not add `Origin: null` to CORS.

## Health

- `GET /health` → `"ok"`
- `GET /overview` → plugin counts and runtime kinds.
- `GET /dev/status` → dev-mode flag, watched paths, and most recent reload
  result by plugin (for example `{"active":true,"hot_reload":true,...}`).

## Plugins

- `GET /plugins` → list of plugin summaries:
  ```json
  [{"name":"notes","version":"0.1.0","language":"rust","type":"library",
    "exports":["greet","complete"],"imports_wasi":10,"imports_host":["..."],"served_on":null}]
  ```
- `GET /ui-plugins` → webview-only `js-wasm` bundles, entry paths, and asset
  digests. These plugins are not instantiated by Wasmtime.

- `GET /plugin/{name}/manifest` → full generated manifest JSON.

- `GET /plugin/{name}/logs` → `{ "logs": ["..."] }`.
- `GET /plugin/{name}/logs/stream` → SSE stream of log lines.
- `POST /plugin/{name}/service/start` → generic service start.
- `POST /plugin/{name}/service/stop` → generic service stop.
- `GET /plugin/{name}/service/status` → generic service status.

Generic services keep one component instance between start/status/stop. Start is
idempotent; stop drops the instance. Hot reload returns `409` while service is
running; stop it, reload, then start again.

- `POST /plugin/{name}/reload` → re-reads manifest from disk and re-instantiates
  the component; returns `{ "reloaded": "name" }` or 4xx/5xx on validation failure.

## Library calls (notes)

- `POST /plugin/{name}/greet` body `{ "name": "POC" }` → `{ "output": "Hello, POC! ..." }`.
- `POST /plugin/{name}/complete` body `{ "prompt": "hi" }` → `{ "output": "..." }` or 400 with error.
- `POST /plugin/{name}/call` body `{ "function": "greet", "args": ["name"] }` →
  `{ "result": ... }` (generic dispatcher based on WIT types). Function names
  are exact WIT names; kebab-case stays kebab-case (`next-version`,
  `save-report`).

## Executable plugins (embedded WASI HTTP)

- `* /app/{name}` and `/app/{name}/{*path}` → dispatched to the component's
  `wasi:http/incoming-handler` in the shared host; preserves method, headers,
  query, request body, and streaming response body. No `wasmtime serve` child is
  launched.

## Service plugins (tailcat-wasi, optional)

- `POST /plugin/{name}/start-service` body `{ "region_json": "" }` → `{ "output": "<address>" }`.
- `POST /plugin/{name}/stop-service` → `{ "running": false, "address": null }`.
- `GET /plugin/{name}/service-status` → `{ "running": bool, "address": string|null }`.
- `POST /plugin/{name}/send-text` body `{ "addr": "...", "text": "..." }` → `{ "output": "queued" }`.
- `GET /plugin/{name}/drain` → `{ "messages": [...], "events": [...] }`.

## Errors

Non-2xx responses return plain text or JSON error message. 404 for unknown plugin.
