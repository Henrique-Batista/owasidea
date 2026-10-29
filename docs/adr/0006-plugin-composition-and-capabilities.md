# ADR 0006 — Runtime plugin calls, app glue and capabilities

Status: **Accepted**  
Date: 2026-10-28  
Target release: **1.2.0** (intentional breaking release)

## Context

Owasidea 1.1 supports independent WASI components and build-time composition.
An app UI can call each plugin through host HTTP routes, but the host has no
runtime plugin resolver. `compose_with` is static linking and cannot represent
dependency cycles. KV config supports one namespace per plugin. Current HTTP
allowlists do not restrict ports or pin DNS results.

The application UI should call one glue plugin. Plugins should call declared
dependencies directly through generated, typed bindings; UI must not orchestrate
backend modules. Dependency declarations may form cycles, but a single nested
call chain must not recurse into an already active plugin.

## Decisions

### 1. App UI calls one glue plugin

Apps may set:

```toml
[app]
ui_plugin = "glue"
```

The shipped UI uses the app-facing call route for this plugin only. In
production mode, host HTTP routes reject app-facing calls to other plugins.
`owasidea dev` and `owasidea tui` retain developer access to all configured
plugins. This is an API-surface rule, **not authentication against other local
processes running as the same OS user**; the host remains loopback-only.

### 2. Runtime calls are host-mediated and typed from WIT

Each plugin declares local app dependencies in `wasm-app.toml`:

```toml
[plugins.glue-rust]
dependencies = ["catalog-go", "policy-rust"]
```

The dependency's WIT package is made available to the consumer during build.
The consumer compiles against generated WIT bindings; it does not hand-write
bindings or make HTTP requests. At runtime, the component imports the dependency
interface and the host resolves that call to the declared plugin.

The host validates that:

- every dependency names a configured plugin;
- every imported package/interface matches the dependency's WIT;
- undeclared plugin imports fail build and host load;
- the requested export exists and argument/result types match WIT.

Runtime dependency calls in the first 1.2 slice target `library` plugins.
Service lifecycle remains host-owned but is callable through the glue's
generated `service-control` import for declared service dependencies:
`start(plugin)`, `status(plugin)` and `stop(plugin)`. This maps to the existing
host lifecycle manager; it does not call guest stop code. General calls into a
running service's arbitrary exports are deferred.

The service-control surface returns typed status, not JSON:

```wit
record service-state {
    running: bool,
    address: option<string>,
    last-error: option<string>,
    restarts: u32,
}

interface service-control {
    start: func(plugin: string) -> result<string, string>;
    status: func(plugin: string) -> service-state;
    stop: func(plugin: string) -> result<_, string>;
}
```

Each target must also appear in `dependencies`. The host checks plugin kind and
the dependency edge before performing lifecycle work.

No registry, remote download, or separate plugin package manager is added.
Dependencies resolve only among plugins in the app config. WIT package/version
is contract identity; the repo/app config is source identity.

### 3. Dependency cycles are allowed; recursive call chains are rejected

The dependency graph may contain cycles. Runtime loading does not topologically
instantiate dependencies. Calls are resolved lazily through the host.

The host carries a per-request call stack. If a nested call targets a plugin
already on that stack, return stable `DEPENDENCY_REENTRANT_CALL` rather than
waiting on its own instance semaphore. Also enforce a maximum nested-call depth,
one overall deadline, existing fuel/memory limits, and existing per-plugin
`max_instances`. A cycle in declarations is valid; an actual recursive
A→B→A invocation in one request is not.

`compose_with` remains an explicit build-time optimization for acyclic,
statically linked clients. It is separate from runtime `dependencies`; a plugin
must not declare the same edge in both modes.

### 4. Multiple KV handles per plugin

Config becomes a list of namespace grants:

```toml
[plugins.glue-rust]
kv = ["glue-state:read-write", "shared-catalog:read"]
```

The host WIT interface exposes `open-kv(namespace)` returning an opaque
resource handle with `get` and `set`. Access is fixed by the manifest grant,
not guest-supplied. Unknown namespaces fail explicitly; denied writes do not
silently succeed. Each handle is bound to one SQLite namespace. Reusing a
namespace across plugins is intentional shared storage.

The old single-string config can be migrated to a one-entry list with a warning
for one release; existing SQLite namespace paths and key/value data stay
unchanged. The generated manifest schema and WIT host interface change under
ABI 2.

### 5. Keep sandbox; reduce permission authoring friction

Sandbox remains enabled by default. Do not add an ambient “trusted plugin” mode
that silently gives host filesystem or network access. Add named permission
presets that expand to explicit manifest capabilities and remain visible in
`inspect`/UI:

- `network-outbound`: unrestricted outbound HTTP/socket destinations, no inbound
  listeners;
- `app-data`: that plugin's own data directory only;
- `network-inbound`: only when explicitly requested, with bind addresses/ports;
- explicit host/IP + port allowlists for least privilege.

Fuel, memory, request timeout, instance count and Wasmtime capability
boundaries remain enforced. WASI is not OS-level isolation; do not claim it is.

### 6. Network allowlists include port and resist DNS rebinding

- HTTP/network declarations accept exact host/IP and optional port, not URLs or
  wildcards.
- HTTP checks scheme, effective port and hostname on every request; redirects
  are not followed by host. A guest-followed redirect is checked as a new call.
- Resolve and pin allowed host addresses for each store/request policy; connect
  only to a pinned address while retaining original hostname for TLS SNI and
  certificate validation.
- Non-loopback HTTP requires HTTPS. Loopback HTTP remains available for dev.
- Socket permissions use exact address + port where protocol provides it.
- DNS resolution itself remains a narrow bootstrap capability and is documented.

These manifest permission shape changes ship under ABI 2. Tests must cover
denied host, wrong port, redirect, rebinding, TLS hostname validation, and
loopback HTTP.

Implementation status (1.2.0): host/port/scheme checks, loopback-only plaintext
HTTP, socket address+port pinning, exact-allowlist HTTP DNS pinning with TLS
SNI, the typed `service-control` import, and a glue-only Shipyard surface are
implemented. The `network-outbound` preset intentionally has no fixed host list
and uses the OS resolver. Executable dependencies are reachable at
`/app/{name}` only when the glue declares them.

## Compatibility and release policy

1.2.0 is intentionally allowed to break 1.1 manifests/configs as requested.
Set `host_abi = "2"` and `schema_version = 2`; 1.2 host rejects ABI 1 with a
clear migration error. `owasidea migrate` (or equivalent explicit command)
converts old `kv = "namespace:access"` to the new one-entry form and reports
other required config changes. Never silently rewrite app config. Existing KV
files remain in place.

This is an explicit exception to current semver wording: update
`docs/ABI_POLICY.md` to state that 1.2.0 is a planned breaking boundary even
though product version major remains 1. Existing 1.1 apps remain supported by
the 1.1 host; they are not silently accepted by ABI 2.

## Alternatives rejected

- **UI HTTP orchestration:** makes UI know backend topology, has no generated
  typed bindings, and cannot serve headless composition cleanly.
- **HTTP between plugins:** couples runtime calls to network routes and
  reintroduces ambient HTTP permissions.
- **Only `wasm-tools compose`:** cannot support runtime dependency cycles.
- **Delete sandbox:** makes a compromised plugin/dependency equivalent to
  unrestricted host code; presets address friction without deleting controls.

## Implementation plan

Detailed, ordered work items and acceptance gates:
[`docs/plans/IMPLEMENTATION_1_2.md`](../plans/IMPLEMENTATION_1_2.md).
