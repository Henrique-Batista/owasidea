# Plano de correções — Owasidea 1.1 (DX e limpeza)

Status: **1.1.0 aplicado** (registry e assinatura de plugin removidos, CLI bundle,
timeouts de teste, KV sem arquivo vazio, TUI ratatui com mouse, build capturado,
demo raiz/rotas POC removidos).
Data: 2026-10-28. Base: `release/1.0` (`0b95023`, tag `v1.0.0`).

## 0. Premissas

- Owasidea **não é mais um POC**: é um produto 1.0. A documentação deve refletir
  o estado atual, não o escopo original.
- Alvos: aplicativos **desktop (Tauri)** e **CLI/headless**. O target CLI ainda
  não tem empacotamento; isso é parte deste plano.
- Plugins são repositórios de código + assets. **Não haverá registry remoto,
  marketplace ou assinatura de plugin no caminho padrão.**
- Integridade de artefatos continua via SHA-256 e validação de manifest.
- Prioridade: experiência do desenvolvedor, remoção de legado, menos código e
  menos dependências.

## 1. Diagnóstico (evidências)

### 1.1 Assinatura de plugin quebra o exemplo `hello`

- `crates/host/src/lib.rs:950-956`: `require_signatures` assumia
  `!cfg!(debug_assertions)`. Um CLI instalado com `cargo install` é release,
  portanto exigia assinatura mesmo sem `[signing]` no `wasm-app.toml`.
- `examples/hello/wasm-app.toml` não tem `[signing]`.
- Resultado: o exemplo mais simples falha em release com “plugin is unsigned but
  signatures are required”, sem instrução clara.
- **Aplicado:** o default virou opt-in (`false` em debug e release). Para exigir
  assinatura, use `[signing].require = true` ou
  `OWASIDEA_REQUIRE_SIGNATURES=true`. A remoção total da assinatura fica como
  decisão separada (ver Fase 1B).

### 1.2 Registry remoto não foi pedido

- `crates/cli/src/registry_lock.rs`, comandos `registry search|install|update|outdated`
  em `crates/cli/src/main.rs:400-441`, `docs/REGISTRY.md`, testes e CI.
  **Removido neste trabalho**: comando, módulo, testes, docs e menções em README,
  `docs/README`, `llms.txt`, `ARCHITECTURE`, `SECURITY`, `UI_PLUGINS`, `ROADMAP`
  e ADR-0001.
- O produto é “plugins como repositórios de código”; registry reinventa GitHub e
  adiciona chave pública, lockfile, update/outdated e superfície de segurança.

### 1.3 `owasidea test` pode travar

- Relato do usuário: “trava e não faz nada”.
- Hipóteses a confirmar: espera de host/smoke/Playwright sem timeout, watcher,
  processo filho sem kill, `--watch` acidental, ou teste de UI sem `CI`.
- A correção precisa: timeouts explícitos, propagação de exit code, kill de
  processos filhos e mensagem de fase atual (“building”, “host”, “smoke”, “ui”).

### 1.4 `owasidea bundle` exige um shell desktop do repositório

- Relato: procura `./crates/desktop/tauri.generated.conf.json`.
- `docs/CLI.md:483-500` documenta `[desktop].path` → `desktop/` → `crates/desktop/`.
  Para um app sem `[desktop]`, o fallback aponta para o shell do próprio repo.
- Não existe `bundle --target cli` nem documentação de empacotamento CLI.

### 1.5 Erros de build somem no TUI de `dev`

- O dashboard captura stdout/stderr do build (`Output`), mas o estado de falha
  não aparece como erro no menu; só aparece ao sair.
- Correção: um `BuildStatus` no estado do TUI (running/ok/failed + última linha
  de erro), banner persistente e tecla para abrir o log completo.

### 1.6 Dívida estrutural e legado

- `crates/cli/src/main.rs`: 7.209 linhas; `crates/host/src/lib.rs`: 4.102 linhas.
- `crates/macros` era uma cópia morta de `crates/owasidea`; removido neste
  trabalho.
- `scripts/gen-manifest.py`, `scripts/build-all.sh`, `scripts/compose*.sh` e o
  app demo da raiz (`plugins/`, `manifests/`, `data/`) duplicam `examples/` e o
  build do CLI. **Removidos nesta fatia:** os scripts legados e os plugins
  `composed`, `composed-go` e `notes-client`.
- `docs/ARCHITECTURE.md` citava `crates/macros`; corrigido.
- `docs/ROADMAP.md` dizia que G3 não foi medido; `PERFORMANCE.md` tem medições.
  Corrigido.
