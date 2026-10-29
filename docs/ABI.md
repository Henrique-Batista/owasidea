# Host ABI versioning (CT-08)

Generated manifests carry three version fields alongside the existing WASI `abi`:

- `schema_version`: manifest schema version (current: `2`).
- `host_abi`: host ABI family accepted by the manifest (current: `"2"`).
- `min_host`: minimum host semver required by the plugin (e.g. `"0.1.0"`).

The host exposes constants `HOST_ABI` and `HOST_VERSION`. On load it:

- accepts only `schema_version: 2` and a matching `host_abi: "2"`;
- rejects ABI 1 and legacy manifests with `owasidea migrate` guidance;
- rejects `min_host` greater than the running host version.

Errors name the plugin, field, and both values. `owasidea validate` checks the
same fields. Regenerate manifests with `owasidea build`; do not edit by hand.

## Deprecation and compatibility

ABI 1 hosts accepted legacy manifests. ABI 2 rejects all ABI 1 manifests;
`owasidea validate` and host load report migration guidance. Run
`owasidea migrate` and rebuild:

```text
error: schema_version 1 is not supported; expected 2. Run `owasidea migrate`
and rebuild with Owasidea 1.2.
```

Policy (semver rules, breaking changes, deprecation window, warning vs error)
lives in [`ABI_POLICY.md`](ABI_POLICY.md). Upgrade steps from 1.1 are in
[`MIGRATION_1_1_TO_1_2.md`](MIGRATION_1_1_TO_1_2.md).

## Field meanings

| Field | Enforced | Meaning |
| --- | --- | --- |
| `schema_version` | yes | manifest JSON shape; only version 2 is accepted |
| `host_abi` | yes | host interface family; only ABI 2 is accepted |
| `min_host` | yes | oldest host semver; newer hosts are fine, older ones reject the plugin |
| `abi` | no | guest contract label (`wasi-0.2`, `wasi-0.3`, `webview-v1`) |
