# Spec Delta

## Purpose

Ensures native UI binaries travel with the application in both desktop and headless distributions, are discoverable at runtime, and can be verified against the digests recorded at build time.

## ADDED Requirements

### Requirement: CLI bundle contents

`owasidea bundle --target cli` SHALL copy every native UI binary into the bundle under `native/<plugin-name>/` and SHALL keep the binary executable on Unix.

The bundled `wasm-app.toml` MUST resolve each `native_entry` to the copied location, and the generated launcher MUST honour native UI startup unless the operator passes `--no-native-ui`. The bundle report MUST include the number of native UI binaries staged.

#### Scenario: Bundle a native UI app

- **WHEN** `owasidea bundle --target cli` runs for an app with a native UI plugin
- **THEN** the output directory contains `native/<name>/<binary>`, the rewritten config, and an executable launcher

#### Scenario: Run the CLI bundle

- **WHEN** the operator runs the generated launcher on a machine with a display
- **THEN** the host starts and the native UI process starts with `OWASIDEA_HOST` pointing at the bundle host

#### Scenario: Binary not executable on Unix

- **WHEN** the staged native UI binary lacks the executable bit
- **THEN** bundling restores it or fails with a message naming the file

### Requirement: Desktop bundle contents

`owasidea bundle` for the desktop target SHALL package native UI binaries as Tauri sidecars named with the host target triple, and SHALL generate the Tauri configuration entries needed to install and launch them.

If the requested bundle target differs from the host platform, the command MUST fail and MUST state that native UI binaries are not cross-compiled.

#### Scenario: Desktop package with a native UI

- **WHEN** a desktop bundle is built on the host platform for an app with a native UI plugin
- **THEN** the produced package contains the sidecar binary named with the host target triple and a Tauri configuration that lists it

#### Scenario: Cross-target request

- **WHEN** a desktop bundle is requested for a target other than the host platform while native UI plugins are declared
- **THEN** the command exits non-zero and explains the cross-compilation limitation

### Requirement: Bundle integrity verification

`owasidea verify` SHALL accept a bundle directory, verify each native UI binary against the digest recorded in the bundle's `native-ui.json`, and fail when a binary is missing, was modified, or is not listed.

The verification report MUST include native UI entries alongside manifests, components, and UI assets.

#### Scenario: Intact bundle

- **WHEN** `owasidea verify dist/` runs against an unmodified bundle
- **THEN** verification succeeds and reports the number of native UI binaries checked

#### Scenario: Tampered binary

- **WHEN** a bundled native UI binary is replaced after bundling
- **THEN** `owasidea verify dist/` exits non-zero and names the plugin and expected digest

#### Scenario: Missing binary

- **WHEN** a native UI binary is deleted from the bundle
- **THEN** verification fails and reports the missing path
