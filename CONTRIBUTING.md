# Contributing

## Development setup

See docs/USAGE.md. Run `owasidea doctor` to verify toolchain.

## Workflow

1. Fork/branch.
2. Make changes; keep host independent of Tauri/UI.
3. Add tests for new capabilities (manifest derivation, capability enforcement, CLI).
4. Run:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   owasidea --config examples/shipyard/wasm-app.toml smoke
   owasidea --config examples/shipyard/wasm-app.toml test
   ```
5. Update docs (USAGE/API/CLI) for behavior changes.
6. Open PR with description and test evidence.

## Coding conventions

- Rust: rustfmt default, clippy -D warnings.
- Manifests are generated; never commit hand-edited manifests (they are gitignored).
- Do not commit large `.wasm` artifacts unless explicitly requested.
- Record non-obvious decisions in docs/adr/.

## Reference app

`examples/shipyard` is the canonical end-to-end example. Any framework change
that affects manifests, build paths, composition, UI plugins, or Tauri packaging
should be verified against it. See `docs/GET_STARTED.md` and
`docs/AI_AGENTS.md`.

## Scope

POC-level: vertical slice first. Production hardening (fuel, persistent KV, signing) tracked separately.