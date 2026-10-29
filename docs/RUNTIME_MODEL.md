# Runtime model for plugin authors

This is the short answer to “what can a plugin do, and when is the sandbox
applied?”. It describes the current implementation, not a future design.

## 1. When the sandbox runs

| Phase | What happens |
| --- | --- |
| Build (`owasidea build`) | **No sandbox.** `cargo`, `go`, `npm`, `wasm-tools compose` run as your user. The build only compiles, composes, derives WIT/imports and writes the manifest. |
| Load (`owasidea run/dev/smoke/test`) | Manifest schema/ABI, duplicate names, checksums, declared vs actual imports, HTTP permission shape and composed-plugin resolution are checked. Every component is instantiated once as a smoke test. |
| Per call | A fresh Wasmtime `Store` is created for library calls and `/app` requests; services keep one store until stop. Fuel, memory, `max_instances` and the host timeout apply here. |
| Per operation | HTTP hosts, socket addresses, filesystem preopens and KV access are checked when the guest uses them. |

The component bytes are compiled once and cached. The manifest is not re-read
per call; use `/plugin/{name}/reload` (or restart the host) after a rebuild.

## 2. Plugin types and routes

| Type | Runtime | Entry point |
| --- | --- | --- |
| `library` | `wasi-component` | `POST /plugin/{name}/call` |
| `service` | `wasi-component` | `POST /plugin/{name}/service/start|stop|restart`, `GET .../service/status` |
| `executable` | `wasi-component` + `wasi:http` | `* /app/{name}` and `/app/{name}/{path}` |
| `ui` | `js-wasm` (webview only) | static bundle in a sandboxed iframe |

There is **no global `/call` route**. The generic route is
`/plugin/{name}/call` with `{ "function": "kebab-name", "args": [...] }`.
Top-level exports are resolved by name; interface exports are resolved only
through `manifest.exports`. HTTP exposure is opt-in per plugin:

```toml
[plugins.notes]
expose = true
```

The default is `false`: `/plugin/{name}/*` and `/app/{name}` return `403`
unless the plugin sets `expose = true`. There is no per-export ACL: once a
plugin is exposed, every export in its manifest is reachable. The dashboard
only renders forms for manifest exports, which is a UI convenience, not
authorization. The host API has no authentication; keep it on loopback.

`/app/{name}` is only registered for executable/hybrid plugins with
`wasi:http/incoming-handler`. Guest response headers are filtered, and HTML/JS/
CSS/SVG are served as `text/plain` because the route shares the host origin.

## 3. UI → plugin

Apps set one glue plugin:

```toml
[app]
ui_plugin = "app-glue"
```

The shipped app UI calls that plugin through the host API. The glue is always
reachable without setting `expose = true`. In production, other plugins are
rejected (`403`) unless they opt in with `expose = true`; `GET /plugins` lists
only the glue and exposed plugins. Service lifecycle routes follow the same
rule; the glue normally uses the typed `service-control` WIT import instead.
`owasidea dev`, the smoke/test harness and `OWASIDEA_DEV_MODE=1` keep the
developer surface for every plugin; `owasidea tui` only sees them when attached
to a dev host. This is an API surface rule, not authentication against other
processes owned by the same OS user; keep the host loopback-only.

A UI plugin in an iframe is still **not** allowed to call the host directly. It
uses the opt-in parent bridge:

```toml
[plugins.my-board]
type = "ui"
runtime = "js-wasm"
bridge = ["core-rust.ping"]
```

The bridge is parent-mediated, nonce-checked, allowlisted, timeout/cancelled.

## 4. Plugin → plugin

Runtime dependencies are declared per plugin:

```toml
[plugins.app-glue]
path = "plugins/app-glue"
language = "rust"
type = "library"
dependencies = ["catalog-go"]
```

