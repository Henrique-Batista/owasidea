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
- `GET /overview` → plugin counts, runtime kinds, and `build_id` (string or null).
- `GET /dev/status` → dev-mode flag, watched paths, and most recent reload
  result by plugin (for example `{"active":true,"hot_reload":true,...}`).

## Metrics

`GET /metrics` returns the Prometheus text exposition format. Global counters
are always present:

```
# TYPE owasidea_requests_total counter
owasidea_requests_total 42
# TYPE owasidea_errors_total counter
owasidea_errors_total 3
```

Per-plugin counters carry a `plugin` label and are emitted only for plugins
currently loaded by the host:

```
# TYPE owasidea_plugin_requests_total counter
owasidea_plugin_requests_total{plugin="notes"} 5
# TYPE owasidea_plugin_errors_total counter
owasidea_plugin_errors_total{plugin="notes"} 1
# TYPE owasidea_plugin_latency_ms_sum counter
owasidea_plugin_latency_ms_sum{plugin="notes"} 87
# TYPE owasidea_plugin_latency_ms_count counter
owasidea_plugin_latency_ms_count{plugin="notes"} 5
# TYPE owasidea_plugin_fuel_total counter
owasidea_plugin_fuel_total{plugin="notes"} 24510
```

- Instrumented paths: `POST /plugin/{name}/call`, `POST /plugin/{name}/service/start`,
  `GET /plugin/{name}/service/status`, `POST /plugin/{name}/service/stop`,
  `POST /plugin/{name}/service/restart`, and `* /app/{name}` (executable
  `wasi:http` plugins).
- `requests` counts every instrumented operation, including failed ones;
  `errors` counts non-2xx responses, dispatcher errors, and guest failures.
- `latency_ms_sum` / `latency_ms_count` are a counter pair, so average latency is
  `sum / count` over any window in Prometheus.
- `fuel_total` is Wasmtime fuel consumed, measured as
  `resources.max_fuel - store fuel remaining`. It is `0` for paths that do not
  execute guest code (service stop) and for executable plugins proxied through
  `wasi:http` (the store is owned by the spawned guest task).
