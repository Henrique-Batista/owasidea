# Roadmap 1.0 — Status tracker

Status do plano: **concluído** (release local; tag/push com aprovação)
Última atualização: 2026-10-27
Fonte de escopo: [`ROADMAP_IMPLEMENTATION_PLAN.md`](ROADMAP_IMPLEMENTATION_PLAN.md)

## Legenda

Status permitidos: `todo`, `doing`, `review`, `done`, `blocked`.

- `todo` — não iniciado; dependências podem ou não estar prontas.
- `doing` — em implementação em uma worktree.
- `review` — diff pronto, aguardando review/gates.
- `done` — mergeado na `release/1.0`, gates verdes, tracker atualizado.
- `blocked` — bloqueado; detalhes em [`BLOCKERS.md`](BLOCKERS.md).

## Como atualizar

- Atualizar no **mesmo commit** do item (ou commit `docs(status): ...` imediato).
- Preencher `Worktree`, `Commit` e `Gates` quando o item passar a `done`/`doing`.
- Itens não iniciados usam `—` em Worktree/Commit/Gates/Notas.
- Não inventar commits ou worktrees para itens não feitos.

## Tracker

| ID | Status | Worktree | Commit | Gates | Notas |
| --- | --- | --- | --- | --- | --- |
| B0.1 | done | — | 0ee8461 | fmt/clippy/test verdes | baseline git + exemplo versionado |
| B0.2 | done | — | c68e1dc (merge fc86770) | YAML válido + fmt; CI remoto não executado | CI Linux + Windows |
| B0.3 | done | main (release/1.0) | 99db2ae | docs | tracker + blocker template |
| B0.4 | done | wt-audit | 19c0a7e (merge 63591ca) | cargo test + Shipyard build/smoke PASS; 1 FAIL (validate --json) | auditoria em `AUDIT_V1.md`; 6 FIX abertos |
| B0.5 | done | — | 150fac8 (merge 2af19d0) | fmt/clippy/test verdes | seam de configuração |
| B0.6 | done | wt-config | 408731e (merge 50583c1) | fmt/clippy/test verdes | schema v1 + `deny_unknown_fields` + docs/CLI.md |
| B0.7 | done | wt-authoring | cadc41e (merge e6e85a8) | fmt/check verdes | convenções de módulos em AGENTS.md/ARCHITECTURE.md |
| RF-01 | done | wt-host | 244042b (merge 818e197) | fmt/clippy/test + Shipyard build | valida addr/ui_addr/paths; porta 0 só bloqueada no bundle |
| RF-02 | done | wt-config | 0d238a2 (merge 0f13255) | fmt/clippy/test + render/--check no Shipyard | `config render` runtime/tauri/playwright |
| RF-03 | done | wt-authoring | db75c0e (merge f55f6f8) | REV + SEC aprovaram | ADR-0003 Accepted: initialization script + runtime.json |
| RF-04 | done | wt-ui | e726a4b (merge 95944f3) | fmt/clippy/test + Shipyard build/smoke + desktop check | runtime.json + initialization script; shell do exemplo; testes Playwright adicionados (não executados) |
| RF-05 | done | wt-host | 89648c7 (merge 0b8b05d) | fmt/clippy/test + SEC aprovado | CSP estrita gerada; bundle usa `tauri.generated.conf.json`; paths confinados |
| RF-06 | done | wt-config | c7b0221 (merge 7edf0a7) | fmt/clippy/test + Shipyard build | porta UI com fallback; `tauri.dev.conf.json`; Playwright lê env |
| RF-07 | done | wt-host | 3b8f10e + 118743a (merge 10e83b5) | fmt/clippy/test + build determinístico + runtime check | build_id em manifests/runtime//overview/UI; fallback manifest |
| RF-08 | done | wt-ui | ebc0b4e (merge 83e246b) | fmt/clippy/test; Playwright adicionado (não executado) | banner de skew UI/host + warning no dev |
| RF-09 | done | wt-config | 9911b66 (merge 6219525) | build direto sem env falha; com env passa | Vite exige OWASIDEA_HOST; docs atualizadas |
| DL-01 | done | wt-config | 48b62a5 (merge 529d331) | fmt/clippy/test + desktop check + Shipyard build/smoke | `dev --desktop`; host/Vite/Tauri sem duplicação; shutdown polling |
| DL-02 | done | wt-config | e88b7b7 + ef52057 (merge b7aa2bc) | fmt/clippy/test + help flags | `--open/--no-ui/--no-host/--no-watch`; erros propagam; sem host duplicado |
| DL-03 | done | wt-config | 1a3b597 (merge 1491d46) | fmt/clippy/test + call real no Shipyard | `call <plugin>.<fn>`; args inline/@file/stdin; timeout; erros |
| DL-04 | done | main (release/1.0) | 1288751 | fmt/clippy/test + `--app` real | `call --app` com `--method/--path/--body`; `/app/{plugin}` |
| DL-05 | done | wt-config | c64f4b5 (merge 2d97eb1) | fmt/clippy/test | histórico + SSE follow; filtro de nível; `--json` |
| DL-06 | done | wt-host | 9dc2f94 + 1cc36b9 (merge 5c6ede5) | fmt/clippy/test + status real no Shipyard | health/build_id/plugins/services; `--watch`; JSON estável |
| DL-07 | done | wt-ui | 5794bc9 (merge 0833b5d) | fmt/clippy/test | bloco next steps; stale bundle; links viewer |
| DL-08 | done | wt-ui | 1006869 + 10cf610 (merge 6628999) | cargo gates + UI build + Playwright 8/8 | watcher de `plugins/*/dist`; índice versionado; reload de iframe |
| CT-01 | done | wt-contracts | 06bd591 (merge ed25c20) | fmt/clippy/test isolado + Shipyard build | `params`/`results` estruturados; interface exports preservados |
| CT-02 | done | wt-contracts | b7c0d48 (merge 1c7dc55) | fmt/clippy/test + Shipyard build | parser/validador de tipos com proptest; build valida params/results |
| CT-03 | done | wt-contracts | 09e811e (merge d15aad6) | fmt/clippy/test + client --check no Shipyard | `createClient` TS tipado; `CallError`; timeout; `--check` |
| CT-04 | done | wt-ui | 1fc070c + 03caa8e (merge 9de58de) | cargo gates + UI build + Playwright 8/8 | forms genéricos por `params`; resultado/erro/ms; snippets |
| CT-05 | done | wt-authoring | ae399b0 + 1a26839 (merge 131dccb) | REV aprovado; SEC aprovado com ressalvas | ADR-0004: MessageChannel + nonce + allowlist; SDK same-origin; gates WebKit/WebView2 |
| CT-06 | done | wt-ui | 1ad071f + e68a3e5 (merge e72fdbe) | cargo gates + UI build + Playwright 11/11; SEC aprovado | bridge opt-in MessageChannel/nonce/allowlist; timeout/cancel/reload |
| CT-07 | done | wt-ui | 4fda7b1 + 3e17d75 + d0f9547 (merge 22e5850) | Playwright raiz 11/11 + Shipyard 7/7 | SDK same-origin; board-svelte chama changelog-ts.render |
| CT-08 | done | wt-host | bf344eb + 63b308f (merge 28cd8b1) | fmt/clippy/test + Shipyard build/smoke; SEC aprovado | schema_version/host_abi/min_host; legado aceito; semver |
| AX-01 | done | wt-config | 6e79863 (merge d2326cf) | fmt/clippy/test | templates em `crates/cli/templates/`; main.rs sem strings grandes |
| AX-02 | done | wt-config | 97e5b84 (merge 48b0b94) | fmt/clippy/test + scaffold Svelte build real | Svelte/React/Vue/Yew/Blazor/Uno; `ui_build`/base `./` |
| AX-03 | done | wt-authoring | 4688d11 (merge 06f026f) | fmt/clippy/test | README com build/call/permissões/teste; WIT C# sem duplicação |
| AX-04 | done | wt-config | c978b06 (merge 58dc4d3) | fmt/clippy/test + dry-run sem arquivos | `--template/--with-test/--no-register/--dry-run`; presets init |
| AX-05 | done | wt-authoring | c1399e9 (merge 2561634) | fmt/clippy/test + UI build + Playwright 11/11 | catálogo `OWASIDEA_E_*`; `explain`; UI mostra hint/docs |
| AX-06 | done | wt-config | 5689baf + fc95870 (merge fec01b4) | fmt/clippy/test | `doctor --fix/--yes`; `tools.lock`; dry-run sem sudo |
| AX-07 | done | wt-authoring | 861cd23 + 1b82fea (merge 09f3ce2) | fmt/clippy/test + schema --check | JSON Schema (`additionalProperties:false`); comando `schema`; docs |
| AX-08 | done | wt-authoring | 47cc2db (merge b50c283) | fmt/clippy/test | `plugin add` com preview de permissões; `--dry-run`; sem overwrite |
| PX-01 | done | wt-host | 0b96e3c + 4f4abaa (merge b0fa228) | fmt/clippy/test (71 unit + 13 CLI) | `permissions.lock.json`/diff; `--fail-on-widen`; endpoint/UI |
| PX-02 | done | wt-config | 6fb3ad8 + dd5ca92 (merge 8807e80) | fmt/clippy/test (75 unit + 13 CLI) | `test --generate [--force]`; defaults por tipo; não sobrescreve |
| PX-03 | done | wt-config | 6c27e77 (merge 5e6a7bf) | fmt/clippy/test (76 unit + 16 CLI) | `--filter/--plugin/--watch`; TestOptions |
| PX-04 | done | wt-config | 8c48009 (merge 14d212a) | fmt/clippy/test | `plugins.lock`; `install --locked`; `update`; `outdated`; digest/assinatura |
| PX-05 | done | wt-authoring | 24fce84 + 0038e64 (merge 2fe869e) | fmt/clippy/test (16 CLI) | `verify <dist>`; manifests/componentes/assinaturas/UI; `--json` |
| PX-06 | done | wt-host | 2b2a1b6 + d438898 (merge 30f9a66) | fmt/clippy/test (host 6+2 integração) | LogEntry ts/level/plugin/request_id/fields; SSE JSON; compat `logs` |
| PX-07 | done | wt-host | d1f1ae6 (merge b21bbfc) | fmt/clippy/test (host 12+2) | métricas por plugin requests/errors/latency/fuel em `/metrics` |
| PX-08 | done | wt-host | 2bb0044 (merge 01834d3) | fmt/clippy/test + Playwright 12/12 | `/service/restart`; last_error/restarts; UI restart |
| PX-09 | done | wt-config | e60d558 (merge 13a11e8) | fmt/clippy/test | shell helper cmd/sh; paths/temp; CI Windows fmt/clippy/test; docs |
| RL-01 | done | wt-config | d5ec289 (merge 9c38779) | fmt/clippy/test (86 unit + 17 CLI) | `--profile`; overlays `wasm-app.<profile>.toml`; dist/<profile> |
| RL-02 | done | wt-release | 981cef4 (merge 90be49b) | fmt/clippy/test (100 unit + 19 CLI) + cargo check desktop + latest.json local | `tauri-plugin-updater`; `[updater]`/env; `owasidea latest` + `--check`; CI opcional; docs/UPDATER.md |
| RL-03 | done | wt-release | d1f83a6 (merge 3f6b697) | fmt/clippy/test (118 unit + 22 CLI), YAML parse, skip/dry-run | `signing plan`; PFX/Azure Windows + Developer ID/notarization; CI opcional |
| RL-04 | done | wt-release | f49af67 (merge 3e61bc7) | fmt/clippy/test (124 unit + 23 CLI) + script SBOM | CycloneDX/SPDX; provenance; `SOURCE_DATE_EPOCH`; bundle/CI |
| RL-05 | done | wt-host | b6b1a66 (merge 434383d) | fmt/clippy/test com e sem feature `otel`; teste live OTLP | spans `plugin.call`/`http.request`; endpoint OTLP; desligado sem feature/env |
| RL-06 | done | wt-host | 5ce286c + 4b5e8d0 (merge 8fcea64) | fmt/clippy/test com/sem `streaming` (32 streaming); SEC aprovado | ADR-0005; jobs/SSE/cancel; limites/panic/bind; ressalvas menores |
| RL-07 | done | wt-authoring | 48f8f66 (merge 9a90578) | fmt/clippy/test (124 unit + 26 CLI) | ABI_POLICY + migration 0.x→1.0 + CHANGELOG; validate warns legacy |
| G0 | done | main (release/1.0) | 38b2b9c + 1565d1b + 6ac48c9 (merge 6b8589a) | REV aprovado; sem blocker aberto | docs bridge/client/ADR reconciliados; `init` gera `bridge.ts`; handshake endurecido no Shipyard |
| G1 | done | main (release/1.0) | 65b3e6d + 6ac48c9 (merge b76ad6d) | SEC aprovado; sem crítico/alto | timeouts POC + headers saneados; bridge fail-closed; CSP/updater/registry/capabilities revisados |
| G2 | done | main (release/1.0) | d337025 + a4cd9ad (merge 1035a3a) | Linux verde: fmt/clippy/test + Playwright 40/40 + Shipyard 7/7 + build/smoke + deb + `verify dist` 11; Windows **não executado** | CI remoto não executado; matriz em `V1_RELEASE_REPORT.md` §3 |
| G3 | done | main (release/1.0) | 19a5222 | startup p95 117 ms; ping 1.1 ms; TS 1.8 ms; build 8.9 s; deb 23.6 s/25.4 MB — todos dentro do orçamento | `docs/plans/PERFORMANCE.md`; cold start 1× |
| G4 | done | main (release/1.0) | 79724a8 | link checker: 180 links, 0 broken; docs index; claims audit | `docs/README.md`; GET_STARTED/API/CLI/UI_PLUGINS/PLUGIN_AUTHORING/WINDOWS/REGISTRY/ROADMAP/SECURITY/ABI_POLICY/MIGRATION/CODE_SIGNING/UPDATER/RELEASE/ERRORS/OPTIONAL_TOOLCHAINS/plans linkados |
| G5 | done | main (release/1.0) | 5af2140 + este commit | 0 blockers; relatório final; tag `v1.0.0` local criada; push não realizado | `docs/plans/V1_RELEASE_REPORT.md`; G0/G1 aprovados; G2 Linux verde/Windows não; G3/G4 done |

