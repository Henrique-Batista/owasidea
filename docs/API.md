# HTTP API Reference

Base URL: `http://127.0.0.1:8787` (configurable). CORS permissive. All JSON.

## Health

- `GET /health` → `"ok"`

## Plugins

- `GET /plugins` → list of plugin summaries:
  ```json
  [{"name":"notes","version":"0.1.0","language":"rust","type":"library",
    "exports":["greet","complete"],"imports_wasi":10,"imports_host":["..."],"served_on":null}]
  ```

- `GET /plugin/{name}/manifest` → full generated manifest JSON.

- `GET /plugin/{name}/logs` → `{ "logs": ["..."] }`.
- `GET /plugin/{name}/logs/stream` → SSE stream of log lines.

- `POST /plugin/{name}/reload` → re-reads manifest from disk and re-instantiates
  the component; returns `{ "reloaded": "name" }` or 4xx/5xx on validation failure.

## Library calls (notes)

- `POST /plugin/{name}/greet` body `{ "name": "POC" }` → `{ "output": "Hello, POC! ..." }`.
- `POST /plugin/{name}/complete` body `{ "prompt": "hi" }` → `{ "output": "..." }` or 400 with error.

## Executable plugins (reverse proxy)

- `* /app/{name}` and `/app/{name}/{*path}` → proxied to the plugin's wasmtime serve instance, preserving method/headers/query/body.

## Service plugins (tailcat-wasi, optional)

- `POST /plugin/{name}/start-service` body `{ "region_json": "" }` → `{ "output": "<address>" }`.
- `POST /plugin/{name}/stop-service` → `{ "running": false, "address": null }`.
- `GET /plugin/{name}/service-status` → `{ "running": bool, "address": string|null }`.
- `POST /plugin/{name}/send-text` body `{ "addr": "...", "text": "..." }` → `{ "output": "queued" }`.
- `GET /plugin/{name}/drain` → `{ "messages": [...], "events": [...] }`.

## Errors

Non-2xx responses return plain text or JSON error message. 404 for unknown plugin.