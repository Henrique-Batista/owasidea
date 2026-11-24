# Tasks

## 1. Schema and validation

- [x] 1.1 Add `native_build` and `native_entry` to `PluginSection` in `crates/cli/src/config.rs`, keeping `deny_unknown_fields`; verify `cargo test -p owasidea-cli` still parses every existing example config
- [x] 1.2 Implement native UI validation (type/runtime pairing, webview-field rejection, argv non-empty, path containment and existence after build) and unit-test each rejection in `crates/cli/src/validate.rs`; verify `cargo test -p owasidea-cli native_ui` passes
- [x] 1.3 Print the trust warning for each native UI during build and add a unit test asserting the warning names the plugin; verify with a fixture app containing one native UI
- [x] 1.4 Document the two config fields and the trust boundary in `docs/PLUGIN_AUTHORING.md`; verify the documented snippet is accepted by `owasidea validate` on the fixture app

## 2. Build, manifest, and discovery

- [x] 2.1 Implement native UI build execution in a new `crates/cli/src/native_ui.rs` (argv in plugin dir, digest, entry resolution); unit-test success, missing binary, and non-zero command exit
- [x] 2.2 Generate `native-ui.json` after build with `schema`, `trust`, and per-plugin `name`, `binary`, `sha256`; verify a fixture build produces the exact expected JSON and that no `*.manifest.json` is written for the plugin
- [x] 2.3 Extend `owasidea verify` to check `native-ui.json` digests and report native UI counts; verify tests cover intact, missing, and tampered binaries
- [x] 2.4 Teach the host to read `native-ui.json`, merge entries into `GET /plugins` with no exports, and skip them for `/plugin/{name}/call` and `/app/{name}`; verify with a host integration test and a malformed-index test (warning, host still starts)
- [x] 2.5 Update `docs/API.md` and `docs/UI_PLUGINS.md` with the `native-ui` entry shape, the discovery rule, and the absence of HTTP dispatch; verify the documented `GET /plugins` example matches the integration test fixture

## 3. Lifecycle

- [x] 3.1 Add host readiness probing (30 s deadline) before spawning native UIs in `owasidea run`; verify a fake host that never binds causes a non-zero exit and no child process
- [x] 3.2 Implement the supervisor in `crates/cli/src/native_ui.rs`: spawn with `OWASIDEA_HOST`, `OWASIDEA_PLUGIN`, `OWASIDEA_MANIFESTS`, prefixed logs, no auto-restart, reap on exit; verify with a long-running fixture binary and a crashing fixture binary
- [x] 3.3 Add `--no-native-ui` and graceful no-display handling; verify `--no-native-ui` starts only the host and that an unset display reports a warning while the host stays up
- [x] 3.4 Wire start/stop/restart/status into the `owasidea dev` dashboard TUI; verify with a unit test on the supervisor state machine and a manual `owasidea dev` run
- [x] 3.5 Document run/dev behaviour and controls in `docs/CLI.md`; verify the documented keys match the TUI help output

## 4. Packaging

- [x] 4.1 Stage native UI binaries in `owasidea bundle --target cli` under `native/<name>/`, rewrite `native_entry`, restore the executable bit, and extend the bundle report; verify with a fixture bundle and an executable-bit assertion
- [x] 4.2 Add Tauri sidecar packaging for desktop bundles (`binaries/<name>-<triple>`, generated `externalBin` entry) and fail fast on cross-target requests; verify filename and config assertions in `crates/cli/tests/cli.rs`
- [x] 4.3 Extend `dist_stage`/bundle verification to cover native UI binaries and update `docs/BUILD_LAYOUT.md`; verify `owasidea verify dist/` passes on an intact fixture and fails on a tampered one

## 5. Scaffold, example, and end-to-end

- [x] 5.1 Add the `native-ui` branch to `owasidea plugin new` with a `gpui` template pinned to `gpui = "=0.2.2"`; verify `--type native-ui --lang gpui` generates a plugin that `owasidea build` compiles, and that `--runtime js-wasm` is rejected
- [x] 5.2 Add `examples/gpui-counter` (own `[workspace]`, pinned lockfile) with a window that calls a Rust library plugin through `OWASIDEA_HOST`; verify it builds and, on a display, displays the call result
- [x] 5.3 Add the example to `scripts/test-examples.sh` in headless mode and document GPU/display prerequisites in `docs/OPTIONAL_TOOLCHAINS.md`; verify the script passes with `--no-native-ui`
- [x] 5.4 Update `docs/GET_STARTED.md` and `docs/GET_STARTED.pt-BR.md` with a native UI walkthrough; verify the commands run as written on the example

## 6. Integration checks

- [x] 6.1 Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`; verify all pass
- [x] 6.2 Build and bundle `examples/gpui-counter` for both `--target cli` and desktop on Linux; verify the bundle contains the sidecar/copy, `owasidea verify dist/` passes, and the launcher starts the host without a display
- [x] 6.3 Run `scripts/test-examples.sh` and confirm existing examples are unaffected; verify no manifest or bundle diff appears for apps without native UIs
