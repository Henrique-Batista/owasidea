# Desktop (Tauri 2)

The desktop shell embeds `owasidea-host` in-process and loads `ui/dist`. The
frontend communicates through the host HTTP API; the Tauri webview never links
Wasmtime directly. Desktop release builds include the configured CSP and
sandboxed webview UI plugin loader.

At startup the shell resolves manifests and the trust key from the Tauri
resource directory, creates and sets `OWASIDEA_DATA_DIR` to the platform
app-data directory (so KV databases and relative preopens never land in the
working directory), and starts the host. `owasidea bundle` generates
`bundle.resources` (manifests, components, `trust/owasidea.pub`,
`owasidea.runtime.json`) from `wasm-app.toml`; the base `tauri.conf.json` only
owns targets and icons.

Build from the repository root with `owasidea bundle` after `owasidea build`.


## Windows

`tauri.windows.conf.json` selects NSIS and `icons/icon.ico`. Build natively on
Windows:

```powershell
owasidea bundle
```

Cross-compiling the Windows installer from Linux is not wired; use a Windows
runner. See `docs/WINDOWS.md`.
