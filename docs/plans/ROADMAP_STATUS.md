# Roadmap 1.0 — Status tracker

Status do plano: **em execução**
Última atualização: 2026-10-26
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
| RF-05 | todo | — | — | — | — |
| RF-06 | done | wt-config | c7b0221 (merge 7edf0a7) | fmt/clippy/test + Shipyard build | porta UI com fallback; `tauri.dev.conf.json`; Playwright lê env |
| RF-07 | done | wt-host | 3b8f10e + 118743a (merge 10e83b5) | fmt/clippy/test + build determinístico + runtime check | build_id em manifests/runtime//overview/UI; fallback manifest |
| RF-08 | done | wt-ui | ebc0b4e (merge 83e246b) | fmt/clippy/test; Playwright adicionado (não executado) | banner de skew UI/host + warning no dev |
| RF-09 | done | wt-config | 9911b66 (merge 6219525) | build direto sem env falha; com env passa | Vite exige OWASIDEA_HOST; docs atualizadas |
| DL-01 | todo | — | — | — | — |
| DL-02 | todo | — | — | — | — |
| DL-03 | todo | — | — | — | — |
| DL-04 | todo | — | — | — | — |
| DL-05 | todo | — | — | — | — |
| DL-06 | todo | — | — | — | — |
| DL-07 | todo | — | — | — | — |
| DL-08 | todo | — | — | — | — |
| CT-01 | done | wt-contracts | 06bd591 (merge ed25c20) | fmt/clippy/test isolado + Shipyard build | `params`/`results` estruturados; interface exports preservados |
| CT-02 | done | wt-contracts | b7c0d48 (merge 1c7dc55) | fmt/clippy/test + Shipyard build | parser/validador de tipos com proptest; build valida params/results |
| CT-03 | todo | — | — | — | — |
| CT-04 | todo | — | — | — | — |
| CT-05 | todo | — | — | — | — |
| CT-06 | todo | — | — | — | — |
| CT-07 | todo | — | — | — | — |
| CT-08 | todo | — | — | — | — |
| AX-01 | todo | — | — | — | — |
| AX-02 | todo | — | — | — | — |
| AX-03 | todo | — | — | — | — |
| AX-04 | todo | — | — | — | — |
| AX-05 | todo | — | — | — | — |
| AX-06 | todo | — | — | — | — |
| AX-07 | todo | — | — | — | — |
| AX-08 | todo | — | — | — | — |
| PX-01 | todo | — | — | — | — |
| PX-02 | todo | — | — | — | — |
| PX-03 | todo | — | — | — | — |
| PX-04 | todo | — | — | — | — |
| PX-05 | todo | — | — | — | — |
| PX-06 | todo | — | — | — | — |
| PX-07 | todo | — | — | — | — |
| PX-08 | todo | — | — | — | — |
| PX-09 | todo | — | — | — | — |
| RL-01 | todo | — | — | — | — |
| RL-02 | todo | — | — | — | — |
| RL-03 | todo | — | — | — | — |
| RL-04 | todo | — | — | — | — |
| RL-05 | todo | — | — | — | — |
| RL-06 | todo | — | — | — | — |
| RL-07 | todo | — | — | — | — |
| G0 | todo | — | — | — | — |
| G1 | todo | — | — | — | — |
| G2 | todo | — | — | — | — |
| G3 | todo | — | — | — | — |
| G4 | todo | — | — | — | — |
| G5 | todo | — | — | — | — |

## FIX — gaps da auditoria v1.0 (B0.4)

| ID | Status | Worktree | Commit | Gates | Notas |
| --- | --- | --- | --- | --- | --- |
| FIX-01 | todo | — | — | — | P1: testes de estouro de `max_fuel`/`timeout_ms`/`max_memory_mb`/`max_instances` (mapear em PX-07) |
| FIX-02 | todo | — | — | — | P1: `owasidea validate --json` deve emitir JSON estável |
| FIX-03 | todo | — | — | — | P1: compilar templates Rust/Go/TS nos testes (mapear em AX-03) |
| FIX-04 | todo | — | — | — | P2: teste e2e de hot reload/409 e SSE (mapear em DL-05/PX-06) |
| FIX-05 | todo | — | — | — | P2: verificar bundle Tauri deb/rpm/AppImage no CI (mapear em RL/CI) |
| FIX-06 | todo | — | — | — | P2: documentar toolchains opcionais ausentes (mapear em AX-06) |
