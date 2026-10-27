# Security model (production pilot)

## Trust boundaries

- WASI components and webview UI bundles are untrusted inputs.
- The host is trusted and runs with the operating-system identity of the user.
- Registry indexes are discovery metadata; publisher public keys are trust roots
  distributed out-of-band.
- The Tauri webview and local HTTP API are same-machine interfaces, not remote
  authentication boundaries.

## Enforced controls

- **Import integrity:** Wasmtime component imports are checked against the
  generated manifest before instantiation. Undeclared or stale WASI/host
  interfaces fail startup/reload.
- **WASI filesystem:** No inherited directories. Only manifest-declared,
  canonicalized preopens are mounted, with explicit read-only/read-write access.
- **WASI sockets:** Network defaults to deny. Exact outbound hostnames/IPs are
  resolved at instance creation and socket destinations are checked against the
  resulting address set. Inbound binds require explicit local IPs. TCP/UDP and
  DNS imports must also exist in the component WIT.
- **WASI HTTP:** Outbound HTTP is denied unless exact hosts are listed in
  `permissions.http.outbound`; wildcard `*` is rejected. Executable HTTP
  components run through embedded `wasmtime-wasi-http`, not a sidecar with
  ambient network access. Guest responses proxied through `/app/{name}` are
  header-filtered to an allowlist (`content-type`, `content-length`,
  `cache-control`, `etag`, `last-modified`, `expires`); `set-cookie`,
  `access-control-*` and framing headers are dropped, `nosniff` plus CSP
  `sandbox` are added, and HTML/JS/CSS/SVG are served as `text/plain` so a guest
  cannot execute script or set cookies on the host origin.
- **Host KV/log:** KV imports require a matching namespace/access permission;
  read-only plugins cannot persist writes. Plugin logs use bounded per-plugin
  ring buffers with per-message truncation.
- **Process/environment:** Plugins do not inherit host environment variables,
  stdin, stdout, or stderr. Tailcat's optional network hints are allowlisted and
  passed only to that named integration. Tailcat's hardcoded service routes are
  registered only when the plugin is loaded or `OWASIDEA_ENABLE_TAILCAT=1` is
  set, so a generic host does not expose that optional integration.
- **Resources:** Component stores apply manifest fuel and memory limits; a
  per-plugin semaphore enforces `max_instances`, and host-driven calls/HTTP
  response startup have request timeouts. The typed POC routes (`dial-tcp`,
  `start-server`, `bind-udp`, `tls-probe`, `dial-echo`, `echo-udp`,
  `parse-address`) use the manifest `timeout_ms` (default 10 s) and answer `504`;
  a timed-out call drops its store and returns the instance slot. These
  timeouts interrupt a guest that yields (I/O, timers, async imports); a purely
  CPU-bound guest is stopped by `max_fuel`, not by wall-clock time.
- **Integrity/signing:** SHA-256 detects accidental/tampered artifact changes.
  Ed25519 `owasidea-manifest-v1` signatures authenticate effective manifest
  permissions plus WASI component bytes. UI manifests sign the complete asset
  digest map; the webview checks each file before loading the iframe.
- **Registry:** HTTPS only (except localhost tests), redirects rejected,
  same-origin package files, strict path checks, size caps, exact versions, and
  pinned public-key verification.
- **Webview UI:** UI plugins are webview-only and run in iframes without
  same-origin/top-navigation/popup privileges. Tauri CSP restricts scripts,
  frames, workers, and network destinations. Host CORS allows only Tauri and
  local Vite origins by default. CSP `connect-src` is generated from
  `[host].addr` by `config render --target tauri`; bundle mode rejects a
  generated config whose CSP does not allow the exact host origin. The dev
  config additionally allows `http://127.0.0.1:*`. Bare `*` and `null` are
  never emitted.

## Desktop packaging

