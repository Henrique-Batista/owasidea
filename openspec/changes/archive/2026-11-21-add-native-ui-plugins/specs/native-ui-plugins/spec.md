# Spec Delta

## Purpose

Lets an Owasidea app declare, build, and discover UI plugins that run as native desktop processes instead of sandboxed web assets, while keeping the plugin contract and manifests consistent with the rest of the framework.

## ADDED Requirements

### Requirement: Native UI plugin declaration

An app configuration SHALL accept a plugin with `type = "native-ui"` and `runtime = "native"`. The plugin entry MUST declare `native_build` (an argv array executed in the plugin directory) or an already-built `native_entry`, and MUST declare `native_entry` as a path inside the plugin directory.

The system MUST reject, with a non-zero exit and a message naming the plugin:

- `type = "native-ui"` without `runtime = "native"`, or `runtime = "native"` with any other type;
- webview-only fields (`ui_entry`, `ui_build`, `bridge`) on a native UI plugin;
- `native_entry` that is absolute, escapes the plugin directory, or does not exist after `native_build` runs;
- `native_build` containing an empty argv array or an empty program name.

#### Scenario: Valid declaration

- **WHEN** a plugin declares `type = "native-ui"`, `runtime = "native"`, `native_build = ["cargo", "build", "--release"]`, and `native_entry = "target/release/board"`
- **THEN** `owasidea build` builds the plugin and produces its manifest

#### Scenario: Webview field on a native UI

- **WHEN** a native UI plugin declares `ui_entry = "index.html"`
- **THEN** `owasidea build` exits non-zero and reports that `ui_entry` is webview-only

#### Scenario: Entry escapes the plugin directory

- **WHEN** `native_entry = "../../bin/evil"` is declared
- **THEN** validation fails before executing any build command

#### Scenario: Missing runtime marker

- **WHEN** a plugin declares `type = "native-ui"` without `runtime`
- **THEN** validation fails and reports the expected `runtime = "native"` value

### Requirement: Native UI manifest

`owasidea build` SHALL generate a manifest for every native UI plugin. The manifest MUST contain the plugin `name`, `type = "native-ui"`, `runtime = "native"`, the `binary` path relative to the app root, the binary's SHA-256 digest, and a `trust` field with the value `unrestricted`.

Native UI manifests MUST NOT claim WIT exports, WIT imports, filesystem, key/value, network, or capability entries. If a native UI plugin declares any of those fields, the build MUST fail.

#### Scenario: Manifest contents

- **WHEN** a native UI plugin builds successfully
- **THEN** its manifest contains `type = "native-ui"`, `runtime = "native"`, `binary`, `sha256`, and `trust = "unrestricted"`, and no `exports` array

#### Scenario: Capability attempt

- **WHEN** a native UI plugin declares `fs = ["reports"]`
- **THEN** the build fails and explains that native UIs cannot be capability-restricted

#### Scenario: Binary changed after manifest generation

- **WHEN** the binary digest differs from the digest recorded in the manifest
- **THEN** `owasidea verify` fails and names the plugin

### Requirement: Native UI discovery

The build SHALL write a `native-ui.json` index next to the generated manifests. The index MUST list each native UI plugin with its name, binary path, and digest.

`GET /plugins` SHALL list native UI plugins with `type = "native-ui"` and `runtime = "native"`, and MUST NOT route HTTP requests to them. Native UIs MUST NOT be served under `/app/{name}` or `/plugin/{name}/call`.

#### Scenario: Index generated

- **WHEN** an app with one native UI plugin builds
- **THEN** `native-ui.json` contains exactly that plugin's name, binary, and digest

#### Scenario: Discovery endpoint

- **WHEN** a client calls `GET /plugins`
- **THEN** the response includes the native UI plugin with type `native-ui` and runtime `native`

#### Scenario: No HTTP dispatch

- **WHEN** a client posts to `/plugin/{native-ui-name}/call`
- **THEN** the host responds as if no such library plugin exists

### Requirement: Scaffolding

`owasidea plugin new` SHALL accept `--type native-ui` and SHALL reject `--runtime js-wasm` for that type. Scaffolding MUST register the plugin in the app configuration with `native_build` and `native_entry` derived from the selected language/template, and MUST NOT add webview fields.

#### Scenario: Scaffold a native UI

- **WHEN** the user runs `owasidea plugin new board --lang gpui --type native-ui`
- **THEN** the command creates the plugin directory, registers `type = "native-ui"` and `runtime = "native"`, and prints the next build command

#### Scenario: Invalid runtime

- **WHEN** the user runs `owasidea plugin new board --type native-ui --runtime js-wasm`
- **THEN** the command exits non-zero with a message describing the native runtime requirement

### Requirement: Trust disclosure

The CLI SHALL state that native UI plugins run as trusted local code with the user's privileges, both when a native UI is built and in the generated `native-ui.json` index. Documentation for the plugin type SHALL state that native UIs are outside the WASI capability broker and the iframe sandbox.

#### Scenario: Build warning

- **WHEN** `owasidea build` builds one or more native UI plugins
- **THEN** the output includes a trust warning that names each plugin and does not imply sandboxing

#### Scenario: Index disclosure

- **WHEN** the generated `native-ui.json` is inspected
- **THEN** it contains a `trust` field describing native UIs as unrestricted local processes
