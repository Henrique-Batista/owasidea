# Owasidea documentation

Index of the documentation set. Start with `GET_STARTED.md`; the reference app
is `../examples/shipyard`. Small complete apps for learning and testing live in
`../examples/` (see `../examples/README.md`); the interactive dashboard is
described in `CLI.md` (`dev` and `tui`).

## Start here

- [GET_STARTED.md](GET_STARTED.md) — install, scaffold, build, run, package.
- [GET_STARTED.pt-BR.md](GET_STARTED.pt-BR.md) — guia em português.
- [USAGE.md](USAGE.md) — setup, commands and day-to-day use.
- [AI_AGENTS.md](AI_AGENTS.md) — deterministic playbook for agents and CI.

## Reference

- [CLI.md](CLI.md) — every `owasidea` command and flag.
- [API.md](API.md) — HTTP API, call contract, metrics, streaming jobs, typed client.
- [ARCHITECTURE.md](ARCHITECTURE.md) — host, runtime, manifests, capability broker.
- [ERRORS.md](ERRORS.md) — stable `OWASIDEA_E_*` codes and `owasidea explain`.
- [ABI.md](ABI.md) — manifest `schema_version`/`host_abi`/`min_host` fields.
- [ABI_POLICY.md](ABI_POLICY.md) — semver, breaking changes, deprecation window.
- [MIGRATION_0_X_TO_1_0.md](MIGRATION_0_X_TO_1_0.md) — upgrade a 0.x app to 1.0.

## Plugins and UI

- [PLUGIN_AUTHORING.md](PLUGIN_AUTHORING.md) — language-specific build outputs and WIT.
- [SOURCE_GENERATORS.md](SOURCE_GENERATORS.md) — `// owasidea:` source markers.
- [UI_PLUGINS.md](UI_PLUGINS.md) — static webview bundles, iframe sandbox, bridge SDK.

## Security and operations

- [SECURITY.md](SECURITY.md) — trust boundaries, enforced controls, residual risks.
- [REGISTRY.md](REGISTRY.md) — signed HTTPS registry, install flow, lockfile.
- [RELEASE.md](RELEASE.md) — release checklist, gates, profiles, CI secrets.
- [CODE_SIGNING.md](CODE_SIGNING.md) — Windows Authenticode, macOS Developer ID/notarization.
- [UPDATER.md](UPDATER.md) — Tauri updater, `latest.json`, channels, local testing.
- [WINDOWS.md](WINDOWS.md) — native Windows support and cross-compilation limits.
- [APPIMAGE.md](APPIMAGE.md) — AppImage packaging and clean-build requirements.
- [CLEAN_BUILD.md](CLEAN_BUILD.md) — bubblewrap and chroot clean builds without Docker.
- [OPTIONAL_TOOLCHAINS.md](OPTIONAL_TOOLCHAINS.md) — jco, .NET/C#, wasi-sdk, trunk, `doctor --fix`.
- [ROADMAP.md](ROADMAP.md) — shipped milestones and remaining backlog.

## Plans and status (1.0)

- [plans/ROADMAP_IMPLEMENTATION_PLAN.md](plans/ROADMAP_IMPLEMENTATION_PLAN.md) — scope, phases, gates.
- [plans/ROADMAP_STATUS.md](plans/ROADMAP_STATUS.md) — item-by-item tracker and gate status.
- [plans/V1_RELEASE_REPORT.md](plans/V1_RELEASE_REPORT.md) — G5 release report: test matrix, benchmarks, SBOM hashes, residual risks, tag.
- [plans/PERFORMANCE.md](plans/PERFORMANCE.md) — G3 startup/call/build/bundle measurements and limits.
- [plans/AUDIT_V1.md](plans/AUDIT_V1.md) — v0–v1.0 audit and FIX gaps.
- [plans/BLOCKERS.md](plans/BLOCKERS.md) — blocker log template and entries.

## Architecture decisions

- [adr/0001-name-and-ui.md](adr/0001-name-and-ui.md) — project name and UI approach.
- [adr/0002-shipyard-reference-app.md](adr/0002-shipyard-reference-app.md) — reference application.
- [adr/0003-runtime-host-discovery.md](adr/0003-runtime-host-discovery.md) — runtime descriptor and host discovery.
- [adr/0004-ui-plugin-bridge.md](adr/0004-ui-plugin-bridge.md) — MessageChannel bridge, nonce, allowlist.
- [adr/0005-streaming-long-calls.md](adr/0005-streaming-long-calls.md) — jobs, SSE and cancellation.

## Status of 1.0 claims

Keep these labels in sync with code and tests; do not upgrade a claim without
evidence.

| Claim | Status |
| --- | --- |
| Typed TypeScript client (CT-03) | Implemented: `owasidea client --lang ts -o <file> [--check]`; `createClient`, `CallError`, timeout. |
| UI plugin bridge (ADR-0004) | Implemented and opt-in via `bridge = ["plugin.export"]`; parent-mediated `MessageChannel`, nonce, allowlist, timeout/cancel. |
| Streaming jobs (RL-06) | Experimental and feature-gated (`--features streaming`); routes are absent without it. Bridge stays request/response. |
| WebView2 (Tauri Windows) | **Not validated.** Chromium and WebKit are validated; WebView2 is a required gate before claiming support. Engines that reject the port transfer must fail closed (`BRIDGE_DISABLED`, iframe stays isolated); there is no fallback implementation. |
| Windows toolchain paths | Implemented, but a Windows CI run is still the final proof; Linux tests are not Windows validation. |
| Performance budgets (G3) | Measured in [`plans/PERFORMANCE.md`](plans/PERFORMANCE.md): startup, call p50/p95, incremental build and deb bundle all within budget on the recorded Linux host. Cold-start distribution, load tests and Windows/macOS are not measured; do not generalize the numbers. Gate closure is tracked in `ROADMAP_STATUS.md`. |
