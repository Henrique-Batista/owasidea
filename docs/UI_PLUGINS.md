# Webview UI plugins

UI plugins are static browser applications. They run only in the Tauri webview;
the Rust host never loads their WASM module or starts a JavaScript runtime.
Each plugin is shown in an iframe with a restrictive sandbox. All files are
hashed into its generated manifest, and `owasidea build` also writes
`ui/public/ui-plugins.json` next to the assets. The dashboard verifies against
that index, so it checks exactly the bundle it loaded.

## Scaffold and configure

```bash
owasidea plugin new dashboard --lang yew --type ui
```

Replace `plugins/dashboard/dist/` with your framework's production web output.
The output must contain an entry HTML file. Configure build and entry paths:

```toml
[plugins.dashboard]
path = "plugins/dashboard"
language = "yew"
type = "ui"
runtime = "js-wasm"
ui_entry = "index.html"
ui_build = ["trunk", "build", "--release", "--dist", "dist"]
```

`ui_build` is an argv array executed in the plugin directory (no shell). Omit it
when build output already exists. `owasidea build` copies the static output to
`ui/public/plugins/<name>/` and generates a `js-wasm` manifest with per-file and
bundle SHA-256 digests.

## Supported WASM web frameworks

All examples use the same static bundle contract; adjust output paths to match
your framework/toolchain version.

### Svelte / React

Use each framework's Vite build and set `base: './'` so assets resolve inside the iframe:

```toml
language = "svelte"
ui_build = ["npm", "run", "build"]
ui_entry = "index.html"
```

```toml
language = "react"
ui_build = ["npm", "run", "build"]
ui_entry = "index.html"
```

### Yew

Build with Trunk and point `ui_entry` at `index.html`:

```toml
language = "yew"
ui_build = ["trunk", "build", "--release", "--dist", "dist"]
ui_entry = "index.html"
```

### Blazor WebAssembly

Publish the browser app, then set `ui_entry` to the generated `wwwroot/index.html`
if the SDK preserves that directory in the publish output:

```toml
language = "blazor"
ui_build = ["dotnet", "publish", "-c", "Release", "-o", "dist"]
ui_entry = "wwwroot/index.html"
```

### Uno Platform WebAssembly

Publish the browser target and point `ui_entry` at its generated HTML. Use the
target framework and output directory produced by your Uno template:

```toml
language = "uno"
ui_build = ["dotnet", "publish", "-c", "Release", "-o", "dist"]
ui_entry = "wwwroot/index.html"
```

## Runtime contract

- UI manifests use `runtime = "js-wasm"` and `type = "ui"`.
- UI plugins are listed by `GET /ui-plugins` and by the bundled
  `ui-plugins.json`; headless plugins remain in `GET /plugins`.
- The dashboard checks every listed asset and the aggregate digest against
  `ui-plugins.json` before creating the iframe. Signed manifests remain the
  source of truth for headless plugins and registry installs.
- The iframe uses `sandbox="allow-scripts allow-forms"`; it does not receive
  same-origin, top-navigation, or popup privileges.
- UI plugins do not receive WASI, host KV/log, filesystem, socket, or native host
  capabilities. The iframe stays fully isolated unless the plugin opts into the
  least-privilege parent-to-plugin bridge (ADR-0004, CT-05/CT-06/CT-07): only the
  exports listed in `bridge` are callable, and everything else is denied.
- UI assets are included in local desktop bundles; remote registry entries list
  each file separately and verify it against the same manifest digest map.

For registry signing and publishing, see [REGISTRY.md](REGISTRY.md).

## Working examples

The Shipyard reference app ships two complete isolated UI plugins:

- `examples/shipyard/plugins/board-svelte` — Svelte 4 + Vite, local state only.
- `examples/shipyard/plugins/map-react` — React 18 + Vite, interactive map.

Each has its own `package.json`, Vite config with `base: './'`, and a static
`dist/index.html`. The dashboard in `examples/shipyard/ui/src/main.ts` reads
`ui-plugins.json`, verifies every file and the aggregate digest, and only then
creates the iframe.

Build them through the framework:

```bash
cd examples/shipyard
owasidea build
```

`owasidea build` runs `ui_build` in each plugin and copies `dist/` into
`ui/public/plugins/<name>/`. It refuses to overwrite an unowned output directory;
remove a manually created directory or let the framework own it.

## Packaging caveat

