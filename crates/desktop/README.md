# Desktop (Tauri 2)

The desktop shell embeds `owasidea-host` in-process and loads `ui/dist`. The
frontend communicates through the host HTTP API; the Tauri webview never links
Wasmtime directly. Desktop release builds include the configured CSP and
sandboxed webview UI plugin loader.

Build from the repository root with `owasidea bundle` after `owasidea build`.


## Windows

`tauri.windows.conf.json` selects NSIS and `icons/icon.ico`. Build natively on
Windows:

```powershell
owasidea bundle
```

Cross-compiling the Windows installer from Linux is not wired; use a Windows
runner. See `docs/WINDOWS.md`.
