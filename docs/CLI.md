# CLI Reference

Binary: `owasidea`. Commands that produce structured output support `--json`.

## Global flags

- `--config <path>` (default `wasm-app.toml`)
- `--manifest-dir <path>` (default `manifests`)
- `--http-addr <addr>` (overrides `[host].addr`; default `127.0.0.1:0` for CLI
  commands, `127.0.0.1:8787` for a configured app)
- `--json`

`--config` defines the app root. Relative `--manifest-dir`, plugin `path`, UI
`ui`, and desktop `path` values resolve against the config file's directory. This
allows:

```bash
owasidea --config examples/shipyard/wasm-app.toml build
owasidea --config examples/shipyard/wasm-app.toml inspect core-rust --json
```

from any working directory.

## Commands

### `init <dir>`

Scaffold an app directory with `wasm-app.toml`, `plugins/`, `manifests/`, `ui/`,
and `docs/`. The UI files are the generic dashboard: `index.html`, `main.ts`,
`vite.config.ts`, `tsconfig.json`, and `package.json`.

### `doctor [--json] [--install-wasi-sdk]`

Check required tooling (rustc, cargo, cargo-component, wasm-tools, go, node, npm)
and optional per-language tools (clang, dotnet, jco, componentize-dotnet, trunk,
python3). It also reports wasi-sdk and the WASI preview1 reactor adapter. Exits
non-zero when a required tool is missing.

`--install-wasi-sdk` downloads wasi-sdk 34 to `~/.local/wasi-sdk` (or
`WASI_SDK_DIR`) and copies the reactor adapter when available. On Windows it
downloads the Windows archive and detects `%USERPROFILE%\.local\wasi-sdk`
plus `clang.exe`.

### `plugin new <name> --lang <language> [--type library|executable|service|ui] [--runtime wasi-component|js-wasm]`

Scaffold a plugin under `<app-root>/plugins/<name>` and register it in
`wasm-app.toml`.

Supported languages:

- `rust`, `go`, `typescript`, `javascript`, `csharp`, `c`, `cpp`
- `yew`, `blazor`, `uno`, `svelte`, `react` (UI only; `--type ui --runtime js-wasm`)

UI plugins write a minimal static `dist/index.html`. Stateful service scaffolding
currently exists for Rust. TypeScript/UI plugins need `npm install` in the
plugin directory before build.

### `build [--no-sign]`

Build configured plugins, compose clients declared with `compose_with`,
regenerate manifests from the real WIT, sign when configured, and copy UI
assets.

Behavior:

1. Ordinary plugins build before composed plugins.
2. Missing npm dependencies for the dashboard, TypeScript plugins and UI plugins are installed
   automatically.
3. C/C++ builds auto-detect wasi-sdk and the reactor adapter, then inject
   `WASI_SDK_PATH`/`WASI_PREVIEW1_ADAPTER` into `make` (or the per-plugin
   `build` override).
4. A plugin with `compose_with = ["provider"]` builds as a client, then
   `wasm-tools compose` links the provider components into
   `<name>.component.wasm`.
5. `wasm-tools component wit --json` derives imports/exports; the generated
   manifest includes a SHA-256 checksum.
6. A component importing `poc:host/host-kv` without a matching `kv` permission
   fails the build.
7. UI plugins (`runtime = "js-wasm"`, `type = "ui"`) run `ui_build`, copy
   `dist/` to `ui/public/plugins/<name>/`, and hash every file. Their digests
   are also written to `ui/public/ui-plugins.json`, so the dashboard verifies
   the bundle it actually loaded instead of a manifest that may be newer than
   the running desktop binary. The index is a build-consistency artifact;
   manifests remain the signed source of truth for headless plugins.
8. If `[signing]` exists, every manifest is signed with the configured Ed25519
   key. `generate = true` is opt-in and creates a demo key when missing; the
   example app does not enable it, so run `owasidea keygen --out keys` first.

A C/C++ plugin can override the native build command, which is required on
Windows when `make.exe` is not on `PATH`:

```toml
[plugins.fingerprint-c]
path = "plugins/fingerprint-c"
language = "c"
type = "library"
build = ["C:/msys64/usr/bin/make.exe"]
```

Example composition configuration:

