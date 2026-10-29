# ADR 0003 — Runtime host discovery

- Status: Accepted (REV/SEC reviewed)
- Date: 2026-10-26
- Related: RF-03, RF-04, RF-05, RF-09, RF-07, ADR 0001, ADR 0002

## Context

The dashboard currently learns the host origin at **build time**: Vite `define`
injects `__OWASIDEA_HOST__` from `OWASIDEA_HOST` (`ui/vite.config.ts`), and the
Tauri bundle's CSP (`examples/shipyard/desktop/tauri.conf.json`,
`connect-src ... http://127.0.0.1:8787`) is hand-written. Two consequences:

1. Changing `[host].addr` (or `HOST_ADDR` at runtime) requires rebuilding the UI
   and editing CSP by hand, or the packaged app cannot reach the host
   (`warn_csp_mismatch` in `crates/cli/src/main.rs`).
2. The desktop shell reads `HOST_ADDR` at runtime (`examples/shipyard/desktop/src/main.rs`)
   and starts the embedded host on that address, but the webview still believes
   the build-time origin. With a non-default `[host].addr` the app is silently
   offline.

RF-04 will introduce `window.__OWASIDEA__ = {host, app, version, build_id}` and
require the UI to prefer the runtime value, falling back to the build-time
`__OWASIDEA_HOST__`. RF-05 makes CSP generated from config. This ADR fixes the
mechanism by which the runtime value reaches the webview, so RF-04/RF-05/RF-09
can be implemented without inventing a transport.

Additional constraint found during review: the current `tauri.conf.json` declares
the main window declaratively (`app.windows[0]`). Tauri only applies
`initialization_script` to windows created through
`WebviewWindowBuilder`, so a declarative window cannot receive the script. The
desktop shell must create the window programmatically (see Decision).

## Validated Tauri 2 API

Installed version: `tauri = "2.12.5"` (workspace root `Cargo.lock`;
registry `~/.cargo/registry/src/.../tauri-2.12.5`).

- `WebviewWindowBuilder::initialization_script(script: impl Into<String>)`
  (`src/webview/webview_window.rs`, lines ~974-1010; backed by
  `WebviewBuilder::initialization_script` at `src/webview/mod.rs` ~943).
  Runs the JS after the global object exists but **before** the HTML document is
  parsed and before any HTML-included script. Documented example sets
  `window.__MY_CUSTOM_PROPERTY__` — exactly the shape needed for
  `window.__OWASIDEA__`.
- `WebviewWindow::eval(js)` / `eval_with_callback` (`src/webview/webview_window.rs`
  ~2576) — available for late updates, but runs after page load; not suitable
  as the primary injection point.
- Custom protocol: `tauri::Builder::register_uri_scheme_protocol` /
  `register_asynchronous_uri_scheme_protocol` (`src/app.rs` ~2267/~2301) plus the
  `custom-protocol` feature (`examples/shipyard/desktop/Cargo.toml`). Viable but heavier.
- Environment variables: Tauri does not auto-inject arbitrary env into the
  webview; the current code passes `OWASIDEA_HOST` only to Vite at build time
  (`crates/cli/src/main.rs` ~2083/~2998). Runtime env must be read in Rust and
  forwarded via one of the mechanisms above.
- Resources: `tauri::path::PathResolver::resource_dir()` resolves to the
  packaged resource directory on desktop (`src/path/desktop.rs` ~220-235), which
  the CLI already populates through `copy_runtime_resources`.

Conclusion: `initialization_script` is the lowest-friction, earliest-running,
documented mechanism for injecting `window.__OWASIDEA__` before app code runs.
No API gap requiring fallback beyond the documented `eval` retry.

## Options

| Option | Security | CSP | Determinism | Dev/bundle compat | Complexity |
|---|---|---|---|---|---|
| (a) Build-time env only (`OWASIDEA_HOST`) | Low blast radius; origin baked into JS | Static, must hand-edit | Deterministic per build | Dev works; bundle breaks on non-default addr | Low, but does not solve runtime |
| (b) Initialization script + build-time fallback | Script runs pre-parse; origin from Rust/env; JSON-serialized | Generated from same config (RF-05) | Deterministic given config + build_id | Dev keeps Vite env; bundle uses generated script | Medium |
| (c) Runtime fetch of a config file/endpoint | Adds an extra request before bootstrap; origin must be known to fetch | Still needs bootstrap origin | Depends on fetch success | Works in dev and bundle | Medium-high; bootstrap problem |
| (d) Custom protocol (`owasidea://`) serving config | Strong isolation, no origin string in JS | Protocol allow-listed in CSP | Deterministic | Requires feature + handler code; dev uses http | High |

