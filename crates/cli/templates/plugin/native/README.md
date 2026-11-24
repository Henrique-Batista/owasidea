# {{name}} (native UI)

This is a native UI plugin: a local executable that renders its own window.
It is not a WASI component and is not sandboxed. It runs with your user
privileges. See `docs/UI_PLUGINS.md`.

The CLI injects:

- `OWASIDEA_HOST` — base URL of the running host;
- `OWASIDEA_PLUGIN` — this plugin's name;
- `OWASIDEA_MANIFESTS` — manifest directory in use.

Build with:

```sh
owasidea build
owasidea run
```