```toml
[plugins.pipeline-go]
path = "plugins/pipeline-go"
language = "go"
type = "library"
compose_with = ["core-rust"]
kv = "shipyard-pipeline:read-write"
fs = ["reports:read-write"]
persistent = true
```

Build outputs must be named `<plugin-name>.component.wasm`. C/C++ Makefiles must
honor that convention (the WIT world name is not enough).

### `test [--no-build] [--no-smoke] [--no-ui] [--generate] [--force] [--filter <substr>] [--plugin <name>] [--watch]`

Run the full framework test flow:

1. `build` unless `--no-build`.
2. Start a temporary host on an ephemeral port with a temporary
   `OWASIDEA_DATA_DIR` and signing environment.
3. Run generic checks (health, all headless plugins loaded, UI plugin count) and
   the declarative checks under `[smoke]` in `wasm-app.toml`.
4. Stop the host.
5. Run `[test].ui` as an argv array in the app root, with `OWASIDEA_BIN` pointing
   at the current CLI. The example uses Playwright.

`--filter` matches a substring of plugin/function in smoke calls, HTTP checks and
services; `--plugin` restricts the run to one plugin; `--watch` re-runs when
manifests, config or plugin outputs change.

### `smoke [--no-build]`

Build and run only the API smoke checks (steps 1–4 above). Useful in CI.

### `test --generate [--force]`

Generate a separate `smoke.generated.toml` (and `tests/smoke.generated.toml`
when a `tests/` directory exists) from each plugin manifest's exports, using
type-default argument values. Never modifies `wasm-app.toml`. Fails if the
target file exists unless `--force` is given.

### `validate [--json]`

Check generated manifests against `wasm-app.toml`:

- manifest exists
- component checksum matches
- filesystem/KV/HTTP permissions match the config
- UI manifests declare `runtime = "js-wasm"` and `type = "ui"`
- manifest ABI fields (`schema_version`, `host_abi`, `min_host`) are compatible
  with the host (see `docs/ABI.md`)

Deprecated-but-working manifests produce warnings instead of errors (one line
per warning; wrapped here for readability):

```text
warning: notes: ABI_LEGACY_MANIFEST: deprecated legacy manifest (no integer
schema_version field); host ABI 1 still accepts it with defaults, but a future
host ABI will reject it. Regenerate with `owasidea build`
(see docs/ABI_POLICY.md).
warning: notes: ABI_MISSING_HOST_FIELDS: manifest has no host_abi or min_host;
the host applies defaults. Regenerate with `owasidea build`
(see docs/ABI_POLICY.md).
```

Warning codes (`ABI_LEGACY_MANIFEST`, `ABI_MISSING_HOST_FIELDS`) are stable;
match the code, not the prose. Warnings go to stdout and do **not** change the
exit status: the last line is `Validation OK with N deprecation warning(s)` and
the command exits `0`. Incompatibilities the host cannot honour (unknown
`schema_version`, `host_abi` mismatch, `min_host` newer than the host) are
errors.

Exits non-zero on drift. Run after `build`, especially in CI. Policy and removal
window: `docs/ABI_POLICY.md`; 0.x upgrade: `docs/MIGRATION_0_X_TO_1_0.md`.

#### `validate --json`

Emits one JSON object instead of the text lines. The shape is stable and is the
contract for CI/agents:

```json
{
  "ok": false,
  "plugins": [
    {
      "name": "notes",
      "ok": false,
      "errors": ["CHECKSUM MISMATCH"],
      "warnings": ["ABI_LEGACY_MANIFEST: deprecated legacy manifest (...)"]
    }
  ],
  "errors": ["notes: CHECKSUM MISMATCH"],
  "warnings": ["notes: ABI_LEGACY_MANIFEST: deprecated legacy manifest (...)"]
}
```

- `ok` — `true` only when `errors` is empty; warnings never clear it.
- `plugins[]` — one entry per `[plugins.<name>]`, in config order, with
  `ok = errors.is_empty()`. `errors`/`warnings` hold unprefixed messages.
- `errors`/`warnings` — flattened view over all plugins, each prefixed with
  `"<plugin>: "`. Config-level failures (unreadable/invalid `wasm-app.toml`)
  appear here unprefixed with `plugins: []`.
- A missing manifest is reported as `MISSING manifest: <path>`; an unreadable or
  malformed manifest is reported as `cannot read manifest: <detail>`.