Notes:
- (c) has a chicken-and-egg problem: the UI must know where to fetch from before
  it can fetch. It can only be a secondary/refresh mechanism, not the bootstrap.
- (d) is the most isolated but pulls in protocol registration, async responders,
  and CSP changes for a gain (no origin in JS) that (b) already achieves via
  JSON serialization + origin validation.

## Decision

Adopt **(b) initialization script injecting `window.__OWASIDEA__`, with
build-time `__OWASIDEA_HOST__` as fallback**.

Concretely:

1. Runtime metadata source: a generated file `owasidea.runtime.json` (shape:
   `{ "host", "app", "version", "build_id", "manifests_dir" }`).
   - In `dev`, the CLI writes it to a temporary path and points the desktop at
     it through `OWASIDEA_RUNTIME`; env vars `HOST_ADDR`, `OWASIDEA_APP_NAME`,
     `OWASIDEA_APP_VERSION`, `OWASIDEA_BUILD_ID`, and `OWASIDEA_MANIFESTS`
     override individual fields for quick debugging.
   - In `bundle`, the CLI generates it from `[app]` and `[host].addr` (RF-01)
     and copies it into the Tauri resource directory via `copy_runtime_resources`
     (already used for manifests and signing keys), so the installed app needs no
     environment variables.
   - `version` and `build_id` come from the same build inputs RF-07 will use
     (app version from `[app].version`, `build_id` from git sha + timestamp or
     `SOURCE_DATE_EPOCH`); the CLI computes them once per build and writes them
     to both `owasidea.runtime.json` and manifests, avoiding skew.
2. Host address transport to the installed binary: `owasidea.runtime.json` in the
   resource directory is the authoritative source; `HOST_ADDR` remains a runtime
   override for dev/debug. The binary never depends on a compile-time env var.
3. The desktop shell (`examples/shipyard/desktop/src/main.rs`) resolves metadata in this
   order: `OWASIDEA_RUNTIME` file if present, otherwise
   `app.path().resource_dir().join("owasidea.runtime.json")`, with env overrides
   applied on top. It then starts the embedded host using the resolved host
   address and builds a JSON object
   `{ "host": "http://127.0.0.1:<port>", "app": "<name>", "version": "<ver>",
      "build_id": "<id>" }`.
   The value is produced with `serde_json::to_string` (or equivalent) and
   embedded as `window.__OWASIDEA__ = <JSON>;` — **never** via string
   interpolation, so host/app/version cannot inject JS.
4. Window creation: remove the declarative `app.windows` entry from
   `tauri.conf.json` (`"windows": []`) and create the main window in `setup`
   with `tauri::WebviewWindowBuilder::new(app, "main",
   tauri::WebviewUrl::App("index.html".into()))
   .initialization_script(script).build()?`. This is the only way to attach the
   script. `WebviewUrl::App` still honors `build.devUrl` in dev mode.
5. The script is attached with the main-frame `initialization_script` (not
   `initialization_script_for_all_frames`), so it runs only on the top-level app
   document and is **not** injected into UI-plugin iframes. It is also guarded by
   a `window.location.origin` check per Tauri's documented guidance.
6. The UI reads `window.__OWASIDEA__?.host` first and falls back to the
   build-time `__OWASIDEA_HOST__` (RF-09). If neither is present, fail loudly
   with an actionable error instead of silently using a default.
7. `HOST_ADDR` / `[host].addr` is the single source of truth for the host
   origin; the CLI's bundle path generates the desktop config/env from it
   (RF-02 `config render`), and `warn_csp_mismatch` becomes a hard error in
   bundle mode (RF-05).
8. The host origin injected is validated before use by the UI: must be an
   `http://127.0.0.1:<port>` or `http://localhost:<port>` URL matching the
   configured `[host].addr`; arbitrary origins and `null`/wildcard are rejected.
   No unvalidated origin is ever stored in `window.__OWASIDEA__`.

## Security

- Serialize with `serde_json::to_string` (or equivalent) — no template strings,
  no `format!` of user-controlled values into JS.
- Guard the script with `window.location.origin` check; do not run on remote or
  unexpected origins.
- CSP `connect-src` is generated from `[host].addr` (RF-05) and must allow
  exactly that origin (plus `http://127.0.0.1:*` only in dev, per RF-05); no
  `*`, no `null`, no wildcard schemes.
