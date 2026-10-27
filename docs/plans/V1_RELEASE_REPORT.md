# Owasidea 1.0 — Relatório de release (G5)

Status: **pronto para release local**; **0 blockers**.
Data: 2026-10-27. Branch: `release/1.0`. Base: `79724a8` (+ o commit deste
relatório). Escopo e critérios: [`ROADMAP_IMPLEMENTATION_PLAN.md`](ROADMAP_IMPLEMENTATION_PLAN.md).
Tracker por item: [`ROADMAP_STATUS.md`](ROADMAP_STATUS.md). Blockers:
[`BLOCKERS.md`](BLOCKERS.md) (vazio).

## 1. Resumo

- 1.0 está pronto para **release local**: `build`, `smoke`, `bundle deb` e
  `verify dist/` passam no Shipyard; nenhum item `blocked`.
- **G0** (architecture review) e **G1** (security review) aprovados.
- **G2**: matriz Linux verde; **Windows não executado** (nem CI remoto).
- **G3**: todos os benchmarks dentro do orçamento.
- **G4**: documentação com 180 links verificados e **0 quebrados**.

## 2. Itens done/blocked e commits

Todos os itens do plano estão `done`; **nenhum `blocked`**. A lista
item-a-item, com worktree, commit e gates, é a fonte de verdade em
[`ROADMAP_STATUS.md`](ROADMAP_STATUS.md). Resumo por fase:

| Fase | Escopo | Itens | Status | Evidência |
| --- | --- | --- | --- | --- |
| 0 | Baseline, CI, auditoria | B0.1–B0.7 | done | tracker; [`AUDIT_V1.md`](AUDIT_V1.md); `.github/workflows/ci.yml` |
| 1 | Config única, identidade, runtime | RF-01–RF-09 | done | tracker (RF-01…RF-09) |
| 2 | Loop de desenvolvimento | DL-01–DL-08 | done | tracker (DL-01…DL-08) |
| 3 | Contratos, clients, UI bridge | CT-01–CT-08 | done | tracker; ADR-0004 |
| 4 | Authoring UX | AX-01–AX-08 | done | tracker (AX-01…AX-08) |
| 5 | Produtividade | PX-01–PX-09 | done | tracker (PX-01…PX-09) |
| 6 | Release e operação | RL-01–RL-07 | done | tracker; ADR-0005 |
| 7 | Gate 1.0 | G0–G5 | done | este relatório |

Gaps abertos pela auditoria da Fase 0 (`FIX-01`…`FIX-07`): todos `done`, com
teste e doc no mesmo item. Destaques:

| ID | Entrega | Commit | Gate |
| --- | --- | --- | --- |
| FIX-01 | testes WAT de fuel/memória/timeout/max_instances | `7d56647` (merge `35f77ad`) | 26 host |
| FIX-02 | `validate --json` estável + warnings ABI | `864057f` (merge `292667b`) | 136 unit + 34 CLI |
| FIX-03 | `scripts/test-templates.sh` (Rust/Go/TS) | `e7e522b` (merge `b43513f`) | templates buildam |
| FIX-04 | reload 200 / serviço 409 / SSE sem duplicar histórico | `26a625f` + `7bfde17` (merge `ffd9d7a`) | 3 e2e `reload_sse` |
| FIX-05 | CI full: deb+rpm, AppImage condicional, `verify dist/`, upload | `328f5fb` | fmt/clippy/test + YAML/TOML |
| FIX-06 | `docs/OPTIONAL_TOOLCHAINS.md` + `tools.lock` | `328f5fb` | links + lock |
| FIX-07 | assinatura sobre JSON cru + interop CLI/host | `40b1c02` (merge `975855d`) | host assinado + Playwright 7/7 |

## 3. Matriz de testes

Executada em Linux x86_64 nesta árvore (`release/1.0`, base `79724a8`).

| Gate | Comando/escopo | Resultado |
| --- | --- | --- |
| Formatação | `cargo fmt --check` | pass |
| Lint | `cargo clippy -p owasidea-host -p owasidea-cli --all-targets -- -D warnings` | pass, 0 warnings |
| Unit CLI | `cargo test -p owasidea-cli` (`src/main.rs`) | **147 pass**, 0 fail |
| Integração CLI | `cargo test -p owasidea-cli --test cli` | **42 pass**, 0 fail |
| Unit host | `cargo test -p owasidea-host --lib` | **34 pass**, 0 fail |
| Integração host | `--test integration` | **3 pass**, 0 fail |
| Reload/SSE | `--test reload_sse` | **3 pass**, 0 fail |
| Playwright raiz | `npx playwright test` (Chromium + WebKit) | **40/40** pass |
| Playwright Shipyard | `examples/shipyard`: `npx playwright test` | **7/7** pass |
| Shipyard build | `owasidea build` | pass; 11 manifests assinados |
| Shipyard smoke | `owasidea smoke` | pass (todos os 9 componentes + serviço + `wasi:http`) |
| Bundle | `owasidea bundle --bundles deb` | pass; `Shipyard_0.1.0_amd64.deb` |
| Verify | `owasidea verify dist --public-key keys/owasidea.pub --json` | `ok: true`, **11 manifestos** conferidos |
| SBOM | `examples/shipyard/dist/sbom.{cdx,spdx}.json` | **935 componentes** em ambos |
| Windows | host/CLI/NSIS | **não executado** |
| CI remoto | GitHub Actions | **não executado** (workflow existe; sem run) |