`ui/public/plugins/` is copied into `ui/dist` by Vite, so the Tauri bundle
contains the UI plugin assets automatically. But the generated UI manifests in
`manifests/` contain per-file hashes. Rebuild **both** the UI plugins and the
Tauri app after changing any UI plugin asset, or the dashboard will reject the
iframe with an integrity error.

## Isolation model

- The iframe uses `sandbox="allow-scripts allow-forms"`; it has an opaque origin
  and cannot read the parent DOM, cookies, or storage.
- Opt-in bridge: set `bridge = ["plugin.export"]` in `[plugins.<name>]`. `owasidea build`
  validates each entry and writes `metadata.ui.bridge` (sorted, dedup). Parent creates
  a `MessageChannel` only for UI plugins with non-empty bridge; protocol in ADR-0004.
- UI plugins receive no WASI, KV, filesystem, socket, or native capabilities.
- Keep each plugin's bundle self-contained: relative asset URLs (`base: './'`)
  and no cross-origin runtime fetches unless the Tauri CSP allows them.

## Generic invocation forms (CT-04)

The dashboard derives one form per WIT export listed in a plugin manifest, ignoring
entries whose `signature` is `interface`. Each declared `param` becomes a field:

- `bool` -> checkbox
- integer/float types -> number input
- `string` -> text input
- `list`, `option`, `result`, `tuple`, `record`, `enum`, `flags`, `variant` -> textarea
  expecting a JSON value, with the WIT type shown as a hint

Fields are labelled with the parameter `name`. The Invoke button validates arity and
client-side types, calls `POST /plugin/{name}/call`, and shows the returned
`result`/`results`, HTTP status/body on error, and latency in milliseconds.
fetch and curl snippets are generated from the submitted arguments.

## Dev reload (DL-08)

During `owasidea dev`, changes under `plugins/<name>/dist/**` (or `plugins/<name>/index.html`)
are detected by the watcher. The CLI rebuilds that UI plugin's assets/index entry,
regenerates `ui/public/ui-plugins.json`, and the UI polls the index every ~1.5s;
when the version/hash changes, the dashboard re-verifies the assets and mounts a
**new** iframe with a fresh bridge nonce (the live iframe is never re-armed).
Production (no watcher) keeps the existing behavior.

## Bridge SDK (CT-07)

UI plugins that declare a `bridge` allowlist can call allowlisted backend
functions through the parent-mediated MessageChannel bridge. The SDK is shipped
same-origin with the bundle at `./owasidea-bridge.js` (copied by
`owasidea build` from `ui/public/owasidea-bridge.js`).

### Configure

```toml
[plugins.board-svelte]
...
bridge = ["changelog-ts.render"]
```

### Usage (Svelte/TS)

```ts
const mod: any = await import(/* @vite-ignore */ './owasidea-bridge.js');
const bridge = await mod.connect(); // handshake; rejects BRIDGE_DISABLED after 5s
const result = await bridge.call('changelog-ts', 'render', ['v1.0.0', ['feat: add']]);
// or: bridge.call('changelog-ts.render', ['v1.0.0', ['feat: add']])
```

`connect()` resolves with `{ call, cancel }`. `call()` returns a Promise that
resolves with `result`/`results` or rejects with `BridgeError { code, message }`.
Use `bridge.cancel(id)` when you retain the request id.

The SDK must be the copy produced by `owasidea build` (it is hashed with the
bundle). The dashboard mounts the iframe with a one-time
`?__owasidea_bridge=<nonce>` parameter and only transfers a `MessagePort` after
the SDK echoes that nonce in a `ready` control message from the iframe's own
window. Bundles built before this handshake existed stay isolated and
`connect()` rejects with `BRIDGE_DISABLED` until they are rebuilt.

A plugin that navigates away (reload, `location.href`, redirect) loses the
bridge permanently: the dashboard closes the port, cancels in-flight calls and
never re-sends `init` to the new document. If you need a reload, rebuild the
plugin so the dashboard swaps the iframe.

Bridge timeout: the dashboard uses 10 s by default, or the manifest's
`permissions.resources.timeout_ms` / `metadata.ui.timeout_ms` when present,
capped at 30 s.

### Allowlist

Only entries listed in `bridge` (and validated at build time against the
headless plugin manifest/exports) are callable. Calls to other functions return
`NOT_ALLOWED`. Manifests without `bridge` behave as before (fully isolated).

### Streaming jobs

The experimental long-call jobs API (ADR-0005, feature `streaming`) is
host-only: the bridge remains request/response and does not expose job events or
cancellation to UI plugins. A UI plugin that needs progress must use its own
allowlisted request/response call until the bridge protocol gains streaming.