- Label values are escaped (`\`, `"`, newline) and families are sorted by plugin
  name for stable scrapes.

## Tracing (OpenTelemetry, RL-05)

OTLP trace export is optional and off by default. Enable it at build time and
configure an endpoint at run time:

```bash
cargo build -p owasidea-host --features otel
export OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318   # OTLP/HTTP
# optional: traces-specific override, wins over the endpoint above
export OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://127.0.0.1:4318/v1/traces
```

- Without the `otel` feature, or with the feature compiled in but the endpoint
  unset (or blank), the host installs the same `tracing_subscriber::fmt`
  subscriber as before, creates no spans, starts no exporter, and logs exactly
  as it does today.
- Transport is OTLP over HTTP/protobuf (`http://host:4318/v1/traces` by
  default). A gRPC collector can be fronted by an OTLP/HTTP receiver.
- Spans:
  - `http.request` — one per inbound host HTTP request, with
    `http.request.method`, `url.path`, `http.response.status_code` and
    `otel.status_code`.
  - `plugin.call` — one per plugin invocation, with `plugin`, `function`,
    `call.kind` (`library` for `/call`, `/greet`, `/complete`; `service` for
    `start-service`, `stop-service`, `restart-service`; `executable` for
    `/app/{name}`), `status` (`ok`/`error`) and `otel.status_code`.
- Context propagation: the host reads the W3C `traceparent` (and `tracestate`)
  request header, so a caller's trace id becomes the parent of the exported
  spans. A malformed header is ignored; the request is never rejected because of
  it.
- `service.name` is `owasidea-host`. Pending spans are flushed on `SIGINT`
  (`Ctrl+C`) shutdown; a collector that is unreachable only produces exporter
  warnings and never fails requests.
- Failures to build the exporter fall back to plain logging with a warning.

## Plugins

- `GET /plugins` → list of plugin summaries:
  ```json
  [{"name":"notes","version":"0.1.0","language":"rust","type":"library",
    "exports":["greet","complete"],"imports_wasi":10,"imports_host":["..."],"served_on":null}]
  ```
- `GET /ui-plugins` → webview-only `js-wasm` bundles, entry paths, and asset
  digests. These plugins are not instantiated by Wasmtime.

- `GET /plugin/{name}/manifest` → full generated manifest JSON.

- `GET /plugin/{name}/logs` → `{ "logs": ["..."], "entries": [ { "ts": 0, "level": "info", "plugin": "name", "request_id": 0, "message": "...", "fields": null } ] }` (supports `?level=`).
- `GET /plugin/{name}/logs/stream` → SSE stream of structured JSON logs (`event: log`, `data: {...}`), supports `?level=`.
- `POST /plugin/{name}/service/start` → generic service start.
- `POST /plugin/{name}/service/stop` → generic service stop.
- `GET /plugin/{name}/service/status` → generic service status.
- `POST /plugin/{name}/service/restart` → drop the live instance, then start a
  fresh one; returns the same body as `service/status`. Accepts an optional
  `{ "region_json": "" }` body.

Generic services keep one component instance between start/status/stop. Start is
idempotent; stop drops the instance. Restart drops the instance first, so guest
state and sockets are recreated; it returns `409` when the plugin is not
`service`/`hybrid` or when a concurrent start made the restart inconsistent, and
`404` for an unknown plugin. Hot reload returns `409` while service is running;
stop it, reload, then start or restart again.

Status response:

```json
{ "running": true, "address": "127.0.0.1:9000", "last_error": null, "restarts": 1 }
```

- `address` is populated when the start export returns a bare `host:port`.
- `last_error` keeps the reason the last start attempt failed (guest error,
  timeout, missing `start-service` export); a successful start clears it, so it
  also explains why a stopped service did not come up.
- `restarts` counts explicit restarts that replaced a live instance. It survives
  stop/start and is not reset by the host.

- `POST /plugin/{name}/reload` → re-reads manifest from disk and re-instantiates
  the component; returns `{ "reloaded": "name" }` or 4xx/5xx on validation failure.

## Library calls (notes)

- `POST /plugin/{name}/greet` body `{ "name": "POC" }` → `{ "output": "Hello, POC! ..." }`.
- `POST /plugin/{name}/complete` body `{ "prompt": "hi" }` → `{ "output": "..." }` or 400 with error.
- `POST /plugin/{name}/call` body `{ "function": "greet", "args": ["name"] }` →
  `{ "result": ... }` (generic dispatcher based on WIT types). Function names
  are exact WIT names; kebab-case stays kebab-case (`next-version`,
  `save-report`).

## Streaming jobs (RL-06, experimental)

Long-call jobs are compiled in only with the `streaming` feature:

```bash
cargo build -p owasidea-host --features streaming
```

Without the feature the routes below do not exist (`404`) and
`POST /plugin/{name}/call` is unchanged. See ADR-0005 for the design and
limitations.

| Route | Purpose |
| --- | --- |
| `POST /plugin/{name}/call/jobs` | create a job for a `library`/`hybrid` export |
| `GET /jobs/{id}` | job status, result/error, and buffered events |
| `GET /jobs/{id}/events` | SSE stream (`event: job`, then `event: end`) |
| `POST /jobs/{id}/cancel` | cancel a job (idempotent) |

Create:

```json
{ "function": "slow-sum", "args": [500000000] }
```

`202 Accepted`:

```json
{
  "job_id": "9f0c…",
  "state": "queued",
  "status_url": "/jobs/9f0c…",
  "events_url": "/jobs/9f0c…/events",
  "cancel_url": "/jobs/9f0c…/cancel"
}
```

States are `queued`, `running`, `succeeded`, `failed`, and `cancelled`.
`GET /jobs/{id}` returns the job with `events` (a bounded snapshot) and, when
terminal, `result` or `error`. `args` use the same positional JSON/WIT type
rules as `/call`; an unknown export becomes a failed job (the component is only
instantiated when the job runs).

SSE events carry a monotonic `seq` in the `id` field:

```
event: job
id: 3
data: {"seq":3,"ts":1793000000512,"event":"progress","data":{"elapsed_ms":500}}

event: end
data: {"state":"succeeded"}
```

`event: end` is always last and closes the stream. A client that connects after
the job finished receives the buffered events and the `end` immediately. Send
`Last-Event-ID: <seq>` to resume after a known event; if the ring already
dropped events, the stream emits `event: dropped` with
`{"requested_after":N,"first_available":M}` before the replay. A result larger
than 256 KiB is returned as `{"truncated":true,"original_bytes":N}`, and the
`finished` event carries `"truncated":true`. Every `/jobs/*` response sets
`Cache-Control: no-store`.

Cancel returns `200` with the current job. Cancelling a terminal job does not
change it. A job cancelled while still `queued` never emits `started` and ends
`cancelled`. The prototype drops the executor future; it cannot preempt a guest
that never yields. An executor panic fails the job (`job panicked`) and releases
its slot.

Limits (per host process):

- 8 active jobs (`429` beyond that);
- 64 retained terminal jobs;
- 256 events per job (oldest dropped first);
- 64 KiB serialized `args` (`413`);
- 256-byte function name (`400`);
- 256 KiB serialized result (larger becomes truncated metadata);
- 8 KiB per event `data` (larger payloads become `{"truncated":true}`);
- 4 SSE readers per job and 64 globally (`429` beyond that);
- timeout `clamp(manifest permissions.resources.timeout_ms, 1, 300000)` ms,
  default 30 s; a timeout marks the job `failed`, it is not a `504`;
- `progress` heartbeat every 500 ms while the call runs.

Job ids are 128-bit random values. If the RNG fails, creation returns `500`
instead of falling back to a predictable id. Job ids are not written to logs;
HTTP traces redact `/jobs/<id>` to `/jobs/{id}`.

The host refuses to bind a non-loopback address unless
`--allow-non-loopback` or `OWASIDEA_ALLOW_NON_LOOPBACK=1` is set.

Jobs share the plugin's `instance_slots` (`permissions.resources.max_instances`,
default 1), so a long job serializes other calls to the same plugin unless the
manifest allows more instances.

