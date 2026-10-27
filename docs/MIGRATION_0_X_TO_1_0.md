# Migrating an Owasidea app from 0.x to 1.0

Status: applies to apps created with any 0.x release of `owasidea` and to
plugins built against a 0.x host.

1.0 is a **compatible** release: a 0.x app keeps running. This guide is the
checklist to remove deprecations, so the app is ready before host ABI 2 removes
legacy manifests. Policy behind every rule: [`ABI_POLICY.md`](ABI_POLICY.md).

Estimated work: 10–30 minutes for a typical app (config review, one
`owasidea build`, one `validate`). No plugin source changes are required unless
you hit one of the "what breaks" items in §8.

---

## 0. Quick path

```bash
# 1. Update the toolchain and CLI
cargo install --path crates/cli        # or use the release binary for your platform

# 2. Check the config still parses under the frozen v1 schema
owasidea --config wasm-app.toml doctor

# 3. Regenerate manifests (adds schema_version/host_abi/min_host)
owasidea --config wasm-app.toml build

# 4. Confirm ABI, checksums and capabilities
owasidea --config wasm-app.toml validate

# 5. Run the app
owasidea --config wasm-app.toml run
```

If step 4 prints `Validation OK.` with no `warning:` lines, the migration is
done. Warnings are advisory and exit `0`; see §5.

## 1. Config (`wasm-app.toml`)

The filename and v1 schema are unchanged. What changed is **strictness**: 1.0
rejects unknown keys instead of ignoring them.

```toml
[app]
name = "my-app"
version = "1.0.0"          # drives Tauri version + latest.json
identifier = "dev.example.myapp"

[host]
addr = "127.0.0.1:8787"    # single source of truth for the host origin

[plugins.notes]
path = "plugins/notes"
language = "rust"
type = "library"
```

Checks:

- Remove keys you added "just in case". Unknown top-level or
  `[plugins.<name>]` keys now fail with the key name and line/column
  (`OWASIDEA_E_CONFIG_INVALID`).
- Ports/origins live in `[host].addr` and `[dev].ui_addr` only. Do not duplicate
  them in UI or desktop configs; `owasidea config render` derives
  `runtime.json`, `playwright.json`, and `tauri.generated.conf.json`.
- If you use profiles, overlays are `wasm-app.<profile>.toml` and artifacts go
  to `dist/<profile>/` (`bundle --profile`, `latest --profile`).
