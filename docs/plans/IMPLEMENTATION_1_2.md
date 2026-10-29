# Owasidea 1.2 — Runtime dependencies and developer experience

Status: **ready for implementation after Gate 0**  
Date: 2026-10-28  
Target: **1.2.0**, intentionally breaking manifest/WIT ABI (`host_abi = "2"`,
`schema_version = 2`)  
Architecture decision: [`ADR-0006`](../adr/0006-plugin-composition-and-capabilities.md)

## Goal

An app UI calls one configured glue plugin. Plugins call local dependencies
directly through generated WIT bindings; the host routes calls and enforces the
dependency graph. Dependency declarations may be cyclic, but a nested call may
not re-enter a plugin already on its call stack. A plugin may open multiple KV
namespaces with independent access grants. Permissions stay on by default but
get named presets. HTTP and socket policies include ports and resist DNS
rebinding.

No registry, remote downloads, or host JavaScript runtime.

## Decisions locked by the product owner

| Area | Decision |
| --- | --- |
| Plugin calls | Host-mediated; WIT types generate consumer bindings during build. |
| Dependencies | Local plugins declared in app config; no registry. WIT package/version is contract identity. |
| Cycles | Dependency graph may cycle; runtime recursion/re-entry into active plugin is rejected. |
| UI | Shipped app UI calls configured glue plugin only; host enforces. Dev TUI may access all plugins. |
| KV | Multiple namespace handles per plugin, access fixed by app config. |
| Sandbox | Retain, default sandboxed. Named presets reduce permission boilerplate; no implicit ambient filesystem/network. |
| Network | Exact host/IP + port, DNS pinning, redirects rechecked, non-loopback HTTP denied. |
| Compatibility | 1.2.0 is intentionally breaking; ABI 2/schema 2, migration guide required. |
| First implementation | Vertical slice with two languages, glue UI, dependency RPC and multiple KV. |

## Planned config and contracts

### Local dependency declarations

```toml
[app]
ui_plugin = "app-glue"

[plugins.app-glue]
path = "plugins/app-glue"
language = "rust"
type = "library"
dependencies = ["catalog-go", "policy-rust"]
kv = ["app-state:read-write", "catalog-cache:read"]
```

- `dependencies` names plugins in the same app config only.
- The provider's WIT package and version define callable contract.
- Build resolves all plugin contracts before compiling any plugin, so cyclic
  declarations do not require topological build order.
- Prove imported WIT dependency compilation first for Rust and Go. Before 1.2.0
  release, run a language conformance matrix for Rust, Go, TypeScript, C#, C and
  C++; if a toolchain cannot import dependencies, report that as an explicit
  unsupported case rather than silently falling back to HTTP.
- Runtime `dependencies` are distinct from explicit `compose_with` static
  composition. The same edge cannot use both modes.
- Every component import must match an app dependency; undeclared imports fail
  build and host load with plugin/package details.

### Runtime call behavior

- Consumer source imports the provider WIT package; generated bindings provide
  normal language-level types.
- Host linker supplies dependency interfaces. Calls resolve to configured
  plugin exports, never to arbitrary names outside the declared graph.
- First vertical slice supports `library` providers and WIT value types already
  supported by generated bindings. Resources, streams, callbacks and arbitrary
  service exports are excluded from dependency RPC.
- Glue controls declared service dependencies through a typed
  `service-control` WIT import: `start(plugin)`, `status(plugin)` and
  `stop(plugin)`. Status contains `running`, `address`, `last-error`, and restart
  count. `stop` drops the host-held instance; it never calls guest stop code.
- Host propagates request ID, cancellation and remaining deadline; nested work
  shares fuel/time/instance budgets where possible.
- Call stack rejects re-entry into an active plugin with stable
  `OWASIDEA_E_DEPENDENCY_REENTRANT_CALL`; maximum depth is 16 in v1.2.
- Missing plugin/export, type mismatch, timeout and trap produce structured
  errors; caller's store/permit is always released.

### UI glue enforcement

- `[app].ui_plugin` is required for a production desktop bundle when app UI
  exists. `init` scaffolds a minimal glue plugin and sets it by default.
- Production app-facing routes expose only the configured glue plugin. Calls to
  another plugin return 403/404; plugin dependencies are only reachable through
  host WIT calls.
- The dev dashboard/TUI keeps developer routes for all configured plugins.
- CORS is not authentication. Scope is a browser/app API surface; a process
  running as the same OS user can still access loopback. Keep host loopback-only.
- Shipyard's user-facing actions are routed through `app-glue`; tests cover
  current workbench, service UI and `wasi:http` behavior.