- `package.json` da raiz apontava para `index.js` inexistente; corrigido.

### 1.7 KV e rede não estão claros

- O `hello` **não usa KV** (`examples/hello/plugins/greeter/src/lib.rs` e
  `wasm-app.toml` não declaram `kv`). O manifest gerado tem `"kv": null`.
- Permissões são declaradas no `wasm-app.toml`, viram manifest no build e são
  aplicadas em runtime pelo host. HTTP/rede são allowlists exatas; não há
  “descobrir a URL em runtime”.
- Falta um documento único de modelo de execução para o autor de plugin.

### 1.8 TUI atual

- `crates/cli/src/tui/` usa ratatui (immediate mode), ~1.9k linhas, integrada ao
  `dev`.
- Limitações percebidas: pouca hierarquia visual, build error escondido, sem
  séries temporais/insights, atualização por polling.
- Protótipos comparáveis criados em `tools/tui-lab/` (ratatui, Bubble Tea,
  OpenTUI) com o mesmo dataset e os mesmos estados.

## 2. Fases

### Fase 0 — Baseline e decisões (1 dia)

- Congelar `release/1.0`; trabalhar em branch `cleanup/1.1`.
- Rodar e registrar: `cargo fmt --check`, `cargo clippy`, `cargo test`,
  `scripts/test-examples.sh hello`, `owasidea build/smoke/test` no `hello`.
- Confirmar as decisões: remover registry + assinatura; manter checksums;
  escolher TUI; aceitar CLI target.
- Critério de saída: baseline verde (ou falhas conhecidas documentadas) e
  decisões registradas em ADR curto.

### Fase 1 — Remover registry e assinatura de plugin (P0)

**1A — Registry (feito).** Removidos `crates/cli/src/registry_lock.rs`, comandos
`Registry`/`install`/`update`/`outdated`, flags de `plugin add`, testes,
`docs/REGISTRY.md` e menções em README/docs. Não há `plugins.lock`.

**1B — Assinatura (feito).** Removidos:

- `keygen`, `sign`, `[signing]`, trust root, `verify --public-key`
- `OWASIDEA_REQUIRE_SIGNATURES`, `OWASIDEA_PUBLIC_KEY`, verificação Ed25519
- `ring`/`base64` onde só existiam por isso
- seções em `SECURITY`, `GET_STARTED`, `CLI`, `MIGRATION`, `PLUGIN_AUTHORING`,
  `README`, exemplos e CI
- passos de `keygen` no CI e no shipyard

Manter (em qualquer caso):

- SHA-256 dos componentes e do manifest
- `owasidea validate` para checksums/capabilities
- `owasidea verify dist/` sem assinatura (checagem de integridade)

Critério de saída: `examples/hello` roda com `cargo install --path crates/cli`
sem chave; `cargo test` verde; docs sem menção a registry/assinatura de plugin.

### Fase 2 — Corrigir o loop de desenvolvimento (P0)

**Aplicado nesta fatia:**

- `owasidea test`: timeout de request (60 s), timeout de UI (30 min), kill do
  filho, detecção de host que morreu antes de `/health`, stdin nulo para
  `npx`/`npm`, mensagens de fase (`building`, `api smoke`, `ui`) e `--watch` que
  não engole mais falhas.
- `owasidea dev`: `build_error` persistente no estado do TUI; footer vermelho
  “BUILD FAILED …” até um rebuild bem-sucedido.
- `owasidea bundle`: não cai mais em `crates/desktop` silenciosamente; exige
  `[desktop].path` ou `desktop/`; `--target cli` gera `dist/cli/` com binário,
  manifests, componentes e `run.sh`/`run.bat`.
- `owasidea dev` agora roda o build inicial como child capturado quando o TUI
  está ativo: as linhas aparecem em Output e a falha vira banner persistente.
- Pendente: teste de integração dedicado para cada falha acima.

### Fase 3 — Reorganizar a documentação (P1)

- Reescrever `AGENTS.md` para 1.0: alvos, escopo, não-objetivos, layout real,
  comandos de verificação e convenções.
- ADR-0006 propõe glue UI, chamadas host-mediated tipadas, dependências
  cíclicas, múltiplos KV e sandbox graduado (`trusted`/`sandboxed`).
