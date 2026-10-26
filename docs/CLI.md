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
   key. `generate = true` creates a demo key when missing.

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

### `test [--no-build] [--no-smoke] [--no-ui]`

Run the full framework test flow:

1. `build` unless `--no-build`.
2. Start a temporary host on an ephemeral port with a temporary
   `OWASIDEA_DATA_DIR` and signing environment.
3. Run generic checks (health, all headless plugins loaded, UI plugin count) and
   the declarative checks under `[smoke]` in `wasm-app.toml`.
4. Stop the host.
5. Run `[test].ui` as an argv array in the app root, with `OWASIDEA_BIN` pointing
   at the current CLI. The example uses Playwright.

### `smoke [--no-build]`

Build and run only the API smoke checks (steps 1–4 above). Useful in CI.

### `validate`

Check generated manifests against `wasm-app.toml`:

- manifest exists
- component checksum matches
- filesystem/KV/HTTP permissions match the config
- UI manifests declare `runtime = "js-wasm"` and `type = "ui"`

Exits non-zero on drift. Run after `build`, especially in CI.

### `config render [--target runtime|tauri|playwright|all] [--check]`

Render derived configuration artifacts from `wasm-app.toml` (single source of
truth for host/port/CSP/dev URL):

- `runtime` -> `<ui>/.owasidea/runtime.json`
- `playwright` -> `<ui>/.owasidea/playwright.json`
- `tauri` -> `<desktop>/tauri.generated.conf.json` (copies base `tauri.conf.json`,
  overriding `build.devUrl` and ensuring the host origin is in `connect-src`)

`--check` does not write; exits non-zero if files are missing or stale.

### `dev [--no-build]`

Build first, then start the host, the Vite dev server (if
`<app-root>/ui/package.json` exists), and a recursive watcher. The UI port comes
from `[dev].ui_addr` when set; otherwise the first free port in 5173..=5273.
A busy configured port falls back to the next free port. `dev` also writes
`<desktop>/tauri.dev.conf.json` (merged from the base `tauri.conf.json`, with
`build.devUrl` set to the reserved UI URL) for `dev --desktop` (DL-01). Prints:

- UI URL
- host URL
- loaded plugin names
- watch roots
- Tauri dev config path and devUrl

The watcher hot-reloads stateless components by calling
`POST /plugin/{name}/reload`. Stateful services require an explicit stop/reload;
reload while running returns HTTP `409`. Source changes that require compilation
print a message and require `owasidea build`.

### `run`

Run the current manifests without building or watching. The host address comes
from `--http-addr`, then `[host].addr`, then `127.0.0.1:0`. Signing environment
from `[signing]` is applied automatically.

### `inspect <plugin> [--json]`

Show the generated manifest, and for headless plugins the formatted WIT.
`--json` prints the raw manifest.

### `bundle [--no-build] [--bundles <targets>] [--isolate-host-libs]`

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
shell must include the public key as a Tauri resource and set
`OWASIDEA_REQUIRE_SIGNATURES=true` and `OWASIDEA_PUBLIC_KEY`.
`bundle.resources` in `tauri.conf.json` must include the generated manifests and
component `.wasm` files; see `examples/shipyard/desktop`.

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

### `wit generate [path]`

Regenerate WIT from source annotations (`owasidea::*` in Rust, `// owasidea:`
in Go/C/C++, JSDoc in TypeScript, attributes in C#). `build` calls this per
plugin. See `docs/SOURCE_GENERATORS.md`.

### `[signing]` configuration

```toml
[signing]
key = "keys/owasidea.key"
public_key = "keys/owasidea.pub"
generate = true
require = true
```

`build` and `bundle` sign every manifest. `run`, `dev` and `test` apply
`OWASIDEA_PUBLIC_KEY` and `OWASIDEA_REQUIRE_SIGNATURES` before starting the host.
Keep private keys out of the repository in production.

### `keygen --out <dir>` / `sign <plugin> --key <key>`

Generate an Ed25519 keypair and sign a release manifest plus component bytes.
Private keys are written mode `0600` on Unix. See `docs/REGISTRY.md` and
`docs/SECURITY.md`.

### `registry search <query> --url <index.json> [--json]`

Search a static HTTPS registry index. HTTP is allowed only for localhost tests.

### `registry install <name> --version <version> --url <index.json> --public-key <key> [--force]`

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
