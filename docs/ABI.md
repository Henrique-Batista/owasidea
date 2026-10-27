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