Nota: os totais unit/CLI/host acima são os medidos neste commit. O snapshot do
G2 registrava 144 unit + 37 CLI + 27 host; a diferença são testes adicionados
por FIX-02/FIX-05/FIX-07 depois daquele snapshot. Nada foi removido.

## 4. Benchmarks vs orçamento (G3)

Fonte: [`PERFORMANCE.md`](PERFORMANCE.md) — Linux x86_64, Ryzen 9 7900, host
**debug**, Shipyard @ `6b8589a`.

| Métrica | Medido | Orçamento | Veredito |
| --- | --- | --- | --- |
| Startup host (cache quente) | p50 114 ms / **p95 117 ms** | ≤ 300 ms | pass |
| `core-rust.ping` | p50 0.9 ms / **p95 1.1 ms** | p95 ≤ 20 ms | pass |
| `changelog-ts.render` (TS) | p50 1.7 ms / **p95 1.8 ms** | p95 ≤ 100 ms | pass |
| `owasidea build` incremental | **8.9 s** | ≤ 60 s | pass |
| `owasidea bundle --bundles deb` | **23.6 s**; deb 25.4 MB; `dist/` 52 MB | ≤ 5 min | pass |

## 5. SBOM e hashes

Artefatos em `examples/shipyard/dist/` (não versionados; regeneráveis por
`owasidea build && owasidea bundle --bundles deb`).

| Artefato | Caminho | Tamanho | SHA-256 |
| --- | --- | --- | --- |
| Pacote Debian | `examples/shipyard/dist/Shipyard_0.1.0_amd64.deb` | 25.4 MB (25 363 736 B) | `8e8c099fc71799b41905b3e18e939c4f43f3b18278ddc1cbd63b53209b85ae00` |
| SBOM CycloneDX 1.5 | `examples/shipyard/dist/sbom.cdx.json` | 520 508 B | `9b91abdc72bba4f78a8d6f7ca92e18ab759242f1dd9913726b889cfce2a73132` |
| SBOM SPDX 2.3 | `examples/shipyard/dist/sbom.spdx.json` | 364 688 B | `40c722212d7332025e46835925d95eab4df30dbb1e232b35b4a9d6cf0479ee9b` |

- Ambos os SBOMs listam **935 pacotes/componentes** (cargo + npm + Go).
- `sbom.spdx.json` tem `created: 1970-01-01T00:00:00Z`, coerente com
  `SOURCE_DATE_EPOCH` (build determinístico onde possível).
- O `.deb` contém os 11 manifestos assinados, componentes e `trust/`; a
  verificação usa a chave pública **fora** de `dist/` (falha esperada se
  `dist/trust` for usada como raiz — `TRUST_ROOT_UNPINNED`).

## 6. Riscos residuais e limitações

- **WebView2 não validado** (sem runner Windows). O bridge é *fail-closed*: sem
  handshake válido o iframe não recebe `bridge:init`; comportamento WebView2
  permanece não comprovado.
- **Cold start medido 1×** (3.55 s, cache vazio); não é distribuição e não há
  número Windows/macOS.
- **Jobs/streaming é experimental** (ADR-0005, feature `streaming`); API pode
  mudar antes de ABI 2.
- **Empacotamento**: AppImage validado apenas condicionalmente
  (`bwrap`/`--isolate-host-libs`); **NSIS** não executado; **signing real**
  (Authenticode/Developer ID/notarização) e **updater publish** não executados —
  só plano/dry-run e `latest.json` local.
- **CI Windows/remoto pendente**: workflow versionado, nenhum run registrado.
- **Dívida pós-1.0**: `crates/cli/src/main.rs` e `crates/host/src/lib.rs`
  continuam grandes; novos itens devem entrar em módulos (convenção B0.7).
- Benchmarks usam host **debug** (pessimista para host) e chamadas sequenciais
  (sem teste de carga).

## 7. Migração 0.x → 1.0

Guia: [`docs/MIGRATION_0_X_TO_1_0.md`](../MIGRATION_0_X_TO_1_0.md). 1.0 é
compatível com apps 0.x; o guia remove depreciações antes da ABI 2 (config
estrita, `schema_version`/`host_abi`/`min_host`, `validate` com warnings).
Política: [`docs/ABI_POLICY.md`](../ABI_POLICY.md); changelog:
[`CHANGELOG.md`](../../CHANGELOG.md).

## 8. Tag `v1.0.0`

A tag **local** `v1.0.0` foi criada neste commit:

```bash
git tag -a v1.0.0 -m "Owasidea 1.0.0"
```

**Push exige aprovação** (`git push origin v1.0.0`); nenhum push foi feito. A
tag pré-existente é `v1.0.0-pre` (baseline da Fase 0).

## 9. Evidências

- [`ROADMAP_STATUS.md`](ROADMAP_STATUS.md) — itens, commits, gates.
- [`PERFORMANCE.md`](PERFORMANCE.md) — metodologia e limites do G3.
- [`AUDIT_V1.md`](AUDIT_V1.md) — auditoria v0–v1.0 e origem dos FIX.
- [`../RELEASE.md`](../RELEASE.md) — checklist de release, perfis, segredos.
- [`../../examples/shipyard/AGENTS.md`](../../examples/shipyard/AGENTS.md) —
  app de referência usado em build/smoke/Playwright/bundle.
