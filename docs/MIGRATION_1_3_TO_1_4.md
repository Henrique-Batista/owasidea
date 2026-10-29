# Migrating 1.3 → 1.4: HTTP exposure is deny-by-default

1.4 stops exposing every plugin on `/plugin/{name}/*` and `/app/{name}`. A
plugin must opt in:

```toml
[plugins.notes]
expose = true
```

## What stays reachable

- `GET /health`, `/overview`, `/metrics`, `/plugins`, `/ui-plugins` and the UI
  assets.
- The configured `[app].ui_plugin` glue, without `expose = true`, because the
  webview calls it.
- Plugins with `expose = true`.
- Plugin-to-plugin `dependencies` calls: host-mediated, never HTTP.
- `owasidea dev`, `owasidea smoke`, `owasidea test` and any host started with
  `OWASIDEA_DEV_MODE=1`: the full developer surface.

## What now returns 403

- `/plugin/{name}/call`, `/greet`, `/complete`, `/manifest`, `/logs`,
  `/reload`, `/service/*` and `/call/jobs` for a plugin without `expose = true`.
- `/app/{name}` for an executable/hybrid plugin without `expose = true`.
- `GET /plugins` hides unexposed plugins in production.

## Steps

1. Add `expose = true` to every plugin a person, script, dashboard or bundle
   calls over HTTP. Keep internal providers and dependencies unexposed.
2. Rebuild with `owasidea build`. The generated manifest gets
   `"http_exposed": true|false` and `min_host: 1.4.0`.
3. Test with `owasidea run` (production surface) and `curl`; use
   `owasidea dev` when you need every plugin reachable.
4. Check each example:
   - `examples/ticker`: the UI calls the generic service routes; the plugin sets
     `expose = true`.
   - `examples/shipyard`: the UI calls `/app/webhook-go`; that plugin sets
     `expose = true`. The `app-glue` plugin is the `ui_plugin` and does not need
     the flag.
   - `examples/hello` and `examples/notes`: the `ui_plugin` is the single
     plugin; it stays reachable without the flag.

## Compatibility

- Manifests built by 1.3 have no `http_exposed` field. A 1.4 host treats them as
  exposed, so upgrading the host does not break running apps. Rebuild with 1.4
  to get default-deny.
- New 1.4 manifests declare `min_host: 1.4.0`; a 1.3 host rejects them. To roll
  back, rebuild with the 1.3 CLI.
- `host_abi` stays `2`; this is an additive manifest field plus a host policy
  change, not an ABI break.

## Dev-only bypass

`OWASIDEA_DEV_MODE=1` is set by `owasidea dev`, `owasidea smoke` and
`owasidea test`. Do not set it for a production `owasidea run` or a bundled
host. The bypass is intentionally explicit.
