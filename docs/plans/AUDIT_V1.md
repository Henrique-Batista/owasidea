# Audit v0–v1.0 (B0.4)

Data: 2026-10-26
Commit base: `2af19d0` (release/1.0)
Branch/worktree: `wt-audit`

## Ambiente

Ferramentas encontradas (doctor --json):
- Obrigatórias: rustc, cargo, cargo-component, wasm-tools, go, node, npm — OK.
- Opcionais presentes: clang (C/C++), dotnet (C#), wasi-sdk (+ reactor adapter), python3.
- Opcionais ausentes: jco (JS/TS), componentize-dotnet (C#), trunk (Yew UI).
- Playwright: CLI 1.64.1 instalado, mas browsers do Playwright não instalados (`~/.cache/ms-playwright` ausente) — smoke de browser não executado.
- Tauri CLI (`cargo-tauri`) presente, mas build desktop não exercitado.

`CARGO_TARGET_DIR=/mnt/scratch/build-artifacts/cargo`; `OWASIDEA_DATA_DIR=/tmp/opencode/audit-data`.

## Resultado dos gates

- `cargo build -p owasidea-cli`: PASS.
- `cargo test`: PASS (host 5 testes, CLI 10 testes de integração + 4 unit, config 8, dev 2).
- `owasidea doctor --json`: PASS.
- Shipyard `owasidea build`: PASS (11 plugins, incluindo C#, C, C++, Go, Rust, TS).
- Shipyard `owasidea smoke`: PASS (todos os checks, incluindo serviço start/status/stop e `wasi:http`).
- `owasidea inspect --json`: PASS. `owasidea validate --json`: FAIL (saída não é JSON).
- Host temporário em `127.0.0.1:8899`: `/health` e `/metrics` PASS.

## Checklist de auditoria

| # | Item (seção 9) | Status | Evidência |
|---|---|---|---|
| 1 | `max_fuel`, `timeout_ms`, `max_memory_mb`, `max_instances` aplicados; teste que estoura cada limite | PASS (aplicado) | `crates/host/src/lib.rs`: `set_fuel`, `StoreLimitsBuilder::memory_size`, `tokio::time::timeout`, `instance_slots` Semaphore. Sem teste dedicado de estouro. |
| 2 | KV SQLite persiste entre reinícios; preopen `reports/` grava | PASS | `open_kv_db` (`kv.sqlite`); smoke `core-rust.save-report`/`load-report` OK. |
| 3 | Reload re-lê manifest e rejeita serviço rodando (409) | PASS | `reload()` lê manifest do disco; retorna `StatusCode::CONFLICT` se serviço ativo/tailcat. |
| 4 | HTTP outbound allowlist bloqueia host não declarado | PASS | `AllowlistHooks::send_request` compara host com `permissions.http.outbound`; retorna `HttpRequestDenied`. |
| 5 | SSE entrega novas linhas e não duplica histórico | PASS | `plugin_logs_stream` usa `last` id e filtra `id > last`. |
| 6 | `/call` valida aridade/tipo e preserva `result`/`results` | PASS | `call_export`: checa `ty.params().count()`, `json_to_val`; retorna `result` (1) ou `results` (N). |
| 7 | Composição build-time gera manifest sem `imports.plugins` | PASS | `examples/shipyard/manifests/pipeline-go.manifest.json`: `imports.plugins == []`. |
| 8 | Serviço persiste instância entre start/status/stop | PASS | `state.services` HashMap; smoke watchtower start/status/stop OK. |
| 9 | C# e TS carregam e executam no host | PASS | Smoke: `policy-csharp.classify`, `changelog-ts.render`. |
| 10 | Tauri abre, carrega `ui/dist`, chama host e empacota deb/rpm/AppImage | NOT VERIFIED | `cargo-tauri` presente, mas build desktop/pacotes não executado; sem artefatos. |
| 11 | UI plugins verificam assets e rodam em iframe sandbox | PASS | `ui/main.ts`: `verifyUiPluginAssets` (SHA-256) + `sandbox="allow-scripts allow-forms"`. |
| 12 | Hot reload de library funciona; serviço exige stop | PASS (código) | `reload()` bloqueia serviço ativo; hot-reload library via `/plugin/{name}/reload`. Não exercitado end-to-end. |
| 13 | `dev` reserva portas e watcher ignora `node_modules`/`target` | PASS | Testes `dev_reserves_concrete_host_and_ui_ports`, `dev_watcher_extracts_wasi_plugin_and_skips_webview_plugins`. |
| 14 | Templates criam projetos que buildam (Rust/Go/TS) | NOT VERIFIED | Scaffolds Rust/Go/C++ gerados (teste de registro), mas não compilados; apenas UI scaffold (Yew) tem build no teste. |
| 15 | `inspect`, `validate`, `doctor` retornam JSON estável | FAIL | `inspect --json` e `doctor --json` OK; `validate --json` ignora a flag e imprime texto. |
| 16 | Registry instala artefato assinado e rejeita checksum inválido | PASS | `crates/cli/tests/cli.rs`: `registry_install_downloads_only_after_checksum_and_signature_validation`. |
| 17 | Capabilities negam import não declarado | PASS | `validate_component_imports` rejeita interface não declarada. |
| 18 | `/metrics` expõe contadores | PASS | Host temporário: `owasidea_requests_total`, `owasidea_errors_total`. |
| 19 | `docs/SECURITY.md` reflete o que é/não é isolado | PASS | Documento lista controles aplicados e riscos residuais. |

Contagem: PASS 16, FAIL 1, NOT VERIFIED 2.

## Gaps (FIX-*)

- **FIX-01 (P1)** — Ausência de testes que estouram `max_fuel`, `max_memory_mb`, `timeout_ms` e `max_instances`.  
  Evidência: limites aplicados em `crates/host/src/lib.rs`, mas nenhum teste cobre estouro.  
  Sugestão: adicionar testes de integração com plugin que loopa/aloque/exceda instâncias e esperar erro/timeout.

- **FIX-02 (P1)** — `validate --json` não emite JSON.  
  Evidência: `cmd_validate` ignora flag global; saída textual.  
  Sugestão: suportar `--json` retornando objeto estruturado (status por plugin).

- **FIX-03 (P1)** — Templates Rust/Go/TS/C/C++/C# não são compilados nos testes.  
  Evidência: `crates/cli/tests/cli.rs` valida registro de scaffold, mas não build dos projetos gerados.  
  Sugestão: teste de build (pelo menos Rust e Go) no CI.

- **FIX-04 (P2)** — Hot reload e streaming SSE não têm teste end-to-end.  
  Evidência: implementação por inspeção; sem cobertura automatizada de reload/409 e SSE.  
  Sugestão: teste HTTP que recarrega library e consome `/logs/stream`.

- **FIX-05 (P2)** — Empacotamento Tauri (deb/rpm/AppImage) não verificado neste ambiente.  
  Evidência: nenhum artefato gerado; build desktop não executado.  
  Sugestão: rodar `owasidea bundle` em CI Linux e validar artefatos.  
  **Resolvido (wt-release):** CI full constrói `deb,rpm`, roda `owasidea verify dist/` e publica `linux-packages`; job AppImage instala `bubblewrap`, usa `--isolate-host-libs`, verifica e publica, com skip seguro quando `bwrap` não funciona. `bundle` passa a estagiar manifests/componentes/trust/UI em `dist/` e `verify` falha com `NO_MANIFESTS` em vez de passar vazio.

- **FIX-06 (P2)** — Ferramentas opcionais ausentes (jco, componentize-dotnet, trunk) limitam build de UI TS/C#/Yew.  
  Evidência: `owasidea doctor --json`.  
  Sugestão: documentar instalação ou marcar como opcionais no CI.  
  **Resolvido (wt-release):** `docs/OPTIONAL_TOOLCHAINS.md` cobre versões, instalação por SO, `doctor --fix`/`tools.lock` e limitações; `tools.lock` ganhou cargo-component/wasm-tools/go/node/npm; GET_STARTED e WINDOWS linkam o documento; CI Linux instala .NET 10 + wasi-sdk.

## Limitações da auditoria

- Não foram instalados browsers do Playwright nem toolchains opcionais ausentes; smoke de UI/browser e builds dependentes não foram exercitados.
- Tauri bundle não foi construído por custo/tempo; apenas scaffold inspecionado.
- Auditoria não implementou correções; apenas documentou gaps.
- Itens marcados PASS baseiam-se em código + testes existentes e, em parte, inspeção; nem todos têm teste dedicado (ver FIX-*).