Example (`plugins/example`):

```bash
curl -s -X POST localhost:8787/plugin/example/call/jobs \
  -H 'content-type: application/json' \
  -d '{"function":"slow-sum","args":[500000000]}'
curl -N localhost:8787/jobs/<job_id>/events
```

Job calls are counted in the plugin's `/metrics` counters. There is no job queue
metric or persistence yet; jobs and events are lost when the host restarts.
Guest-driven progress through a WIT `progress` import is specified in ADR-0005
but not implemented in this prototype.

## Executable plugins (embedded WASI HTTP)

- `* /app/{name}` and `/app/{name}/{*path}` → dispatched to the component's
  `wasi:http/incoming-handler` in the shared host; preserves method, headers,
  query, request body, and streaming response body. No `wasmtime serve` child is
  launched.
- Guest response headers are filtered. Only `content-type`, `content-length`,
  `cache-control`, `etag`, `last-modified` and `expires` are forwarded; every
  other header (`set-cookie`, `access-control-*`, framing, `content-encoding`,
  …) is dropped because `/app/{name}` is served on the host origin.
- The host always adds `X-Content-Type-Options: nosniff` and
  `Content-Security-Policy: sandbox`, and rewrites `text/html`,
  `application/xhtml+xml`, JavaScript, CSS, SVG and unknown types to
  `text/plain; charset=utf-8`. Inert types (`text/plain`, `application/json`,
  `text/csv`, `application/octet-stream`, `image/png|jpeg|gif|webp`) pass
  through.

## Typed POC routes (optional)

`POST /plugin/{name}/dial-tcp` `{host, port}`, `/start-server` `{region_json}`,
`/bind-udp` `{port}`, `/parse-address` `{addr}`, `/tls-probe`
`{host, port, server_name}`, `/dial-echo` `{host, port}`, `/echo-udp` `{port}`.
Each call runs under the plugin's `permissions.resources.timeout_ms` (default
10 s) and answers `504` when the export does not return; the `max_instances`
slot is released on timeout. These routes exist for the sockets/network POC and
are not part of the stable plugin ABI.

## Service plugins (tailcat-wasi, optional)

The hardcoded tailcat routes are **not** exposed by default. They are registered
only when the `tailcat-wasi` plugin is loaded or `OWASIDEA_ENABLE_TAILCAT=1` is
set; otherwise they return `404` like any unknown route.

- `POST /plugin/tailcat-wasi/start-service` body `{ "region_json": "" }` → `{ "output": "<address>" }`.
- `POST /plugin/tailcat-wasi/stop-service` → `{ "running": false, "address": null }`.
- `GET /plugin/tailcat-wasi/service-status` → `{ "running": bool, "address": string|null }`.
- `POST /plugin/tailcat-wasi/send-text` body `{ "addr": "...", "text": "..." }` → `{ "output": "queued" }`.
- `GET /plugin/tailcat-wasi/drain` → `{ "messages": [...], "events": [...] }`.

## Errors

Non-2xx responses return plain text or JSON error message. 404 for unknown plugin.


## Typed client (CT-03)

Generate with `owasidea client --lang ts -o src/client.ts`. Usage:

```ts
import { createClient } from "./client";
const client = createClient({ baseUrl: "http://127.0.0.1:8787", timeoutMs: 30000 });
const out = await client.coreRust.ping();
```

Calls use `POST /plugin/{name}/call` with `{ function, args }`. Failures throw
`CallError` with `status`, `body`, `plugin`, `function`.

## Manifest ABI fields (CT-08)

Manifests include `schema_version`, `host_abi`, and `min_host`. The host rejects
incompatible manifests at load time; see `docs/ABI.md`.

Legacy manifests (no `schema_version`) are still accepted under host ABI 1 and
are deprecated: `owasidea validate` warns (`ABI_LEGACY_MANIFEST`,
`ABI_MISSING_HOST_FIELDS`) without failing. Regenerate with `owasidea build`.
Policy: `docs/ABI_POLICY.md`; 0.x upgrade: `docs/MIGRATION_0_X_TO_1_0.md`.