- Criar `docs/RUNTIME_MODEL.md` (ou seção no `PLUGIN_AUTHORING.md`) com:
  - tipos de plugin e o que cada um pode fazer;
  - UI → plugin: rotas HTTP, quais exports são chamáveis, filtros;
  - plugin → plugin: composição em build time vs chamada via host/UI;
  - KV: namespace, acesso, SQLite, isolamento;
  - rede: `http_outbound`, `network_outbound`, `network_inbound`, resolução DNS,
    o que acontece quando nega;
  - filesystem: preopens e `OWASIDEA_DATA_DIR`;
  - quando o sandbox age (build vs load vs runtime);
  - exemplo `hello` sem KV, sem assinatura.
- Corrigir contradições: G3, `crates/macros`, status de push/tag, POC vs 1.0.
- Remover docs de registry/assinatura e links órfãos.
- Critério de saída: um desenvolvedor responde às perguntas de UI/KV/rede lendo
  só o guia; link checker sem links quebrados.

### Fase 4 — TUI (P1)

- Protótipos prontos em `tools/tui-lab/`: ratatui, Bubble Tea e OpenTUI, com o
  mesmo dataset, abas, log, métricas, testes e banner de build failure.
- Tamanhos medidos: ratatui 11 MB (debug, sem runtime), Bubble Tea 5,6 MB (sem
  runtime), OpenTUI 53 MB de `node_modules` + 5 MB nativo e exige Bun.
- Recomendação: **manter ratatui**. O problema atual não é o framework: o
  reducer já é MVU/Elm-like e testado; faltam dados (métricas/SSE) e estado de
  build. Bubble Tea é a alternativa se o time aceitar Go; OpenTUI fica dev-only.
- Ratatui escolhido. Aplicado: mouse capture, abas clicáveis, lista de plugins
  clicável e botões do footer clicáveis (`r`, `s`, `R`, `o`, `?`, `q`).
- Próximo: abas Overview/Plugins/Logs/Metrics/Tests/Build, SSE em vez de polling,
  sparkline de latência, visualização de testes/benchmarks, command palette,
  scroll por aba e output rolável.
- Critério de saída: protótipo escolhido vira o dashboard real; build error e
  métricas visíveis sem sair do TUI; teste de render/update.

### Fase 5 — Endurecer HTTP/rede (P1, pendente)

Corrigir os pontos encontrados na auditoria:

- **Porta:** `http_outbound = ["api.example.com:443"]` (porta opcional) e
  comparar `uri.port_or_known_default()`; hoje o allowlist ignora a porta.
- **DNS pinning:** resolver o host uma vez por store e conectar ao IP fixado
  (ou validar o IP conectado) para impedir rebinding; hoje o `wasi:http`
  resolve no momento da requisição.
- **Redirects:** documentar que o host não segue redirect; se o guest seguir,
  a nova requisição é checada. Adicionar teste.
- **Scheme:** rejeitar `http://` para hosts não-loopback quando o app exigir
  TLS; manter loopback para dev.
- **Sockets:** avaliar allowlist de portas (`host:port`) e documentar que hoje
  não há restrição de porta.
- **Testes:** host deny → erro `http-request-denied`; DNS rebinding; porta
  errada; redirect.

Não implementado em 1.1.0; entra na 1.2/ABI 2 por mudar o formato do manifest.

### Fase 6 — Performance de compilação e dependências (P2)

Auditoria medida: CLI normal 325 pacotes; host 258; desktop 415. Duplicatas
notáveis: `base64`, `syn` 2/3, `thiserror` 1/2, `toml` 0.8/1.1, `reqwest`
0.12/0.13, `hashbrown` 0.14/0.16/0.17.

- **P0 aplicado:** `reqwest` normal removido do host (ficou só em dev-deps),
  `tokio-stream` removido, `dialoguer` com `default-features = false`,
  `[profile.dev] debug = 1` e `reqwest` com `default-features = false` +
  `rustls-tls` (a cadeia OpenSSL saiu; CLI normal caiu de 325 para 302 pacotes).
- **P1:** `toml 1.x` para deduplicar com Wasmtime; podar features default do
  Wasmtime (cache/cranelift/runtime/component-model/async/pooling; remover
  profiling/gc/threads) e rodar Shipyard smoke.
- **P1:** feature `runtime` no CLI ou crate de contrato para comandos que não
  precisam do host; feature `updater` no desktop; feature `tui` para CI mínimo.
- **P2:** separar `main.rs` (7.209 linhas) em módulos; `ratatui` só se o TUI
  continuar no binário; tokio features por crate.
- Meta: reduzir o build incremental do CLI e o número de crates; medir antes/depois.
- Nota: o `~/.cargo/config.toml` local usa `opt-level=3` para dependências; isso
  domina builds locais e não representa o CI.

