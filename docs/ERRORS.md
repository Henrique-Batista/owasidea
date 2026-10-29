# Owasidea Error Codes

Stable codes in the form `OWASIDEA_E_*`. Use `owasidea explain <code>` (and `--json`).

## OWASIDEA_E_CONFIG_INVALID

Configuration (`wasm-app.toml`) is invalid or missing.

- hint: Check the config path, TOML syntax and required sections (`[app]`, `[host]`, `[plugins.*]`).
- docs: `docs/ERRORS.md#owasidea_e_config_invalid`

## OWASIDEA_E_TOOLCHAIN_MISSING

A required toolchain binary is missing or too old.

- hint: Run `owasidea doctor` to see which tool is missing and how to install it.

## OWASIDEA_E_MANIFEST_INVALID

A generated or loaded plugin manifest is invalid.

- hint: Re-run `owasidea build` to regenerate manifests from WIT and inspect the reported field.

## OWASIDEA_E_HTTP_CALL

HTTP/network call to the host or a plugin failed.

- hint: Confirm the host is running, the address/port, and the request body/args; retry.

## OWASIDEA_E_PLUGIN_NOT_FOUND

The requested plugin does not exist in the manifest directory.

- hint: Check the plugin name and run `owasidea build`; use `owasidea inspect <plugin>`.

## OWASIDEA_E_ABI_INCOMPATIBLE

Plugin ABI/runtime is incompatible with this host.

- hint: Rebuild the plugin with the supported WASI component target and matching WIT.

## OWASIDEA_E_BUILD_FAILED

Plugin build failed.

- hint: Read the compiler/build tool output above; fix the source and retry `owasidea build`.

## OWASIDEA_E_DEPENDENCY_REENTRANT_CALL

A nested dependency call re-entered a plugin already active on the request's
call stack.

- hint: Declared dependency cycles are allowed, but runtime A→B→A is not. Break the recursive path.

## OWASIDEA_E_DEPENDENCY_DEPTH_EXCEEDED

A nested dependency call exceeded the 1.2 limit of 16 active plugins.

- hint: Flatten the dependency chain or move shared logic into a library helper.

## OWASIDEA_E_VALIDATE_FAILED

Manifest validation failed (checksum, capability or config mismatch).

- hint: Run `owasidea validate` and align manifests with `wasm-app.toml` and component WIT.