- Human-only context lines (`checksum ok`, `UI manifest ok`, `no checksum in
  manifest`) are not part of the JSON.
- JSON is written to stdout even when validation fails; the exit code is
  non-zero and the reason also goes to stderr.
- Object key order is not significant (`serde_json` emits sorted keys); match
  fields by name.


### `verify <dist-dir> [--public-key <file>] [--allow-dist-trust] [--json]`

Verify a dist directory produced by `bundle` (or any directory containing
manifests). `bundle` stages the runtime payload into the output directory
(`manifests/`, `plugins/<name>/<name>.component.wasm`, `ui/public/` and
`trust/owasidea.pub`), so verifying `dist/` checks what the installers ship.
Checks:

- each manifest is valid JSON with compatible ABI fields (`schema_version`,
  `host_abi`, `min_host`)
- referenced component exists and its sha256 matches `metadata.sha256`
- Ed25519 signature verifies against a **pinned** public key
- `metadata.ui` asset digests and bundle digest for UI plugins
- `ui-plugins.json` index entries match the manifests

The trust root must be supplied out of band: `--public-key <file>` or
`OWASIDEA_PUBLIC_KEY`. A key copied into `dist/trust` (or `dist/keys`) by the
same bundle is **not** accepted by default, because whoever can rewrite the
artifacts can rewrite that key too; `verify` fails with `TRUST_ROOT_UNPINNED`.
`--allow-dist-trust` opts in for local smoke checks only. With no key at all it
fails with `TRUST_ROOT_MISSING`.

A directory with no `*.manifest.json` fails with `NO_MANIFESTS`; verification
never reports success for zero manifests.

`--json` prints a stable report `{ok, errors:[{file,code,message}], checked}`.
Exits non-zero if any check fails.

### `config render [--target runtime|tauri|playwright|all] [--check]`

Render derived configuration artifacts from `wasm-app.toml` (single source of
truth for host/port/CSP/dev URL):

- `runtime` -> `<ui>/.owasidea/runtime.json`
- `playwright` -> `<ui>/.owasidea/playwright.json`
- `tauri` -> `<desktop>/tauri.generated.conf.json` (copies base `tauri.conf.json`,
  overriding `build.devUrl`, ensuring the host origin is in `connect-src`, and
  generating `bundle.resources` for `manifests/`, every headless component,
  `trust/owasidea.pub` and `owasidea.runtime.json`).
  The generated CSP contains exactly the configured host origin (no wildcard);
  `*` and `null` are rejected. `bundle` always renders this file and passes it
  to `cargo tauri build --config`.

`--check` does not write; exits non-zero if files are missing or stale.

### `dev [--no-build] [--desktop] [--no-tui]`

Build first, then start the host, the Vite dev server (if
`<app-root>/ui/package.json` exists), and a recursive watcher. The UI port comes
from `[dev].ui_addr` when set; otherwise the first free port in 5173..=5273.
A busy configured port falls back to the next free port. `dev` also writes
`<desktop>/tauri.dev.conf.json` (merged from the base `tauri.conf.json`, with
`build.devUrl` set to the reserved UI URL) for `dev --desktop` (DL-01). In dev
mode the generated CSP additionally allows `http://127.0.0.1:*` (wildcard port)
alongside the exact host origin. Prints:

- UI URL
- host URL
- loaded plugin names
- watch roots
- Tauri dev config path and devUrl

With `--desktop`, the CLI also runs `cargo tauri dev --config <desktop>/tauri.dev.conf.json` in the resolved desktop directory, with `OWASIDEA_HOST`, `HOST_ADDR`, `OWASIDEA_MANIFESTS` (and `OWASIDEA_RUNTIME` when set) in the environment. The host and Vite remain managed by the CLI; Tauri only loads the reserved dev URL. Logs are prefixed `[host]`, `[ui]`, `[tauri]`; Ctrl+C stops all three. If `cargo tauri` is missing, the command fails with an actionable install hint.

The watcher hot-reloads stateless components by calling
`POST /plugin/{name}/reload`. Stateful services require an explicit stop/reload;
reload while running returns HTTP `409`. Source changes that require compilation
print a message and require `owasidea build`.

In an interactive terminal `dev` opens the **dashboard** (see below). The host
and Vite run as child processes and their output is captured, so nothing is
printed over the screen. Use `--no-tui` for the plain log, or run it without a
TTY (CI, pipes, `--json`): the plain log is printed automatically. `--desktop`
always uses the plain log.