- The UI validates the runtime host against an allow-list derived from config
  before issuing requests; a mismatched/tampered value is ignored and the
  build-time fallback (or an error) is used. A tampered `owasidea.runtime.json`
  cannot bypass CSP.
- Initialization script payload size is bounded (host URL + small metadata);
  no plugin data or secrets are injected.
- UI-plugin iframes keep their opaque origin and `sandbox="allow-scripts
  allow-forms"`; they do not inherit `window.__OWASIDEA__`.

## Migration

- Apps built without `owasidea.runtime.json` keep working: the desktop falls
  back to env vars and defaults, and the UI falls back to `__OWASIDEA_HOST__`
  (RF-09). No hard break for existing bundles.
- `dev`/Vite continues to set `OWASIDEA_HOST` so direct `npm run dev` works;
  RF-09 only makes a missing value a hard error outside the CLI.
- Bundles use the config-generated host value and CSP; `config render --target
  tauri --check` (RF-02) detects drift.
- `HOST_ADDR` keeps its current meaning (host bind address); the webview origin
  is derived from it, not the reverse.
- Removing the declarative window requires moving creation into `setup`; existing
  `tauri.conf.json` files with `app.windows` should be migrated by the CLI
  (RF-02 target `tauri`) or documented in the upgrade guide.

## Test criteria (RF-04 / RF-05 / RF-09)

- Change `[host].addr` to `127.0.0.1:8899`, run `owasidea build && owasidea
  bundle --bundles deb`, install, verify the app connects without a UI rebuild
  (no `OWASIDEA_HOST` change).
- Playwright smoke: with `HOST_ADDR=127.0.0.1:8899`, the dashboard's host label
  equals the runtime value and `/overview` succeeds.
- `owasidea config render --target tauri --check` fails when `tauri.conf.json`
  CSP is stale relative to `[host].addr`.
- CSP blocks a request to an origin not declared in config (Playwright network
  assertion).
- Direct `npm run build` without `OWASIDEA_HOST` (outside CLI) fails with an
  actionable error (RF-09).
- Negative: a tampered `window.__OWASIDEA__` (e.g. `host: "http://evil.example"`)
  is rejected by the UI validation and does not result in a request.
- Negative: `window.__OWASIDEA__` is `undefined` inside a UI-plugin iframe.

## Consequences

- Positive: non-default `[host].addr` works in bundled apps without hand-editing
  CSP or rebuilding UI; single source of truth for host origin; earliest
  possible injection (pre-parse) avoids flash/FOUC of wrong host.
- Negative: desktop shell must resolve `owasidea.runtime.json`, serialize
  metadata and create the window in `setup`; bundle CSP must be regenerated
  (RF-05) rather than static; declarative window configuration is no longer
  used.
- Risk: Tauri's `initialization_script` runs on all top-level navigations; the
  origin guard is mandatory to avoid re-injection on unexpected origins. Android
  remote-URL caveat (uses `onPageStarted`, not guaranteed pre-parse) is accepted
  for v1 (desktop-first).

## Alternatives rejected

- (a) Build-time env only: does not solve runtime/non-default addresses; kept
  as fallback only.
- (c) Runtime fetch: bootstrap problem; usable later as a refresh/verification
  step, not as the primary discovery.
- (d) Custom protocol: stronger isolation but disproportionate complexity and
  CSP changes for v1; revisit if origin-in-JS becomes unacceptable (e.g. for a
  stricter threat model). Recorded as future option, not implemented now.
- `eval`-after-load as primary: too late (app code may already read the host);
  retained only as a retry/fallback if `initialization_script` is unavailable on
  a platform.

## References

- Tauri 2.12.5 source:
  workspace root `Cargo.lock` (tauri 2.12.5);
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.5/src/webview/webview_window.rs`
  (`initialization_script`, `eval`, `initialization_script_for_all_frames`);
  `src/webview/mod.rs`; `src/app.rs` (`register_uri_scheme_protocol`);
  `src/path/desktop.rs` (`resource_dir`).
- Repo: `examples/shipyard/desktop/src/main.rs`, `examples/shipyard/desktop/tauri.conf.json`,
  `ui/vite.config.ts`, `crates/cli/src/main.rs` (`warn_csp_mismatch`,
  `copy_runtime_resources`, bundle `OWASIDEA_HOST`), `examples/shipyard/ui/src/main.ts`.