### Multi-KV

```wit
interface host-kv {
    resource kv {
        get: func(key: string) -> option<string>;
        set: func(key: string, value: string);
    }
    open-kv: func(namespace: string) -> result<kv, string>;
}
```

- `open-kv` succeeds only for a namespace declared in the plugin manifest.
- `read` handles can call `get`; `set` on read-only handle returns a WIT error
  (not silent success).
- Namespace remains `<OWASIDEA_DATA_DIR>/<namespace>/kv.sqlite`; shared namespace
  means shared data by intent.
- `kv = "ns:access"` is migrated to one list entry with a warning; DB files and
  key/value contents are preserved.

### Permission presets

Keep explicit current `fs`, `kv`, `http_outbound`, `network_outbound` and
`network_inbound` grants. Add named `capabilities` presets, not `trust=trusted`
ambient access:

```toml
[plugins.app-glue]
capabilities = ["network-outbound", "app-data"]
```

- `network-outbound`: any outbound destination/port; no inbound listener.
- `app-data`: read/write only under the plugin's own data directory.
- `network-inbound`: remains separately explicit, with bind address/port.
- Presets expand into inspectable effective manifest permissions and appear in
  `permissions.lock.json` diffs. An app can always replace a preset with exact
  host/port/path grants.
- No preset grants host OS filesystem, arbitrary environment variables, or
  unrestricted inbound listeners.

### HTTP and sockets

Config exact entries use `host:port` (IPv6 uses bracket notation); wildcard is
not accepted in exact allowlists. `http_outbound` scheme is derived per request:
non-loopback HTTP is denied, loopback HTTP is allowed for dev, HTTPS is allowed.

- Compare hostname, effective port and scheme on every HTTP request.
- Resolve and pin allowed addresses per store; connect to a pinned IP while
  preserving hostname for TLS SNI/certificate validation.
- Host does not follow redirects. A guest-followed redirect is a new checked
  request and cannot escape the policy.
- Socket connect/send/bind/listen checks include address and port. DNS bootstrap
  behavior is documented and tested.

## Work items and gates

### Gate 0 — Wasmtime/runtime feasibility spike (blocking)

**Question:** can Wasmtime 50 async host imports suspend a caller component,
instantiate/call a dependency in a separate store, and resume the caller without
holding a lock that prevents A→B→A detection?

- Build a disposable Rust→Go component call through a WIT host import.
- Verify args/results use actual WIT types (not an untyped HTTP/JSON-only API).
- Exercise timeout, cancellation, trap and call-stack propagation.
- No product code until spike proves this path. If it fails, revise ADR before
  choosing a generated adapter/worker design.

**Pass:** typed Rust→Go call returns; errors/timeouts are bounded; no lock is
held across guest execution; call context identifies re-entry. **Fail:** stop and
redesign before touching ABI.

### Phase 1 — ABI 2 and dependency build graph

Files: `crates/cli/src/config.rs`, `wit_flow.rs`, `wit_types.rs`, build code in
`main.rs` split into `build.rs`, `crates/host/src/lib.rs`, `abi.rs`, docs.

- Set package version 1.2.0, `HOST_ABI = "2"`, `schema_version = 2`; update
  `ABI_POLICY.md` with the explicit 1.2 minor-breaking exception.
- Add local `dependencies` config and typed schema validation.
- Build pipeline stages: discover plugins → render all WIT → resolve local WIT
  imports/generate bindings → compile all components → validate imports → emit
  manifests. Cycles in the graph must not affect contract generation/build order.
- Keep `compose_with` explicit and reject duplicate runtime/static edges.
- Add `owasidea migrate` for 1.1 config: convert single `kv` string, report
  dependencies requiring explicit declaration, never change source silently.
- Tests: unknown dependency, missing WIT package, wrong version, undeclared
  import, cycle config, build reproducibility and migration preserving KV data.

### Phase 2 — Host-mediated typed runtime calls

Files: new `crates/host/src/dependencies.rs`; `host/src/lib.rs` linker/store,
`jobs.rs` call context; generated binding templates and Go/Rust examples.

- Register WIT imported dependency interfaces from the app graph.
- Dispatch to provider exports using WIT type metadata; only `library` targets
  in first slice.
- Add `CallContext { stack, depth, deadline, request_id }`; reject active-plugin
  re-entry and depth >16 with stable error.
- Do not hold plugin map/service locks while executing guest code.
- Add concurrency/permit tests: parallel independent calls work; nested call
  timeout/trap releases all slots; cycles fail promptly rather than hanging.
- End-to-end: Rust glue calls Go provider; Go client calls Rust provider; a
  declared A↔B graph builds; `A→B→A` invocation returns reentrancy error.