### `tui`

Open the interactive dashboard for a host that is already running (for example
`owasidea run`). The host address comes from `--http-addr`, then `[host].addr`,
then `127.0.0.1:8787`. The command refuses to start without a terminal and
points to `status --watch` or `--json` for scripts.

The dashboard is a client of the host HTTP API and never links Wasmtime. It
shows the host state, build id, watch/hot-reload flags, the plugin list with
type, language, runtime and service state, and five views for the selected
plugin:

| View | Shows |
| --- | --- |
| Overview | type, language, runtime, served path, exports, service address/restarts/last error, the last reloads |
| Manifest | the generated manifest (JSON), scrollable with PgUp/PgDn |
| Call | the callable exports; `←`/`→` choose one, `Enter` edits the args as a JSON array, `Enter` runs it, `Esc` stops editing; the result is shown below |
| Logs | the plugin's ring-buffer log, newest at the bottom |
| Output | build, host, UI, watcher and reload messages (`dev` only) |

Keys (press `?` in the dashboard to see them):

| Key | Action |
| --- | --- |
| `↑` / `↓`, `j` / `k` | select plugin |
| `Tab`, `Shift+Tab`, `1`-`5` | switch view |
| `r` | reload the selected plugin (`POST /plugin/{name}/reload`) |
| `s` | start or stop the selected service |
| `R` | rebuild the app (`owasidea build`), then reload the stateless plugins |
| `o` | open the UI (or the host) in the browser |
| `?` | toggle help |
| `q`, `Ctrl+C` | quit; `dev` also stops the host and the UI |

In `dev`, `R` rebuilds through the same CLI and streams its output to the Output
view. A running service still refuses reload with HTTP 409; stop it first.

### `run`

Run the current manifests without building or watching. The host address comes
from `--http-addr`, then `[host].addr`, then `127.0.0.1:0`. Signing environment
from `[signing]` is applied automatically. The host refuses non-loopback bind
addresses unless `OWASIDEA_ALLOW_NON_LOOPBACK=1` is set (the standalone host
binary also accepts `--allow-non-loopback`).

### `inspect <plugin> [--json]`

Show the generated manifest, and for headless plugins the formatted WIT.
`--json` prints the raw manifest.

### `call <plugin>.<function> [--args <json|@file|->] [--json] [--timeout <secs>]`

Call a plugin export over the host HTTP API. `--args` accepts a JSON array
inline, `@file` to read from a file, or `-` for stdin (defaults to `[]`).
`--json` prints the raw response; otherwise pretty-prints it. `--timeout`
sets the request timeout in seconds (default 30). The host address comes from
`--http-addr`, then `[host].addr`, then `127.0.0.1:8787`. The host is not
started by this command.

Examples:

```sh
owasidea call core-rust.ping
owasidea call core-rust.score --args '[["feat"],9,10]'
owasidea call core-rust.echo --args @args.json
echo '[1,2]' | owasidea call core-rust.sum --args -
```

### `client --lang ts -o <file> [--check]`

Generate a typed TypeScript client from the manifests in `--manifest-dir`.
Only `ts` is supported. `--check` fails if the file is missing or differs
without writing.

### `bundle [--no-build] [--bundles <targets>] [--profile <dev|staging|prod>] [--isolate-host-libs]`

Build and sign, build the UI, and produce a Tauri desktop bundle.

Resolution:

- UI directory: `[desktop].ui` or `ui`
- Desktop crate: `[desktop].path`, then `desktop/`, then `crates/desktop/`

```toml
[desktop]
path = "desktop"
ui = "ui"
```

It runs `npm run build` in the UI directory, then `cargo tauri build` in the
desktop crate. With `[signing]`, manifests are signed during build; the desktop
shell must set `OWASIDEA_REQUIRE_SIGNATURES=true` and resolve
`OWASIDEA_PUBLIC_KEY` from its bundled `trust/owasidea.pub`. `bundle` renders
`bundle.resources` from `wasm-app.toml` (manifests, every headless component,
the trust key and `runtime.json`), so a new plugin does not need a manual entry;
explicit entries in the base `tauri.conf.json` are preserved. The webview UI is
embedded through `build.frontendDist`; the UI build output is only staged as a
resource when the shell has no frontend directory. See
`examples/shipyard/desktop`.

