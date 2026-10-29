# interop

Rust glue plugin calling a Go provider through generated WIT bindings.

- `glue-rust` is the app glue (`[app].ui_plugin`). It imports
  `interop:provider/catalog@0.1.0` and opens two KV namespaces:
  `app-state:read-write` and `catalog-cache:read`.
- `provider-go` exports `interop:provider/catalog@0.1.0`.
- The config declares a cycle (`provider-go` → `glue-rust` → `provider-go`) to
  prove build order does not depend on a topological sort. A runtime call that
  re-enters an active plugin fails with
  `OWASIDEA_E_DEPENDENCY_REENTRANT_CALL`.

```bash
owasidea build
owasidea smoke --no-build
```

The smoke checks prove the typed Rust→Go call, KV persistence, and that a
read-only namespace rejects writes instead of silently dropping them.
