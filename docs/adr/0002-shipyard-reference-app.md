# ADR 0002 — Shipyard reference application

## Context

The framework had small, focused plugin examples but no single application that
proved the whole product path: every language runtime, isolated UI frameworks,
capabilities, composition, service lifecycle, executable HTTP, and Tauri
packaging.

## Decision

`examples/shipyard` is the canonical end-to-end example. It is a release cockpit
with:

- six language runtimes: Rust, Go, TypeScript, C#, C, C++;
- a composed Go→Rust pipeline (`compose_with`);
- host KV, host log, and a filesystem preopen;
- a persistent service (`watchtower-rust`);
- an executable `wasi:http` plugin (`webhook-go`);
- isolated Svelte and React UI bundles;
- a Tauri 2 `.deb` with generated manifests, component files, and a signing
  public key as resources.

## Consequences

- `owasidea build`, `dev`, `validate`, and `bundle` resolve paths relative to the
  config file so the example can be driven from the repository root.
- `bundle` reads `[desktop] path/ui` and supports an app-local Tauri shell.
- The host reads mutable state from `OWASIDEA_DATA_DIR`, which the desktop shell
  points at the platform app-data directory.
- The example build scripts sign manifests with a local demo key. The private key
  is gitignored; production must use a protected CI key. The Tauri resource map is
  explicit so source trees and `node_modules` never enter the installer.
- UI plugins remain presentational and sandboxed; live host data stays in the
  main dashboard.

## Alternatives considered

- Reusing the existing root demo plugins: rejected — the example must be a fresh,
  self-contained project and exercise new code.
- Bundling the whole `plugins/` tree as Tauri resources: rejected — it would ship
  source, caches, and `node_modules`.
- Adding a JavaScript runtime for UI plugins: rejected — `js-wasm` is webview-only
  and UI plugins stay static, hashed bundles.
