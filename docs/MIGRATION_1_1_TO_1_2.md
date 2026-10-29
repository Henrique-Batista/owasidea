# Migrate Owasidea 1.1 apps to 1.2

Owasidea 1.2.0 is an intentional breaking boundary. It uses host ABI 2 and
manifest schema 2; a 1.2 host rejects ABI 1 plugins. Keep 1.1 apps on a 1.1
host, or migrate and rebuild the app before using a 1.2 host. A 1.1 host
continues to support 1.1 apps.

## 1. Preview config migration

Run `owasidea migrate` from the app directory to see supported config changes
and required manual changes. It is a dry run: **no files are changed unless
`--write` is supplied**. Use `owasidea migrate --write` to write the supported
config migration. The command does not rewrite plugin source or guess missing
dependency declarations; apply those changes yourself.

The automatic KV config conversion changes a single grant such as:

```toml
kv = "app-state:read-write"
```

to a one-entry list:

```toml
kv = ["app-state:read-write"]
```

KV database files and their key/value contents stay in place. The namespace
path remains `<OWASIDEA_DATA_DIR>/<namespace>/kv.sqlite`; back up app data as a
normal precaution before upgrading.

## 2. Declare local WIT dependencies

Dependencies are local plugin names from the same `wasm-app.toml`. For each
plugin that calls another plugin, declare that provider explicitly:

```toml
[plugins.app-glue]
dependencies = ["catalog-go", "policy-rust"]
```

The consumer imports the provider's WIT package and builds against generated,
typed bindings. The provider's WIT package and version define the contract.
Dependencies do not use HTTP or a registry. Every imported plugin interface
must match a declared dependency; undeclared imports fail build and host load.
Dependency declarations may be cyclic, but a call chain cannot re-enter a
plugin already active in that request. Do not declare the same edge as both a
runtime dependency and `compose_with`.

`owasidea migrate` reports dependencies that need explicit declarations; it
does not edit source imports or create declarations on your behalf.

## 3. Route app UI through one glue plugin

Set `[app].ui_plugin` to the plugin that owns the app-facing API:

```toml
[app]
ui_plugin = "app-glue"
```

The shipped UI calls this glue plugin only. In production, app-facing routes
for other plugins are rejected; the glue plugin calls its declared backends
through WIT. Developer routes in `owasidea dev` and `owasidea tui` remain
available for all configured plugins. If your UI currently calls backend
plugins directly, route those actions through glue and declare the required
dependencies. Desktop apps should configure `ui_plugin`; without it the host keeps the
developer-style app surface so existing 1.1 apps remain usable during migration.

## 4. Update permissions

Existing explicit grants remain available. You can reduce repeated setup with
named presets, for example:

```toml
[plugins.app-glue]
capabilities = ["network-outbound", "app-data"]
```

- `network-outbound` allows outbound HTTP/socket destinations and ports; it
  does not allow inbound listeners.
- `app-data` allows read/write access only to this plugin's own data directory.
- `network-inbound` remains a separate explicit grant with bind address and
  port. It is not implied by either preset.

Presets expand to inspectable effective permissions. They do not grant access
to arbitrary host files, environment variables, or unrestricted inbound
listeners. Use exact grants when you need narrower access.

HTTP and socket allowlist entries now specify exact `host:port` pairs; IPv6
addresses use brackets, for example `"[2001:db8::1]:443"`. Do not use URLs or
wildcards in exact entries. HTTP policy also checks scheme: non-loopback HTTP
is denied, loopback HTTP is allowed for development, and HTTPS is allowed.
Outbound socket destinations are pinned to the addresses resolved at store
creation. Exact HTTP allowlist hostnames are also resolved once at store
creation; HTTP connects only to those IPs while TLS keeps the original hostname
for SNI/certificate validation. The `network-outbound` preset has no fixed host
list and uses the OS resolver by intent. Guest-followed redirects are checked as
new requests. Update old host-only entries to the intended ports.

## 5. Rebuild and verify

After updating config and plugin code, rebuild **all** plugins with the 1.2
toolchain and regenerate their manifests. ABI 2 changes the host/plugin WIT
and manifest contracts; do not combine ABI 1 artifacts with a 1.2 host or
reuse old generated manifests. Do not edit generated manifests by hand.

Run `owasidea build`, then `owasidea validate` and your app's tests/smoke tests.
Confirm the generated manifests use `host_abi = "2"` and `schema_version = 2`
before bundling or running with the 1.2 host.

See [ADR 0006](adr/0006-plugin-composition-and-capabilities.md),
[ABI policy](ABI_POLICY.md), and the
[1.2 implementation plan](plans/IMPLEMENTATION_1_2.md) for contract details.
