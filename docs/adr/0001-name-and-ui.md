# ADR 0001 — Project name and UI approach

- Product and CLI name: Owasidea (`owasidea`, crates `owasidea-host` and
  `owasidea-cli`).
- App configuration: `wasm-app.toml`.
- Default UI: vanilla TypeScript + Vite. UI plugins use static browser bundles;
  Yew, Blazor, Uno, and JS/WASM run in sandboxed webview iframes only. The host
  never embeds a JS runtime.
- First generic stateful service example: Rust `heartbeat`; lifecycle contract
  is `start-service`, optional `service-status`, and host drop on stop.
- Permission UX: manifests remain the source of truth. UI shows generated
  effective permissions; no runtime file picker or capability escalation UI.
- Remote registry: static HTTPS index, immutable versioned packages, Ed25519
  publisher trust pinned out-of-band. CI publishing; no hosted account service.
