# Changelog

All notable changes to Owasidea are recorded here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions use
[semver](https://semver.org/). ABI rules live in
[`docs/ABI_POLICY.md`](docs/ABI_POLICY.md); upgrade steps live in
[`docs/MIGRATION_0_X_TO_1_0.md`](docs/MIGRATION_0_X_TO_1_0.md).

Release notes for the updater (`latest.json`) may reuse the matching section
below; see [`docs/UPDATER.md`](docs/UPDATER.md).

## [Unreleased]

## [1.4.0] - 2026-10-29

### Changed

- HTTP exposure is deny-by-default. `/plugin/{name}/...` and `/app/{name}`
  return `403` unless the plugin sets `expose = true`. The configured
  `ui_plugin` glue remains reachable without the flag. Plugin-to-plugin
  `dependencies` calls are unaffected.
- `GET /plugins` hides unexposed plugins in production.
- Generated manifests carry `http_exposed` and `min_host: 1.4.0`. Legacy
  manifests without the field keep the pre-1.4 exposed behavior; rebuild to
  apply default-deny.
- `owasidea dev`, `owasidea smoke`, `owasidea test` and
  `OWASIDEA_DEV_MODE=1` keep the full developer surface.

### Added

- `expose = true` per plugin in `wasm-app.toml`.
- `docs/MIGRATION_1_3_TO_1_4.md` with the opt-in checklist.

## [1.3.1] - 2026-10-29

### Added

- C# consumers get `Owasidea.GlobalUsings.g.cs` with `global using static` for
  imported provider interfaces, so `Greet("x")` works without the full
  namespace. Duplicate interface names use aliases.
- C# scaffolds write `Directory.Build.props` setting `OwasideaWitDir` to the
  resolved view and `ArtifactsPath` to `.owasidea/cache/dotnet/<plugin>`.
  Rider/Visual Studio and plain `dotnet build` now use the same paths as
  `owasidea build`.

### Fixed

- `dotnet build` outside the CLI resolves the dependency WIT view and writes
  generated bindings under the project-level artifacts path.

## [1.3.0] - 2026-10-29

### Added

- Project-level build store: `owasidea build` writes dependency packages,
  per-consumer WIT views, final components and toolchain caches under
  `.owasidea/`. See `docs/BUILD_LAYOUT.md`.
- Dependency WIT is resolved automatically from `.owasidea/deps/`; consumers no
  longer need a hand-copied `wit/deps/<provider>`.
- Rust builds use `.owasidea/cache/cargo/<plugin>` as `CARGO_TARGET_DIR` when
  the variable is not already set. C# builds use
  `dotnet --artifacts-path .owasidea/cache/dotnet/<plugin>`.
- C#, C/C++ and TypeScript templates read the resolved WIT view through
  `OwasideaWitDir`, `WIT_DIR`/`OUT` and `OWASIDEA_WIT_DIR`/`OWASIDEA_OUT`.
- Bundles stage rewritten manifests in `.owasidea/runtime-manifests/` so the
  packaged `manifests/` and `plugins/` layout stays self-contained.

### Changed

- Final components live in `.owasidea/build/<plugin>/<plugin>.component.wasm`.
  `plugins/<plugin>/` keeps sources, contracts and toolchain-owned bindings.
- `owasidea build` adopts a legacy plugin-local component into the store and
  removes the duplicate.
- Manifests keep `host_abi: "2"`; `min_host` follows the 1.3 host.

### Fixed

- Rust dependency mirroring preserves the `wit/deps` paths referenced by
  `package.metadata.component.target.dependencies`, merges only the interfaces
  the contract imports, and never replaces existing source packages.

## [1.2.0] - 2026-10-28

### Breaking

- **Host ABI 2 / manifest `schema_version: 2`.** ABI 1 manifests are rejected
  with migration guidance; existing 1.1 apps stay on the 1.1 host. See
  [`docs/MIGRATION_1_1_TO_1_2.md`](docs/MIGRATION_1_1_TO_1_2.md) and the
  approved exception in [`docs/ABI_POLICY.md`](docs/ABI_POLICY.md).
- `kv` is a list of namespace grants: `kv = ["state:read-write"]`.
- HTTP/socket allowlists are exact `host:port` / `IP:port` pairs. Plaintext
  HTTP is loopback-only.

### Added

- Host-mediated typed runtime dependencies between library plugins using
  generated WIT bindings; cycles are allowed and runtime re-entry fails with
  `OWASIDEA_E_DEPENDENCY_REENTRANT_CALL`.
- Typed `poc:host/service-control` for declared service dependencies:
  `start`, typed `status`, and `stop` by dropping the host instance.
- Multi-namespace KV resource handles with per-grant read/write enforcement and
  explicit guest errors.
- `[app].ui_plugin` glue-only production app surface; dev tools keep full access.
  Shipyard now routes workbench and service actions through `app-glue`.
- `owasidea migrate [--write]` for supported 1.1 config forms.
- Permission presets `network-outbound` and `app-data`, expanded into
  inspectable effective permissions.
- `examples/interop`: Rust glue → Go provider, two KV namespaces and a declared
  dependency cycle.

### Fixed

- HTTP outbound checks host, effective port and scheme on every request; exact
  allowlist hostnames are pinned at load with TLS SNI/certificate validation,
  and denied writes or unknown KV namespaces return guest errors instead of
  silent success.

## [1.1.0] - 2026-10-28

### Added

- `docs/README.md` is the documentation index (guides, reference, operations,
  plans, ADRs) with an explicit status table for 1.1 claims: bridge and typed
  client implemented, jobs experimental, WebView2 not validated/fail-closed,
  performance budgets measured on Linux with warm cache (G3).
- `docs/OPTIONAL_TOOLCHAINS.md` documents jco, componentize-dotnet/.NET 10,
  trunk, wasi-sdk, C/C++ and Tauri packaging: versions, per-OS install commands,
  `doctor --fix` / `tools.lock` semantics and limitations (FIX-06).
- `tools.lock` now lists install commands for `cargo-component`, `wasm-tools`,
  `go`, `node` and `npm`, so `owasidea doctor --fix` covers the required
  toolchain (FIX-06).
- `owasidea bundle --target cli` produces a headless directory with the host
  binary, manifests, components and `run.sh`/`run.bat`.
- TUI mouse support: clickable tabs, plugin list and footer buttons.
- `owasidea dev` captures the initial build output into the dashboard and keeps
  a persistent `BUILD FAILED` banner until a rebuild succeeds.

### Removed

- Remote plugin registry (`owasidea registry search|install|update|outdated`,
  `plugins.lock`, `docs/REGISTRY.md`, registry tests). Plugins are source
  repositories and build artifacts; there is no hosted index or marketplace.
- Dead `crates/macros` duplicate of `crates/owasidea`.
- Stray `main`/`directories` fields from the root `package.json`.
- Plugin manifest signing (`owasidea keygen/sign`, `[signing]`,
  `OWASIDEA_REQUIRE_SIGNATURES`, `OWASIDEA_PUBLIC_KEY`, trust-key staging).
  Manifests are unsigned and checked by SHA-256; release integrity comes from
  the build pipeline and OS code signing. `owasidea verify` no longer takes
  `--public-key`/`--allow-dist-trust`.
- Root demo app (`plugins/`, `manifests/`, `data/`, `wasm-app.toml`,
  `crates/desktop`) and the typed POC routes (`dial-tcp`, `start-server`,
  `bind-udp`, `tls-probe`, `dial-echo`, `echo-udp`, `parse-address`).

### Fixed

- `owasidea init --preset full` (the default) now scaffolds `ui/bridge.ts`
  alongside `ui/main.ts`, so `vite build` on a freshly scaffolded app resolves
  `./bridge.ts` instead of failing with "Could not resolve" (G0). Unknown
  presets are rejected, and the `minimal` preset still writes no `ui/`.
- `/app/{name}` filters guest response headers to an allowlist, drops
  `set-cookie`/`access-control-*`, adds `nosniff` and CSP `sandbox`, and serves
  HTML/JS/CSS as `text/plain`. The optional tailcat routes
  are registered only when the plugin is loaded or
  `OWASIDEA_ENABLE_TAILCAT=1` is set. `owasidea build` propagates
  `render_targets` failures instead of reporting a stale runtime descriptor as a
  successful build (G1).
- UI plugin bridge binds the handshake to the verified document: the dashboard
  mounts the iframe with a one-time `__owasidea_bridge` nonce, validates
  `event.source`/nonce on both sides, arms only the first `load`, and closes the
  port without re-sending `init` after any navigation or reload. In-flight calls
  are cancelled once, replay ids are evicted instead of cleared, and the bridge
  timeout honours the manifest (10 s default, 30 s cap) (G1).
- Windows CI signing steps (plan, PFX import, NSIS bundle, Authenticode) run
  only on push/tag builds, never on `pull_request` (G1).
- `owasidea validate --json` now emits a stable `{ok, plugins, errors,
  warnings}` document (per-plugin status, ABI warnings included) and still exits
  non-zero on drift; text output is unchanged without `--json` (FIX-02).
- `owasidea bundle` stages `manifests/`, components and `ui/public/` into the
  output directory, and `owasidea verify dist/` fails with `NO_MANIFESTS`
  instead of silently passing on an artifact-only directory (FIX-05).
- CI builds and verifies `deb` + `rpm`, uploads `linux-packages`, and runs the
  AppImage job with `--isolate-host-libs`, verification, upload and a safe skip
  when `bwrap` is unavailable (FIX-05).

## [1.0.0] - 2026-10-27

First supported release. The `v1.0.0` tag is cut by the release gate
(`docs/RELEASE.md`); this section is the source for the release notes.

### Added

- **Interactive dashboard** (`owasidea tui`, and `owasidea dev` in a terminal):
  a ratatui client of the host HTTP API with plugin list, service state,
  Overview / Manifest / Call / Logs / Output views, reload (`r`), service
  start/stop (`s`), rebuild (`R`), open UI (`o`), an interactive call form with
  JSON args, and a `?` help overlay. Works without a TTY by falling back to the
  plain log; `--no-tui` forces it.
- **Attribute contracts without WIT** for every headless language (Rust, Go,
  TypeScript, C#, C, C++):
  `wit_source = auto|wit|attributes` per plugin. Attribute builds render
  `.owasidea/wit` (gitignored), point Cargo/componentize-go/jco/componentize-dotnet
  at it, and derive the manifest from the real component. `owasidea build`
  fails on drift instead of silently choosing a contract, prunes stale worlds,
  converts Rust/Go/TS/C# names to WIT kebab-case, and `plugin new` scaffolds
  these languages without a `wit/` directory. The C/C++ Makefile bug (`\t`
  literal, unrendered `EXE`) was fixed and C/C++ now use the `api` interface
  with sanitized `_` identifiers.
- **Minimal example apps** under `examples/`: `hello` (Rust library + UI),
  `notes` (host KV and the `kv` capability), `ticker` (service lifecycle and
  the 409 reload rule). Each has smoke checks and Playwright tests;
  `scripts/test-examples.sh` runs them all. Index: `examples/README.md`.
- `owasidea tui` and `dev --no-tui` tests in `crates/cli/tests/cli.rs`; reducer
  and render tests for the dashboard in `crates/cli/src/tui/`.

- **Host runtime**: Wasmtime 50 Component Model, WASI 0.2 + 0.3, capability
  broker with deny-by-default host interfaces, per-call instances, fuel/memory/
  timeout limits, persistent KV (SQLite), filesystem preopens, HTTP allowlist,
  SSE logs, `/metrics` and optional OpenTelemetry spans.
- **Plugins**: Rust, Go, TypeScript, C#, C, and C++ templates; library,
  executable (`wasi:http`), hybrid, service, and webview UI (`js-wasm`) kinds;
  build-time composition via `wasm-tools compose`.
- **Manifests**: generated from the component's real WIT, with
  `schema_version`, `host_abi`, `min_host`, per-file UI digests, `build_id`, and
  Ed25519 signing.
- **CLI**: `init`, `doctor`, `plugin new`, `build`, `dev`, `run`, `test`,
  `smoke`, `inspect`, `call`, `logs`, `client`, `bundle`, `latest`, `validate`,
  `verify`, `keygen`, `sign`, `signing plan`, `config render`, `registry
  search|install|update|outdated`, `explain`, `schema`, `status`.
- **Toolchain installer**: `owasidea doctor --install-all`, repeatable
  `--install <tool>`, and the compatibility `--fix` now build a platform-aware
  plan from `tools.lock` for every required and optional toolchain
  (`rustc`/`cargo`, cargo-component, wasm-tools, wit-bindgen, Go, Node/npm,
  jco, .NET, componentize-go/dotnet, trunk, wasmtime, python3 and wasi-sdk). Dry-run is the
  default; `--yes`/`--execute` runs it, `--allow-sudo` opts into `sudo`
  commands, `--force` replans present tools, and `--json` emits
  `{tool, required, present, action, command, platform, reason}` entries plus a
  summary. Commands run in dependency order and duplicates are executed once.
  `--platform linux|macos|windows` previews another OS without executing it. On
  Windows, winget commands accept the source/package agreements and the
  installer/probe PATH includes the usual cargo, Go, Node/npm, dotnet and WinGet
  directories, so a freshly installed tool is visible to the next command in
  the same run.
- **Desktop**: Tauri 2 shell embedding the host; deb/rpm/AppImage/NSIS
  packaging; CSP rendered from `wasm-app.toml`; optional host-lib isolation for
  AppImage builds.
- **UI plugins**: isolated iframe bundles with per-file SHA-256 digests,
  `ui-plugins.json` index, and an opt-in `bridge` allowlist with a same-origin
  SDK.
- **Release**: profiles (`dev|staging|prod`), updater channels (`stable|beta`),
  `latest.json` generation and `--check`, Windows Authenticode and macOS
  Developer ID/notarization plans, reproducible `build_id`.
- **Docs**: `GET_STARTED`, `AI_AGENTS`, `CLI`, `API`, `ARCHITECTURE`,
  `PLUGIN_AUTHORING`, `UI_PLUGINS`, `SECURITY`, `REGISTRY`, `UPDATER`,
  `CODE_SIGNING`, `RELEASE`, `ABI`, `ABI_POLICY`, and the migration guide.

### Changed

- `owasidea dev` opens the interactive dashboard when stdin and stdout are a
  terminal. Scripts and CI keep the plain log. Use `--no-tui` to keep the log in
  a terminal too. The host and UI output are captured in the dashboard instead
  of printed.
- Framework version is `1.0.0` (`owasidea --version`, `HOST_VERSION`, SBOM tool
  version). It was `0.1.0` in the workspace manifest.

- `wasm-app.toml` is the single source of truth for host/UI ports and origins;
  `config render` derives runtime, Playwright, and Tauri config from it.
- Config validation rejects unknown keys instead of ignoring them.
- `validate` checks manifest ABI fields, checksums, and capability drift;
  `verify` applies the same checks (plus signatures) to a `dist/` tree.
- Stateful services use explicit start/stop/status; reload while running
  returns HTTP `409` instead of guessing.

### Deprecated

| Deprecated | Since | Replacement | Earliest removal |
| --- | --- | --- | --- |
| Manifest without `schema_version` (implicit `0`) | 1.0 | `schema_version: 1` via `owasidea build` | Host ABI 2 |
| Manifest without `host_abi` / `min_host` | 1.0 | generated by `owasidea build` | Host ABI 2 |

`owasidea validate` prints `ABI_LEGACY_MANIFEST` /
`ABI_MISSING_HOST_FIELDS` warnings and still exits `0`. Removal requires at
least two host minors and six months; see `docs/ABI_POLICY.md`.

### Fixed

- `owasidea dev` in a terminal no longer lets the UI dev server read keys from
  the dashboard. Vite's own shortcuts (`r`, `o`, `q`) fired on the same keys.
- CI now uses sccache plus separate cargo registry/tool caches, Node 22 for
  jco's native binding, `npm ci --include=optional`, and an explicit
  `wit-bindgen` install; the Windows job runs on PR/main/release/tags rather
  than every feature push.
- Go `wasi:http` plugins that carry their world in the SDK
  (`go.wasmcloud.dev/component` / `componentize-go.toml`) now build with
  `componentize-go build` when there is no `wit/` and no prebuilt `main.wasm`;
  Shipyard's `webhook-go` builds from a clean checkout.
- The Output of the dashboard shows child logs without ANSI escape codes.

- Stop-service drops the component instance instead of calling the guest stop
  path, which hung with pending async sockets.
- Long-call jobs enforce limits, cancellation, and panic isolation (feature
  `streaming`, off by default).

### Security

- Plugins are untrusted by default: host imports are enforced against the
  generated manifest, filesystem/socket/HTTP access is deny-by-default, and
  manifests are signed.
- Non-loopback host binds require `OWASIDEA_ALLOW_NON_LOOPBACK=1`.
- Limits: WASI capability reduction is not OS-level isolation; see
  `docs/SECURITY.md`.

### Migration

0.x apps keep working. Run `owasidea build` and `owasidea validate` to drop the
legacy-manifest warnings. Full checklist: `docs/MIGRATION_0_X_TO_1_0.md`.

## [0.1.0]

Internal 0.x line (no tagged release): vertical slice (host, CLI, UI, Rust/Go
plugins), then hardening (limits, KV, SSE logs, generic `/call`, services, Tauri
shell, composition, polyglot templates, registry, signing). No stability
guarantees; manifests predate `schema_version`/`host_abi`/`min_host`.