- New optional sections are safe to add: `[updater]`, `[test]`, `[smoke]`,
  `[dev]`. Full field table: [`CLI.md`](CLI.md#config-schema-v1).
- Rust `wit_source` is new and optional. `auto` (default) keeps using an
  existing `wit/`; `attributes` generates `.owasidea/wit` from
  `#[owasidea::world]`/`#[owasidea::export]`; `wit` forces a hand-written
  contract. Existing plugins need no change. Do not keep both a `wit/` file and
  attribute markers: 1.0 fails the build instead of picking one silently.

## 2. Manifest

Manifests are generated, never hand-edited. A 0.x manifest without the CT-08
fields still loads, but regenerate it:

```bash
owasidea --config wasm-app.toml build
owasidea --config wasm-app.toml inspect notes --json
```

1.0 manifests carry:

| Field | 1.0 value | Why |
| --- | --- | --- |
| `schema_version` | `1` | manifest shape |
| `host_abi` | `"1"` | host interface family |
| `min_host` | host semver that built it | refuses to run on an older host |
| `abi` | `wasi-0.2`, `wasi-0.3`, or `webview-v1` | guest contract label (informational) |

Rules:

- Do not patch `schema_version`, `host_abi`, or `min_host` by hand. `build`
  derives them; `validate`/`verify` compare them with the running host.
- If a manifest has `schema_version: 1` but no `host_abi`/`min_host`, it is
  invalid by design — regenerate instead of adding the fields manually.
- `metadata.sha256` must match the component bytes. Editing a `.wasm` without
  rebuilding breaks `validate` and `verify`.

## 3. Bridge (UI plugins)

0.x UI plugins were fully isolated. 1.0 adds an **opt-in** allowlist; isolation
is still the default.

```toml
[plugins.board-svelte]
path = "plugins/board-svelte"
language = "svelte"
type = "ui"
runtime = "js-wasm"
ui_entry = "index.html"
ui_build = ["npm", "run", "build"]
bridge = ["notes.list"]          # <plugin>.<export>, validated at build time
```

- Every entry must be `plugin.export` and the export must exist in the headless
  plugin's manifest, otherwise `build` fails.
- `build` writes the sorted list to `metadata.ui.bridge` and copies
  `owasidea-bridge.js` next to the assets.
- Frontend: `const bridge = await mod.connect()` then
  `bridge.call("notes", "list", [])`. Calls outside the allowlist return
  `NOT_ALLOWED`; plugins without `bridge` behave exactly as in 0.x.
- Protocol and CSP details: [`UI_PLUGINS.md`](UI_PLUGINS.md#bridge-sdk-ct-07).

## 4. Updater

0.x had no updater integration. 1.0 is opt-in; apps without `[updater]` are
unaffected and `owasidea latest` fails with "updater is not configured".

```toml
[updater]
channel = "stable"                                    # stable | beta
endpoint = "https://releases.example.com/{{channel}}/latest.json"
pubkey = "keys/owasidea-updater.pub"
notes_file = "docs/RELEASE_NOTES.md"                  # optional
```

- The updater key (minisign, Tauri) is **not** the plugin signing key
  (`[signing]`, Ed25519). Do not reuse them.
- `bundle` signs updater artifacts and writes `latest.json`;
  `owasidea latest --check` validates an existing one.
- Channels select the release line; profiles select app identity/output dir.
  They are independent knobs. See [`UPDATER.md`](UPDATER.md).

## 5. ABI

What you need to know as an app author:

- Host ABI family is `"1"`. A 0.x plugin whose manifest declares `host_abi`
  other than `"1"` is rejected at load; the error names the plugin, the field,
  and both values. The matching CLI taxonomy entry is
  `OWASIDEA_E_ABI_INCOMPATIBLE` (`owasidea explain OWASIDEA_E_ABI_INCOMPATIBLE`).
- `min_host` greater than the running host is rejected. Keep the CLI/host and
  the manifests from the same build; a stale `dist/` copied from a newer build
  will refuse to load on an older host.
- Legacy manifests (no `schema_version`) are still accepted under host ABI 1
  **and are deprecated**. `validate` prints (one line per warning, wrapped here
  for readability):

  ```text
  warning: notes: ABI_LEGACY_MANIFEST: deprecated legacy manifest (no integer
  schema_version field); host ABI 1 still accepts it with defaults, but a future
  host ABI will reject it. Regenerate with `owasidea build`
  (see docs/ABI_POLICY.md).
  ```

  Regenerating removes the warning. Removal follows the two-minor/six-month
  window in [`ABI_POLICY.md`](ABI_POLICY.md#4-deprecation-window).
- `owasidea verify dist/ --public-key <release.pub>` applies the same ABI checks
  to a release artifact and reports stable codes (`HOST_ABI_MISMATCH`,
  `HOST_ABI_MISSING`, `MIN_HOST`, `MIN_HOST_MISSING`, `SCHEMA_VERSION`). The
  trust root must be pinned (`--public-key`/`OWASIDEA_PUBLIC_KEY`);
  `TRUST_ROOT_UNPINNED` rejects a key staged inside `dist/`.

## 6. CLI flags and commands

Binary name is `owasidea`. Global flags resolve paths from the config file
directory, so every command can run from any working directory:

```bash
owasidea --config examples/shipyard/wasm-app.toml build
owasidea --config examples/shipyard/wasm-app.toml validate
owasidea --config examples/shipyard/wasm-app.toml call core-rust.ping
```

| Global flag | Default | Notes |
| --- | --- | --- |
| `--config <path>` | `wasm-app.toml` | defines the app root |
| `--manifest-dir <path>` | `manifests` (`OWASIDEA_MANIFESTS`) | generated manifests |
| `--http-addr <addr>` | `[host].addr`, else `127.0.0.1:0` for CLI, `:8787` for `call` | host override |
| `--json` | off | stable structured output where supported |

Commands added during 0.x that replace ad-hoc scripts:

| Need | Command |
| --- | --- |
| build + API smoke + UI test | `test`, `smoke` |
| call an export from a terminal | `call <plugin>.<function>` |
| follow host logs | `logs` |
| validate manifests before release | `validate`, `verify <dist>` |
| updater manifest | `latest`, `latest --check` |
| signing readiness | `signing plan --platform windows\|macos\|linux` |
| typed frontend client | `client --lang ts -o src/client.ts` |
| derived config | `config render --check` |
| plugin/permission diffs | `build --fail-on-widen`, `permissions.lock.json` |
| error code lookup | `explain <OWASIDEA_E_*>` |

There is no `wasm-app` binary in 1.0; if a script still calls it, rename it to
`owasidea`. Full reference: [`CLI.md`](CLI.md).

## 7. Environment variables

0.x variables keep working. The ones worth knowing:

| Variable | Purpose |
| --- | --- |
| `OWASIDEA_DATA_DIR` | host KV/state root (use a temp dir in tests) |
| `OWASIDEA_MANIFESTS` | manifest dir (same as `--manifest-dir`) |
| `HOST_ADDR` | host bind override |
| `OWASIDEA_BIN` | CLI path handed to UI tests |
| `OWASIDEA_BUILD_ID` | build identity override (reproducible builds) |
| `OWASIDEA_PUBLIC_KEY` / `OWASIDEA_REQUIRE_SIGNATURES` | manifest signature policy |
| `OWASIDEA_ALLOW_NON_LOOPBACK` | allow a non-loopback host bind (refused by default) |
| `OWASIDEA_UPDATE_CHANNEL` | `stable` / `beta` for updater artifacts |
| `OWASIDEA_DEV_*` | dev-mode watcher/host knobs (internal) |

`OWASIDEA_WASMTIME_CACHE` points the Wasmtime compilation cache at a writable
directory (default: `$OWASIDEA_DATA_DIR/wasmtime-cache`, else a temp dir).
A read-only cache directory only slows cold starts; the host logs a warning and
continues.

## 8. What breaks, and how to fix it

| Symptom | Cause | Fix |
| --- | --- | --- |
| `OWASIDEA_E_CONFIG_INVALID` naming a key | unknown field in `wasm-app.toml` | delete/rename the key; see the schema table |
| `host_abi 0 != 1` at load | plugin built against an older ABI family | rebuild the plugin and manifests with the 1.0 toolchain |
| `min_host 1.0.0 > host 0.9.0` | manifests copied from a newer build | rebuild with the host you ship, or upgrade the host |
| `host_abi required for schema_version 1` | hand-edited manifest | regenerate with `owasidea build` |
| `CHECKSUM MISMATCH` from `validate` | component edited or stale | `owasidea build` |
| `fs/kv/http permission mismatch` | manifest not regenerated after config edit | `owasidea build`; review the permission diff |
| `UI manifest runtime/type mismatch` | UI plugin missing `runtime = "js-wasm"` + `type = "ui"` | add both to `[plugins.<name>]` |
| `bridge entry ... export not found` | allowlist names a non-existent export | fix the entry or the plugin export; entries are `plugin.export` |
| Instantiation fails on an undeclared host import | capability not declared in the manifest | declare it in `wasm-app.toml`, rebuild, review the diff |
| `updater is not configured` from `latest` | no `[updater]` section | add `[updater]` or stop calling `latest` |
| Reload returns HTTP `409` | stateful service is running | stop the service, then reload/restart (unchanged from 0.x) |
| `cargo tauri` not found with `dev --desktop` | Tauri CLI missing | `cargo install tauri-cli` |

Deliberate non-changes (no migration needed): plugin WIT contracts, the HTTP
`/call` and `/service/*` surfaces, KV/filesystem permissions, `js-wasm` being
webview-only, and the `examples/shipyard` workflow.

## 9. Verify the migration

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

cd examples/shipyard
owasidea build
owasidea validate      # no "warning:" lines
owasidea smoke
owasidea test
```

Release-time checks: `owasidea verify dist/<profile> --public-key <release.pub>`
and `owasidea config render --check`. Rollback is a `git checkout` of
`wasm-app.toml` plus `owasidea build`; generated manifests are not tracked, so
there is nothing else to revert.
