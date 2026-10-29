# Security model (1.0)

## Trust boundaries

- WASI components and webview UI bundles are untrusted inputs.
- The host is trusted and runs with the operating-system identity of the user.
- The Tauri webview and local HTTP API are same-machine interfaces, not remote
  authentication boundaries.

## Enforced controls

- **Import integrity:** Wasmtime component imports are checked against the
  generated manifest before instantiation. Undeclared or stale WASI/host
  interfaces fail startup/reload.
- **WASI filesystem:** No inherited directories. Only manifest-declared,
  canonicalized preopens are mounted, with explicit read-only/read-write access.
- **WASI sockets:** Network defaults to deny. Exact outbound `host:port` pairs
  are resolved at instance creation and every socket destination is checked for
  address **and port**. Inbound binds require explicit local `IP:port`. The
  opt-in `network-outbound` preset allows any outbound destination/port but no
  inbound listener. TCP/UDP and DNS imports must also exist in the component WIT.
- **WASI HTTP:** Outbound HTTP is denied unless exact `host:port` pairs are
  listed in `permissions.http.outbound`; wildcard `*` is rejected. Every request
  checks scheme, hostname and effective port; non-loopback plaintext HTTP is
  denied. The host does not follow redirects, so a guest-followed redirect is a
  new checked request. Exact allowlist hostnames are resolved once when the
  store is created; the client connects only to those pinned IPs while TLS keeps
  the original hostname for SNI and certificate validation. The
  `network-outbound` preset has no fixed host list and uses the OS resolver by
  intent. Executable HTTP
  components run through embedded `wasmtime-wasi-http`, not a sidecar with
  ambient network access. Guest responses proxied through `/app/{name}` are
  header-filtered to an allowlist (`content-type`, `content-length`,
  `cache-control`, `etag`, `last-modified`, `expires`); `set-cookie`,
  `access-control-*` and framing headers are dropped, `nosniff` plus CSP
  `sandbox` are added, and HTML/JS/CSS/SVG are served as `text/plain` so a guest
  cannot execute script or set cookies on the host origin.
- **Host KV/log:** KV imports require a matching namespace grant. Each opaque
  handle carries its own read/write grant; unknown namespaces and read-only
  writes return explicit guest errors. Plugin logs use bounded per-plugin ring
  buffers with per-message truncation.
- **Process/environment:** Plugins do not inherit host environment variables,
  stdin, stdout, or stderr. Tailcat's optional network hints are allowlisted and
  passed only to that named integration. Tailcat's hardcoded service routes are
  registered only when the plugin is loaded or `OWASIDEA_ENABLE_TAILCAT=1` is
  set, so a generic host does not expose that optional integration.
- **Resources:** Component stores apply manifest fuel and memory limits; a
  per-plugin semaphore enforces `max_instances`, and host-driven calls/HTTP
  response startup have request timeouts. A timed-out call drops its store and
  returns the instance slot. These timeouts interrupt a guest that yields (I/O,
  timers, async imports); a purely CPU-bound guest is stopped by `max_fuel`,
  not by wall-clock time.
- **Integrity:** SHA-256 detects accidental/tampered component changes and UI
  manifests record the asset digest map; the webview checks each file before
  loading the iframe. Manifests themselves are unsigned and trusted from the
  filesystem: checksums do not authenticate the policy. Release integrity comes
  from the build pipeline and OS code signing of the installer.
- **Webview UI:** UI plugins are webview-only and run in iframes without
  same-origin/top-navigation/popup privileges. Tauri CSP restricts scripts,
  frames, workers, and network destinations. Host CORS allows only Tauri and
  local Vite origins by default. CSP `connect-src` is generated from
  `[host].addr` by `config render --target tauri`; bundle mode rejects a
  generated config whose CSP does not allow the exact host origin. The dev
  config additionally allows `http://127.0.0.1:*`. Bare `*` and `null` are
  never emitted.

## Desktop packaging

Tauri bundles should include only generated manifests, component `.wasm` files
and the runtime descriptor as resources. `owasidea bundle` generates
`bundle.resources` from `wasm-app.toml` (manifests, every headless component,
`owasidea.runtime.json`); the webview UI is embedded through
`build.frontendDist` rather than duplicated as a resource.
The desktop shell must set `OWASIDEA_DATA_DIR` to the platform app-data
directory (creating it) before starting the host; otherwise KV databases and
relative filesystem preopens resolve against the process working directory.
Do not package source trees, `node_modules`, signing keys, or UI-plugin build
caches. The host still binds loopback only; the webview is not a remote trust
boundary.

`owasidea verify` checks manifests, component SHA-256, ABI fields and UI asset
digests. It does not verify manifest signatures because manifests are unsigned.

## Production configuration

Plugins are unsigned and checked by SHA-256. To protect a release, sign the
installer with the platform tooling (Authenticode/Developer ID) and publish
checksums/signatures through a trusted channel; see `docs/CODE_SIGNING.md` and
`docs/RELEASE.md`.

```bash
# Set only required browser origins; do not include `*`.
export OWASIDEA_ALLOWED_ORIGINS='tauri://localhost,https://tauri.localhost'
```

Keep host bound to loopback unless a separately authenticated deployment is
configured.

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
- Manifests are unsigned. An attacker who can rewrite `dist/` can widen
  permissions; SHA-256 only detects component drift. Protect the release
  directory and use OS code signing/update channels.
- This pilot does not implement multi-tenant isolation, publisher key
  rotation/revocation, or marketplace moderation.
- Host HTTP API has no user authentication. CORS is a browser policy, not
  authentication; keep it on loopback and do not expose it to untrusted networks.

## Incident response

1. Stop the host and disable the affected plugin.
2. Remove its manifest/artifact from the local bundle and revoke publisher trust
   through deployment configuration.
3. Preserve logs and the generated manifest for investigation.
4. Publish a new immutable plugin version; do not replace an artifact in place.

## Permission diffs (PX-01)

`owasidea build` writes `permissions.lock.json` and `permissions.diff.json` in the manifest dir. Use `--fail-on-widen` to fail when a permission is added.