Direct UI builds outside the CLI (`npm run build` / `vite`) now fail with an
actionable error if `OWASIDEA_HOST` is not set. Use `owasidea build`/`owasidea dev`
or export `OWASIDEA_HOST=http://127.0.0.1:8787` before building.

By default the Tauri config decides the targets. Shipyard uses `"all"`:

```toml
[desktop]
path = "desktop"
ui = "ui"
output = "dist"

# optional platform override: tauri.windows.conf.json
```

Choose targets explicitly with `--bundles`:

```bash
owasidea bundle --bundles deb
owasidea bundle --bundles deb,rpm,appimage
owasidea bundle --bundles nsis,msi   # Windows
```

If one target fails after others were produced (for example AppImage needs a
working `linuxdeploy`/FUSE environment), the CLI still copies the successful
artifacts to `dist/` and exits non-zero with the failed target summary. Use
`--bundles deb,rpm` to skip AppImage.

For AppImage, the CLI sets `APPIMAGE_EXTRACT_AND_RUN=1` and `NO_STRIP=1` to
avoid FUSE and strip failures. On hosts with vendor-private libraries under
`/usr/lib` (VMware Workstation installs `libgdk_pixbuf` and `libcroco` there),
linuxdeploy picks the wrong copy and fails. Hide those directories from the
bundler instead of exposing them to the whole system:

```bash
owasidea bundle --bundles appimage --isolate-host-libs
```

