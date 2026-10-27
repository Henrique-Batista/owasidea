# Shipyard desktop shell

Tauri 2 shell for the Shipyard example. It embeds `owasidea-host`, points the
webview at `../ui/dist`, and packages generated manifests, the WASI component
files, the signing public key and the runtime descriptor under `$RESOURCE/`.

## Build

From `examples/shipyard`:

```bash
owasidea keygen --out keys   # once; the example does not auto-generate keys
owasidea bundle
```

`bundle` builds and signs first, runs the UI build, then Tauri, and copies the
artifacts to `dist/` because `[desktop].output = "dist"`. The base config uses
`"targets": "all"`; use `owasidea bundle --bundles deb` to generate one target.
`bundle.resources` is generated into `tauri.generated.conf.json` from
`wasm-app.toml`, so the base config only owns targets and icons.

The artifact is written under `$CARGO_TARGET_DIR/release/bundle/deb/` when
`CARGO_TARGET_DIR` is set, otherwise `desktop/target/release/bundle/deb/`.

On Windows, Tauri merges `tauri.windows.conf.json` and builds an NSIS installer
(`.exe`) using `icons/icon.ico`. The same `[desktop].output` copy applies.

## Runtime resolution

```text
$RESOURCE/manifests/<name>.manifest.json
$RESOURCE/plugins/<name>/<name>.component.wasm
```

`main.rs` resolves manifests in this order: `OWASIDEA_MANIFESTS`, Tauri
`$RESOURCE/manifests`, executable-dir `manifests`/`resources/manifests`, then the
source tree via `CARGO_MANIFEST_DIR` (for `cargo run` and direct release
binaries). `owasidea bundle` also copies the runtime resources and trust key to
`target/release/resources/`, so running `target/release/shipyard-desktop`
directly works even when `CARGO_TARGET_DIR` is outside the repository.

It sets `OWASIDEA_DATA_DIR` to the platform app-data directory so KV databases
and the `reports/` preopen are writable in an installed app.

## Adding a plugin

Add the `[plugins.<name>]` entry in `wasm-app.toml`; `bundle` regenerates
`bundle.resources` with that component. Do not bundle the whole `plugins/` tree:
it contains source, `node_modules`, and build caches.