### Fase 7 — Limpeza de legado e higiene (P2)

- App demo da raiz removido: `plugins/`, `manifests/`, `data/`, `wasm-app.toml`,
  `crates/desktop` e `scripts/test-ui.sh`. Os scripts `build-all.sh`,
  `compose*.sh`, `run-host.sh` e `gen-manifest.py` também foram removidos, junto
  com os plugins `composed`, `composed-go` e `notes-client`.
- Rotas tipadas de POC removidas do host (`dial-tcp`, `start-server`, `bind-udp`,
  `tls-probe`, `dial-echo`, `echo-udp`, `parse-address`) e da API/docs.
- `examples/notes/crates/` (cópia de config), `examples/hello/owasidea.pub` e o
  trait `Test` não usado no greeter foram removidos.
- Centralizar `CARGO_TARGET_DIR` dos exemplos em `.owasidea/` na raiz; limpar
  artefatos antigos.
- Revisar `.gitignore` e remover do disco os 1,4 GB de build em `examples/`.
- Critério de saída: árvore limpa, `git status` sem lixo, build dos exemplos
  reproduzível a partir de zero.

## 3. Ordem de execução

1. Fase 0 (baseline).
2. Fase 1 (registry/signing) — maior impacto no `hello`.
3. Fase 2 (test/dev/bundle) — maior impacto diário.
4. Fase 3 (docs) — consolida as decisões.
5. Fase 4 (TUI) — depois do loop de dev estar confiável.
6. Fase 5/6 (perf/legado) — contínuo, com medições.

## 4. Próxima etapa

As decisões de produto foram fechadas e registradas como ADR-0006. O plano
implementável da próxima versão está em
[`IMPLEMENTATION_1_2.md`](IMPLEMENTATION_1_2.md): começa com spike obrigatório de
Wasmtime, depois ABI 2/dependências, host RPC, glue UI, multi-KV e rede.

## 5. Riscos

- Remover assinatura muda o modelo de confiança; manter checksums e documentar
  que a integridade não autentica o autor.
- `bundle --target cli` é um novo formato de distribuição; começar com diretório
  + launcher, não com binário único auto-extraível.
- Trocar TUI pode atrasar o loop de dev; por isso a Fase 2 vem antes.
- Remover o app demo da raiz pode quebrar `tests/ui-smoke.spec.ts`; migrar os
  testes para `examples/hello` ou manter o app até a migração.
- `cargo install` release é o caminho real do usuário; testar sempre em release,
  não só `cargo run` debug.

## 6. Resultados desta fatia

- Registry removido do CLI, testes, docs e README; `owasidea --help` não lista
  mais o comando.
- Assinatura de plugin removida (host, CLI, `verify`, dist, shipyard, CI e
  docs). Manifests são unsigned e verificados por SHA-256.
- Demo raiz (`plugins/`, `manifests/`, `data/`, `wasm-app.toml`,
  `crates/desktop`) e rotas POC removidos.
- TUI ratatui com mouse: abas, lista de plugins e botões do footer clicáveis.
- Build inicial do `dev` capturado no Output, com banner persistente de falha.
- Versão `1.1.0` no workspace, `package.json` e `CHANGELOG`.
- `owasidea bundle --target cli` gera `dist/cli/` e o launcher sobe o host;
  `/health` respondeu `ok` no teste manual.
- `owasidea test` ganhou timeouts e não fica pendurado em host/UI; smoke do
  `hello` passa.
- KV sem permissão não cria mais `<data>/<plugin>/kv.sqlite` (verificado).
- `cargo test` (host 40 + CLI 215 unit + integração) verde; `clippy -D warnings`
  verde; `fmt --check` verde.
- `owasidea smoke` do Shipyard: 11 manifests assinados, 9 componentes WASI, 2 UI,
  serviço e `wasi:http` verdes.
- 325 → 302 pacotes normais no CLI; OpenSSL removido.
- Protótipos de TUI em `tools/tui-lab/` (ratatui, Bubble Tea, OpenTUI).

## 7. Critérios de aceite globais

- `examples/hello`: `build`, `smoke`, `test`, `dev`, `bundle --target cli` sem
  chave e sem shell desktop.
- `examples/notes` e `examples/ticker`: build/smoke/test verdes.
- `examples/shipyard`: build/smoke/test e bundle desktop passam via app-glue.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
  verdes.
- Docs: link checker sem quebras; `AGENTS.md` sem “POC”; nenhum doc prometendo
  registry/assinatura.
- TUI: erro de build visível, logs/SSE, métricas e testes na mesma tela.
