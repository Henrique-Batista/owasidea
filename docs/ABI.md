# Host ABI versioning (CT-08)

Generated manifests carry three version fields alongside the existing WASI `abi`:

- `schema_version`: manifest schema version (current: `1`).
- `host_abi`: host ABI family accepted by the manifest (current: `"1"`).
- `min_host`: minimum host semver required by the plugin (e.g. `"0.1.0"`).

The host exposes constants `HOST_ABI` and `HOST_VERSION`. On load it:

- accepts old manifests without these fields (defaults applied);
- rejects unknown `schema_version`;
- rejects incompatible `host_abi`;
- rejects `min_host` greater than the running host version.

Errors name the plugin, field, and both values. `owasidea validate` checks the
same fields. Regenerate manifests with `owasidea build`; do not edit by hand.

## Deprecation and compatibility

Legacy manifests (no `schema_version`, i.e. implicit `0`) still load under host
ABI 1, but they are **deprecated**. `owasidea validate` prints one line per
warning (wrapped here for readability) and still exits `0`:

```text
warning: notes: ABI_LEGACY_MANIFEST: deprecated legacy manifest (no integer
schema_version field); host ABI 1 still accepts it with defaults, but a future
host ABI will reject it. Regenerate with `owasidea build`
(see docs/ABI_POLICY.md).
warning: notes: ABI_MISSING_HOST_FIELDS: manifest has no host_abi or min_host;
the host applies defaults. Regenerate with `owasidea build`
(see docs/ABI_POLICY.md).
```

Policy (semver rules, breaking changes, deprecation window, warning vs error)
lives in [`ABI_POLICY.md`](ABI_POLICY.md). Upgrade steps for a 0.x app are in
[`MIGRATION_0_X_TO_1_0.md`](MIGRATION_0_X_TO_1_0.md).

## Field meanings

| Field | Enforced | Meaning |
| --- | --- | --- |
| `schema_version` | yes | manifest JSON shape; unknown newer versions are rejected |
| `host_abi` | yes | host interface family; mismatch is rejected |
| `min_host` | yes | oldest host semver; newer hosts are fine, older ones reject the plugin |
| `abi` | no | guest contract label (`wasi-0.2`, `wasi-0.3`, `webview-v1`) |