The consumer imports the provider WIT package and compiles against generated
bindings. Consumer-side dependency imports are validated for Rust, Go,
TypeScript, C#, C and C++ (see
[`plans/DEPENDENCY_CONSUMPTION.md`](plans/DEPENDENCY_CONSUMPTION.md)). At load
time the host registers each imported interface and dispatches calls to the
declared provider in a separate store. Undeclared imports fail build and host load.
Only `library` providers are callable in 1.2; resources/streams/callbacks are
not supported across the boundary.

`owasidea build` publishes provider packages to `.owasidea/deps/` and builds a
per-consumer WIT view under `.owasidea/build/<consumer>/wit/`; see
[`BUILD_LAYOUT.md`](BUILD_LAYOUT.md). Do not hand-copy provider WIT into
`plugins/<consumer>/wit/deps/`.

Dependency declarations may form cycles; a runtime call that re-enters a plugin
already on the call stack fails with
`OWASIDEA_E_DEPENDENCY_REENTRANT_CALL`. Maximum nesting depth is 16. The plugin
map lock is released before guest code runs.

`compose_with` remains the explicit build-time alternative. The same edge cannot
use both `dependencies` and `compose_with`.

The glue can control declared service dependencies through the typed
`service-control` import (`start`, `status`, `stop`). `stop` drops the
host-held instance; it never calls guest stop code. The host checks that the
target is declared and is a `service`/`hybrid` plugin.

## 5. KV (multiple namespaces)

```toml
[plugins.notes]
kv = ["notes:read-write", "shared-cache:read"]
```

- Guest WIT: `open-kv(namespace) -> result<kv, string>`; the handle exposes
  `get` and `set`.
- Only declared namespaces open. Unknown namespaces return a guest error.
- A read-only handle returns an error on `set`; writes are never silently
  dropped.
- Storage stays `<OWASIDEA_DATA_DIR or ./data>/<namespace>/kv.sqlite`, so 1.1
  data is preserved. Sharing a namespace shares its data by intent.
- The old `kv = "ns:access"` form is converted by `owasidea migrate --write`
  and is rejected by 1.2 validation.

## 6. HTTP and sockets (network)

```toml
[plugins.api-client]
http_outbound = ["api.example.com:443"]        # exact host:port
network_outbound = ["api.example.com:443"]     # WASI sockets
network_inbound = ["127.0.0.1:8080"]           # local IP:port
capabilities = ["network-outbound"]            # optional preset
```

- Values are **exact `host:port` pairs**, not URLs; IPv6 uses brackets.
  Wildcards, schemes and bare hosts fail config validation.
- `network-outbound` allows any outbound destination and port but never inbound.
- `wasi:http`: the host checks scheme, hostname and effective port on every
  request. HTTPS is allowed; plaintext HTTP is only allowed to loopback.
  Redirects are not followed by the host; a guest-followed redirect is checked
  again. Exact allowlist hostnames are resolved when the plugin loads/reloads
  (load fails if DNS cannot answer) and HTTP connects only to those pinned IPs;
  TLS still verifies the original hostname. The `network-outbound` preset has no
  fixed host list and uses the OS resolver by intent.
- WASI sockets: outbound hostnames resolve at store creation and the socket
  address **and port** must match. Inbound binds require the exact `IP:port`.
- Denied HTTP returns a `wasi:http` `http-request-denied` error; denied sockets
  return `access-denied`. If the guest traps/unwraps, the host call returns 500
  (or `/app` returns 502/500).
- DNS lookup is available only when an outbound grant exists; resolver traffic
  itself is not filtered by hostname.

## 7. Filesystem

```toml
[plugins.reporter]
fs = ["reports:read-write"]   # preopen under OWASIDEA_DATA_DIR (or CWD)
```

The path is canonicalized per store and mounted read-only or read-write. There
is no inherited filesystem. Absolute paths bypass `OWASIDEA_DATA_DIR`; use
relative paths for portable apps.

## 8. What to read next

- [`PLUGIN_AUTHORING.md`](PLUGIN_AUTHORING.md) — language/build details.
- [`API.md`](API.md) — HTTP routes and payloads.
- [`SECURITY.md`](SECURITY.md) — threat model and residual risks.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — host internals.