## FIX — gaps da auditoria v1.0 (B0.4)

| ID | Status | Worktree | Commit | Gates | Notas |
| --- | --- | --- | --- | --- | --- |
| FIX-01 | done | wt-host | 7d56647 (merge 35f77ad) | fmt/clippy/test (26 host) | testes WAT: fuel/memória/timeout/max_instances; CPU-bound gap documentado |
| FIX-02 | done | wt-config | 864057f (merge 292667b) | fmt/clippy/test (136 unit + 34 CLI) | `validate --json` estável; warnings ABI; texto preservado |
| FIX-03 | done | wt-authoring | e7e522b (merge b43513f) | fmt/clippy/test + script templates | `scripts/test-templates.sh`; Rust/Go/TS build; CI fast |
| FIX-04 | done | wt-host | 26a625f + 7bfde17 (merge ffd9d7a) | fmt/clippy/test (26 unit + 3 e2e `reload_sse`) | reload 200, serviço 409, SSE JSON sem duplicar histórico |
| FIX-05 | done | wt-release | 328f5fb | fmt/clippy/test (139 unit + 36 CLI); YAML/TOML parse | CI full: deb+rpm + `verify dist/` + upload; AppImage condicional (`bwrap`/`--isolate-host-libs`) com verify/upload/skip; `bundle` estagia manifests/componentes/trust/UI |
| FIX-06 | done | wt-release | 328f5fb | fmt/clippy/test; links e `tools.lock` validados | `docs/OPTIONAL_TOOLCHAINS.md`; `tools.lock` com cargo-component/wasm-tools/go/node/npm; GET_STARTED/WINDOWS/README linkam |
| FIX-07 | done | wt-host | 40b1c02 (merge 975855d) | fmt/clippy/test + Shipyard host assinado + Playwright 7/7 | verificação de assinatura sobre JSON cru; interop CLI/host |