### Phase 3 — Glue-only app API

Files: `config.rs`, app route module, UI runtime descriptor, `ui/main.ts`, Tauri
runtime, `examples/hello`, `examples/shipyard`, Playwright tests.

- Add `[app].ui_plugin`; scaffold default glue for full/desktop presets.
- Production app-facing routes expose glue only: plugin calls, executable
  `/app/*`, service lifecycle, plugin logs/manifests and plugin enumeration are
  hidden/rejected for non-glue plugins. Health/runtime metadata remains public.
  Dev TUI/admin routes remain available in dev mode. Never use CORS as route
  authorization.
- App UI invokes glue exports only. Glue delegates business calls through WIT
  dependencies; no browser-to-plugin fan-out.
- Migrate Shipyard workbench actions through `app-glue`; its declared service
  dependencies use `service-control`. Direct production
  `/plugin/{other}/service/*` routes are denied.
- Tests: app route can call glue; direct call to non-glue is denied in packaged
  mode; dev dashboard still sees/calls plugins; iframe bridge can only reach
  explicitly allowed glue exports.

### Phase 4 — Multi-KV WIT and config

Files: host WIT package, Rust/Go generated bindings, config/schema/manifest,
`create_store` linker, SQLite helpers, migration/docs.

- Replace single `Option<KvPermission>` with namespace grants.
- Implement opaque KV resource handles, namespace validation and per-handle
  read/write enforcement.
- Unknown namespace/open and denied writes return explicit guest errors.
- Unit/integration tests: two handles in one plugin, read vs write, missing
  grant, shared namespace behavior, restart persistence, old DB compatibility.
- Include KV in the Rust glue→Go provider vertical slice acceptance.

### Phase 5 — Permission presets and network hardening

Files: config/schema, manifest permissions, `AllowlistHooks`, socket check,
resolver/connector, error catalog, SECURITY/RUNTIME_MODEL docs.

- Add only `network-outbound` and plugin-private `app-data` presets initially.
- Effective preset expansion is visible in `inspect` and permission diff/lock.
- Add host/port allowlist parsing; reject wildcards in exact entries.
- Pin resolved IPs; retain TLS SNI hostname; reject non-loopback HTTP; re-check
  guest-followed redirects.
- Add tests for HTTP denied host/port/scheme, DNS rebinding, redirects, TLS SNI,
  socket ports/binds, preset expansion and permission widening.
- Preserve default deny and loopback host bind. Do not add ambient OS paths.

### Phase 6 — 1.2 migration, examples and release

- Migration guide from 1.1: ABI 2 host requirement, dependency WIT imports,
  glue UI, KV list, permission presets, HTTP host:port syntax.
- `examples/hello`: single plugin acts as glue; simplest path has no dependency
  or KV unless needed.
- Add `examples/interop` with Rust↔Go runtime dependencies, cyclic declarations,
  glue UI and two KV namespaces.
- Shipyard preserves user flows through glue and remains build/smoke/UI tested.
- Update `GET_STARTED`, `PLUGIN_AUTHORING`, `RUNTIME_MODEL`, `API`, `SECURITY`,
  CLI help, schema and llms.txt.
- Full gates: `cargo fmt --check`, clippy `-D warnings`, `cargo test`,
  `scripts/test-examples.sh`, Shipyard build/smoke/test, Linux desktop bundle,
  CLI bundle, Playwright; Windows CI for mouse/TUI and bundle.

## Definition of done

- Plugin authors call dependencies using generated typed bindings, without
  writing HTTP clients or WIT glue by hand.
- UI production API permits only configured glue; dev tools retain full access.
- Cyclic declarations build and runtime recursion fails quickly with a clear
  error; no hangs or leaked instance permits.
- One plugin opens multiple KV handles with independent grants; data persists
  across host restart and 1.1 namespace data is preserved.
- Sandbox remains default; presets reduce setup without granting host OS access.
- HTTP/sockets enforce host, port, scheme and pinned address rules with tests.
- 1.1 manifests/config fail with actionable migration guidance; 1.2 ABI/schema
  values are consistent in generated manifests, CLI and docs.

## Explicitly out of scope for first 1.2 slice

- Remote plugin registry/marketplace or runtime downloads.
- Cyclic nested re-entry (A→B→A) within one call stack.
- Arbitrary runtime calls to service exports; only standard lifecycle controls
  go through `service-control` in 1.2.
- Resources/streams/callback WIT values across plugin calls.
- Unrestricted host filesystem or environment access.
- Replacing ratatui or adding another TUI runtime.