`--isolate-host-libs` runs `cargo tauri build` under
[bubblewrap](https://github.com/containers/bubblewrap) with `/usr/lib/vmware`
and `/usr/lib64/vmware` replaced by empty tmpfs mounts. It is Linux-only, opt-in,
needs no root, and does not modify the host. The CLI prints every hidden
directory. Add more with `--mask-path <dir>` (repeatable). Requires `bwrap`
(`pacman -S bubblewrap`, `apt install bubblewrap`).

For a fully hermetic release, build AppImage in a clean CI runner or chroot
instead; see [`APPIMAGE.md`](APPIMAGE.md) and
[`CLEAN_BUILD.md`](CLEAN_BUILD.md). `deb`/`rpm` do not use the GTK linuxdeploy
plugin.

On Windows Tauri loads `tauri.windows.conf.json`, so the Shipyard example builds
the configured Windows targets (default `"all"`) and copies the `.exe`/`.msi`
to the configured output directory. Cross-compiling a Windows installer from
Linux is not wired; use a Windows host or CI runner.

When `[desktop].output` is set, bundle artifacts are copied there. Example:

```toml
[desktop]
path = "desktop"
ui = "ui"
output = "dist"
```

#### Bundle SBOM and provenance

After copying the artifacts, `bundle` writes `sbom.cdx.json` and
`sbom.spdx.json` into the output directory (RL-04). Add `--provenance` to also
write `provenance.intoto.json`, an unsigned SLSA-like attestation with the
build id, git commit, `SOURCE_DATE_EPOCH` and SHA-256 hashes of every artifact.
Use `--no-sbom` to skip both files. See [`RELEASE.md`](RELEASE.md#41-sbom-and-provenance-rl-04).

#### Bundle updater artifacts

With `[updater]` configured (endpoint + public key), `bundle` also:

- renders `plugins.updater` (endpoints + public key) into
  `tauri.generated.conf.json`;
- sets `bundle.createUpdaterArtifacts = true` when
  `TAURI_SIGNING_PRIVATE_KEY` is present (the Tauri CLI then writes `.sig`
  files next to the updater artifacts);
- scans the copied artifacts and writes `latest.json` next to them.

Without a signing key, artifacts are unsigned and `latest.json` is skipped with
a message; the bundle itself still succeeds. See [`UPDATER.md`](UPDATER.md).

#### Bundle profiles

`bundle --profile dev|staging|prod` merges a per-profile overlay over
`wasm-app.toml` without editing the base file:

```text
wasm-app.toml            # base config (never rewritten)
wasm-app.staging.toml    # optional overlay for the staging profile
```

Merge is deterministic: tables merge recursively, every other value (scalars and
arrays) from the overlay replaces the base value. `plugins.<name>` merges per
plugin, so an overlay can add capabilities without repeating the plugin's
`path`/`language`/`type`.

```toml
# wasm-app.staging.toml
[app]
version = "1.0.0-rc.1"
identifier = "dev.owasidea.shipyard.staging"

[host]
addr = "127.0.0.1:9100"

[plugins.webhook-go]
http_outbound = ["https://staging.example"]
```

The merged config drives the whole bundle:

- `[host].addr` → `OWASIDEA_HOST` for the UI build and `connect-src` in the
  generated CSP (`desktop/tauri.generated.conf.json`);
- `[app].name`/`version`/`identifier` → Tauri `productName`/`version`/
  `identifier` (profile bundles only; without `--profile` the base
  `tauri.conf.json` identity is kept);
- plugins and `[app].version` used by `owasidea build`;
- `ui/.owasidea/runtime.<profile>.json` → `owasidea.runtime.json` in the bundle
  resources (the base `ui/.owasidea/runtime.json` used by `dev`/`run` is left
  untouched);
- artifacts go to `<desktop.output>/<profile>` (`dist/<profile>` when
  `[desktop].output` is unset), so `dev`, `staging` and `prod` never overwrite
  each other. The directory also receives `owasidea.bundle.json` (effective
  profile, host, identity, plugin set, overlay used) and the generated
  `tauri.generated.conf.json`.

A profile without an overlay file uses the base config unchanged; the CLI
prints which case applied:

```text
Bundle profile: staging (overlay: /repo/wasm-app.staging.toml)
Bundle profile: prod (no overlay /repo/wasm-app.prod.toml; using base config)
```

Only `dev`, `staging` and `prod` are accepted. Any other name fails before the
build starts:

```text
error: unknown bundle profile "qa"; expected one of: dev, staging, prod
```

```bash
owasidea bundle --profile staging --bundles deb
owasidea bundle --profile prod --bundles deb,rpm,appimage
owasidea bundle --profile dev --no-build
```

### `latest [--artifacts <dir>] [--check] [--channel stable|beta] [--profile <name>]`

Generate the Tauri updater manifest (`latest.json`) from signed bundle
artifacts, or validate an existing one with `--check`.

- Artifacts: `--artifacts`, else `<desktop.output>` / `dist` / `dist/<profile>`.
- Scans `*.AppImage.sig`, `*.exe.sig`, `*.msi.sig`, `*.app.tar.gz.sig`;
  `deb`/`rpm`/`dmg` are not updater targets.
- Platform keys: `linux-x86_64`, `windows-x86_64`, `darwin-x86_64`,
  `darwin-aarch64` (arch inferred from the file name).
- URL base: `--url-base`, else `OWASIDEA_UPDATER_BASE_URL`, else the parent of
  the first endpoint.
- Channel: `--channel`, else `OWASIDEA_UPDATE_CHANNEL`, else
  `[updater].channel`, else `stable`.
- Version: `--version`, else `[app].version`.
- Notes: `--notes`, `--notes-file`, else `[updater].notes` / `notes_file`.
- `pub_date`: `--pub-date` (RFC 3339), else now (UTC).
- `--check` validates the existing file (semver, RFC 3339 date, non-empty
  platforms, HTTPS URL + signature per platform, channel match) and never
  writes. Exits non-zero on drift.

```bash
owasidea latest --artifacts dist --url-base https://releases.example.com/stable
owasidea latest --artifacts dist --channel beta --version 1.1.0 --notes-file NOTES.md
owasidea latest --artifacts dist --check
```

Requires `[updater].endpoint` and `[updater].pubkey` (or
`OWASIDEA_UPDATER_ENDPOINT`/`OWASIDEA_UPDATER_PUBKEY`). See
[`UPDATER.md`](UPDATER.md) and `scripts/gen-latest-json.sh`.

### `sbom [--output <path>] [--format cyclonedx|spdx] [--provenance] [--artifacts <dir>] [--check] [--profile <name>]`

Generate a software bill of materials from the lockfiles in the app tree
(RL-04):

- `Cargo.lock` (root, desktop, Rust plugins) → `pkg:cargo/...`
- `package-lock.json` (root, UI, JS/TS plugins) → `pkg:npm/...`
- `go.mod` + `go.sum` (Go plugins) → `pkg:golang/...`

The default output is `<dist>/sbom.cdx.json` (CycloneDX 1.5); `--format spdx`
writes SPDX 2.3 JSON. Components are sorted by purl, hashes from the lockfiles
are included, and the serial number/namespace is content-derived, so two runs
with the same `SOURCE_DATE_EPOCH` produce byte-identical documents. Without
`SOURCE_DATE_EPOCH` the CycloneDX timestamp is omitted.

`--provenance` writes `provenance.intoto.json` next to the SBOM: an in-toto
statement (SLSA provenance v1 shape) with the build id, git commit, epoch and
SHA-256 hashes of the files under `--artifacts` (default: the SBOM directory).
`--check` regenerates in memory and fails when the existing file is stale.
`--json` prints a machine-readable summary.

```bash
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) \
  owasidea sbom --output dist/sbom.cdx.json --provenance --artifacts dist
owasidea sbom --output dist/sbom.spdx.json --format spdx
owasidea sbom --check
```

### `signing plan [--platform windows|macos|linux] [--require] [--json]`

Read-only code-signing planner (RL-03). It reports `mode` (`skip`,
`windows-pfx`, `windows-trusted-signing`, `macos-developer-id`, `invalid`),
whether the bundle will sign/notarize, and the **names** of missing variables.
Secret values are never printed or serialized. `--require` exits non-zero when
nothing is configured; an `invalid` plan always exits non-zero. The default
platform is the current host. See [`CODE_SIGNING.md`](CODE_SIGNING.md).

### `wit generate [path]`

Regenerate WIT from source annotations (`owasidea::*` in Rust, `// owasidea:`
in Go/C/C++, JSDoc in TypeScript, attributes in C#). `build` calls this per
plugin. See `docs/SOURCE_GENERATORS.md`.

### `[signing]` configuration

```toml
[signing]
key = "keys/owasidea.key"
public_key = "keys/owasidea.pub"
require = true
```

`build` and `bundle` sign every manifest. `run`, `dev` and `test` apply
`OWASIDEA_PUBLIC_KEY` and `OWASIDEA_REQUIRE_SIGNATURES` before starting the host.
Create the keypair explicitly with `owasidea keygen --out keys`; the optional
`generate = true` flag is a demo-only convenience that creates a key when
missing. Release builds should point `key` at a protected CI secret and keep
`generate` off, so a missing key fails instead of producing a new trust root.
Keep private keys out of the repository in production; publish the public key
out of band (bundled `trust/owasidea.pub` or `--public-key` for `verify`).

### `[updater]` configuration

```toml
[updater]
channel = "stable"                                    # stable | beta
endpoint = "https://releases.example.com/{{channel}}/latest.json"
pubkey = "keys/owasidea-updater.pub"                  # public key file
notes_file = "docs/RELEASE_NOTES.md"                  # optional
```

`endpoint` may use `{{channel}}`, `{{target}}`, `{{arch}}` and
`{{current_version}}`; a trailing `/` means `<endpoint>/latest.json`. HTTPS is
required except for loopback hosts. `pubkey` is the minisign **public** key
whose content is embedded in `tauri.generated.conf.json`.

Environment overrides: `OWASIDEA_UPDATE_CHANNEL`, `OWASIDEA_UPDATER_ENDPOINT`,
`OWASIDEA_UPDATER_PUBKEY`, `OWASIDEA_UPDATER_BASE_URL`. The private signing key
(`TAURI_SIGNING_PRIVATE_KEY`) is only read by the Tauri CLI and is never
rendered into a file. See [`UPDATER.md`](UPDATER.md).

### `keygen --out <dir>` / `sign <plugin> --key <key>`

Generate an Ed25519 keypair and sign a release manifest plus component bytes.
Private keys are written mode `0600` on Unix. See `docs/REGISTRY.md` and
`docs/SECURITY.md`.

### `registry search <query> --url <index.json> [--json]`

Search a static HTTPS registry index. HTTP is allowed only for localhost tests.

### `registry install <name> --version <version> --url <index.json> --public-key <key> [--force] [--locked]`

Download and verify one signed plugin version. Verification happens before
writing. Unsigned, untrusted, malformed, over-sized, cross-origin, and
path-traversal packages are rejected.

### `registry update [--name <name>] [--version <version>] --url <index.json> --public-key <key>`

Consult the registry and update `plugins.lock` entries (lock only).

### `registry outdated --url <index.json>`

List lock entries whose version differs from the registry.

Download and verify one signed plugin version. Verification happens before
writing. Unsigned, untrusted, malformed, over-sized, cross-origin, and
path-traversal packages are rejected.

## `[smoke]` configuration

Smoke checks are declarative data, executed by the framework:

```toml
[[smoke.calls]]
plugin = "core-rust"
function = "score"
args = [["feat: add"], 9, 10]
min = 0
max = 100

[[smoke.calls]]
plugin = "policy-csharp"
function = "next-version"
args = ["v1.2.3", "minor"]
expect_equals = "v1.3.0"

[[smoke.http]]
path = "/app/webhook-go/health"
expect_contains = "shipyard-webhook"

[[smoke.services]]
plugin = "watchtower-rust"
```

`test`/`smoke` also check `/health`, every configured headless plugin, and the
number of UI plugins before running these entries.

## `[test]` configuration

```toml
[test]
ui = ["npx", "playwright", "test", "--config", "playwright.config.ts"]
```

The command is an argv array, not a shell. It runs in the app root with
`OWASIDEA_BIN` pointing at the current CLI.

## Exit codes

- `0` success
- non-zero build/validation/toolchain error
- non-zero when `doctor` is missing a required tool
- non-zero on registry/signature verification failure

## Config schema (v1)

`wasm-app.toml` uses a frozen v1 schema. Unknown fields (top-level or inside
`[plugins.<name>]`) are rejected with an error naming the key and its
line/column. All supported fields:

| Section | Field | Type | Notes |
|---|---|---|---|
| `[app]` | `name`, `version`, `identifier` | string | app metadata |
| `[host]` | `addr` | socket addr | host bind address |
| `[desktop]` | `path`, `ui`, `output` | string | desktop/ui paths |
| `[signing]` | `key`, `public_key`, `generate`, `require` | string/bool | signing config |
| `[test]` | `ui` | string[] | UI test command |
| `[[smoke.calls]]` | `plugin`, `function`, `args`, `expect_contains`, `expect_equals`, `min`, `max` | mixed | smoke calls |
| `[[smoke.http]]` | `path`, `method`, `body`, `expect_status`, `expect_contains` | mixed | smoke HTTP |
| `[[smoke.services]]` | `plugin` | string | smoke services |
| `[plugins.<name>]` | `path`, `language`, `type`, `version`, `fs`, `kv`, `http_outbound`, `network_outbound`, `network_inbound`, `persistent`, `runtime`, `ui_entry`, `ui_build`, `build`, `compose_with` | mixed | plugin config |
| `[dev]` | `watch`, `hot_reload`, `ui_addr` | bool/string | dev mode |

### Validation rules (RF-01)

- `[host].addr` and `[dev].ui_addr` must parse as `SocketAddr`; errors name the
  field and value (e.g. `invalid address for host.addr "127.0.0.1:70000"`).
- Port `0` is allowed for `run`/`dev` (ephemeral) but is rejected by
  `bundle` via `validate_for_bundle`.
- Path fields (`[desktop].path/ui/output`, `[signing].key/public_key`,
  `[plugins.<name>].path`, and each entry of `[plugins.<name>].fs`) must be
  non-empty strings. Relative paths resolve against the config file directory;
  absolute paths are used unchanged (`resolve_path`).
- `[app].name`, `[app].identifier`, and `[app].version` (when present) must be
  non-empty (whitespace-only counts as empty).


## Build id (RF-07)

`owasidea build` computes a deterministic `build_id` and records it in every manifest (`metadata.build_id`), `owasidea.runtime.json`, and exposes it via `OWASIDEA_BUILD_ID`. Override with `OWASIDEA_BUILD_ID`; for reproducible builds set `SOURCE_DATE_EPOCH`. Format: `<git-short-sha>-<epoch>`.

Skew detection (RF-08): the UI compares its runtime `build_id` (`window.__OWASIDEA__.build_id`) with the host `/overview` `build_id`; when both are present and differ it renders a "UI bundle anterior ao host" banner. `owasidea dev` prints a warning when the UI runtime bundle build id differs from (or is missing vs) the host manifest build id.

## Error codes

CLI errors may reference stable codes `OWASIDEA_E_*`. Use `owasidea explain <code>` (and `--json`) for details. Full catalog: `docs/ERRORS.md`.

### Permission diff

`owasidea build [--fail-on-widen]` compares effective permissions with `permissions.lock.json`.