Tauri bundles should include only generated manifests, component `.wasm` files,
the trust key and the runtime descriptor as resources. `owasidea bundle`
generates `bundle.resources` from `wasm-app.toml` (manifests, every headless
component, `trust/owasidea.pub`, `owasidea.runtime.json`); the webview UI is
embedded through `build.frontendDist` rather than duplicated as a resource.
The desktop shell must set `OWASIDEA_DATA_DIR` to the platform app-data
directory (creating it) before starting the host; otherwise KV databases and
relative filesystem preopens resolve against the process working directory.
Do not package source trees, `node_modules`, signing keys, or UI-plugin build
caches. The host still binds loopback only; the webview is not a remote trust
boundary.

`owasidea verify` requires a pinned trust root (`--public-key` or
`OWASIDEA_PUBLIC_KEY`). A public key staged inside the artifact directory is not
accepted by default, because it is writable by whoever can rewrite the
artifacts; `--allow-dist-trust` exists only for local smoke checks.

## Production configuration

Debug builds permit unsigned plugins by default. Release builds require verified
signatures; configure the trust key explicitly:

```bash
export OWASIDEA_REQUIRE_SIGNATURES=true
export OWASIDEA_PUBLIC_KEY=/etc/owasidea/trusted-publisher.pub
# Set only required browser origins; do not include `*`.
export OWASIDEA_ALLOWED_ORIGINS='tauri://localhost,https://tauri.localhost'
```

Keep host bound to loopback unless a separately authenticated deployment is
configured. Protect private signing keys; `owasidea keygen` writes Unix private
keys with mode `0600`. Registry public keys must be pinned through trusted app
configuration, not downloaded from the registry being checked.

### Telemetry (RL-05)

OTLP trace export is opt-in and never enabled by default:

- Build with `--features otel`; nothing is linked or started otherwise.
- Export only starts when `OTEL_EXPORTER_OTLP_ENDPOINT` (or
  `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`) is set to a non-empty value.
- Traces leave the process over OTLP/HTTP to the configured collector. Treat
  the collector as a trusted sink: span attributes include plugin names,
  export/function names, and request paths — not request bodies, argument
  values, prompt text, or KV contents.
- The `traceparent` header is accepted from clients for context propagation and
  is never trusted for authorization. Malformed values are ignored.
- Point the endpoint at a local collector or an authenticated internal
  endpoint; do not send traces over untrusted networks without TLS.
- Export is best-effort: an unreachable collector logs exporter warnings and
  never blocks or fails a plugin call.

## Limits and residual risks

- Wasmtime provides a WebAssembly capability sandbox, not an OS user/container
  boundary. A Wasmtime/runtime vulnerability can compromise the host process;
  production deployment should add OS-level sandboxing and least-privilege user
  accounts.
- DNS resolution itself is enabled when a component imports name lookup and has
  outbound socket permissions. Socket destinations are restricted after
  resolution, but resolver traffic is not filtered by hostname.
- Explicit `network.inbound` grants permit the plugin to bind those IPs and
  receive traffic on its ports. Review manifests as carefully as firewall rules.
- `OWASIDEA_REQUIRE_SIGNATURES` defaults to true in release builds and false in
  debug builds. A present signature without a configured public key is warned
  about, not verified, in debug mode.
- This pilot does not implement multi-tenant isolation, remote registry account
  authorization, signing-key rotation/revocation, or marketplace moderation.
- Host HTTP API has no user authentication. CORS is a browser policy, not
  authentication; keep it on loopback and do not expose it to untrusted networks.

## Incident response

1. Stop the host and disable the affected plugin.
2. Remove its manifest/artifact from the local bundle and revoke publisher trust
   through deployment configuration.
3. Preserve logs and the signed manifest for investigation.
4. Publish a new immutable plugin version; do not replace an artifact in place.

## Permission diffs (PX-01)

`owasidea build` writes `permissions.lock.json` and `permissions.diff.json` in the manifest dir. Use `--fail-on-widen` to fail when a permission is added.
