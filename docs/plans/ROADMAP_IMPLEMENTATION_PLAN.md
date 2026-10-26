# Roadmap 1.0 — Plano de implementação

Status: **planejado, não iniciado**
Última atualização: 2026-10-25
Fonte de escopo: [`docs/ROADMAP.md`](../ROADMAP.md), seção
"Backlog de DX e hardening para 1.0", mais a revalidação dos itens marcados
como concluídos em v0–v1.0.

Este documento é o contrato de execução. Ele define o que fazer, em que ordem,
com qual subagente/modelo, como revisar e quando parar. A implementação só
começa depois que o baseline da Fase 0 estiver commitado.

---

## 1. Objetivo

Levar o framework de "piloto funcional" para **1.0 utilizável por terceiros**:

- configuração única, sem portas/origens duplicadas;
- loop de desenvolvimento rápido (`dev`, `call`, `logs`, `--desktop`);
- contratos tipados entre plugins WASI, host e frontends;
- UI plugins isolados com um bridge opt-in e seguro;
- scaffolds reais, erros acionáveis e testes gerados;
- release reproduzível, assinado e observável.

O escopo inclui **todo** o backlog P0/P1/P2 do roadmap e a auditoria dos itens
v0–v1.0 já marcados. Itens marcados `[x]` não serão reimplementados; a Fase 0
valida cada um e abre `FIX-*` apenas para gaps reais.

---

## 2. Princípios

1. **Manifest é gerado, nunca editado à mão.**
2. **WIT é o contrato.** Tipos e nomes vêm do componente real.
3. **Uma fonte de verdade por configuração.** Portas, origens, CSP, build id e
   identidade saem de `wasm-app.toml`.
4. **Capabilities deny-by-default.** Nenhum item pode ampliar permissões sem
   diff visível e aprovação explícita.
5. **Host sem runtime JS.** `js-wasm` continua sendo só webview.
6. **Sem Docker obrigatório, sem fork do linuxdeploy, sem Extism.**
7. **Cada item entrega código + teste + doc.** Sem "depois eu documento".
8. **Mudança de protocolo/ABI exige ADR e review antes do código.**
9. **Commits pequenos, convencionais, um item por commit.**
10. **Nada de artefato gerado no git** (`.wasm`, `dist/`, manifests, `node_modules`).

---

## 3. Estado atual e pré-condições

### 3.1 O que já existe e deve ser preservado

- Host Wasmtime 50, WASI 0.2/0.3, capabilities, KV SQLite, logs SSE.
- Manifestos gerados de WIT real, assinatura Ed25519, registry HTTPS.
- CLI com `build`, `dev`, `run`, `smoke`, `test`, `bundle`, `inspect`,
  `validate`, `keygen`, `sign`, `registry`.
- Shipyard com 9 componentes WASI (Rust, Go, TS, C#, C, C++), composição
  build-time, serviço persistente, executável `wasi:http`, 2 UI plugins.
- Cache de compilação Wasmtime + componente compilado uma vez por load.
- `ui-plugins.json` gerado junto dos assets.
- `bundle` passa `OWASIDEA_HOST` e avisa sobre CSP divergente.
- `bundle --isolate-host-libs` (bubblewrap) para AppImage.

### 3.2 Dívidas conhecidas que a Fase 0 deve resolver

- `examples/shipyard` está **untracked** no git. Worktrees e CI não o verão.
- Há mudanças não commitadas em `crates/*`, `docs/*` e no exemplo.
- Não existe `.github/workflows/` nem CI.
- `crates/cli/src/main.rs` (~5k linhas) e `crates/host/src/lib.rs` (~2.8k)
  concentram tudo; novos itens devem entrar em módulos, sem refactor global.
- `examples/shipyard/AGENTS.md` ainda diz que `[dev].ui_addr` é fixo em 5180;
  o CLI já reserva portas. Corrigir junto de RF-05.
- `docs/UI_PLUGINS.md` foi atualizado para `ui-plugins.json`, mas docs antigos
  podem citar `/ui-plugins` como fonte de verificação.
- O manifest tem `abi`, mas o host **não valida** compatibilidade.

### 3.3 Pré-condições de ambiente

- Rust stable + `cargo-component`, `wasm-tools`, `tauri-cli`.
- Node 20+ e npm; Playwright instalado no repo.
- Go 1.27.1 com `componentize-go` como tool.
- .NET 10 + workload WASI; wasi-sdk para C/C++.
- Linux para bundle/AppImage; Windows nativo para NSIS (cross-compile não é
  suportado).
- `CARGO_TARGET_DIR` em disco grande (`/mnt/scratch/build-artifacts/cargo`).
- `OWASIDEA_DATA_DIR` em diretório temporário durante testes.

---

## 4. Modelo de execução

### 4.1 Loop do orquestrador

O agente principal (orquestrador) executa, sem pedir confirmação a cada item:

```
1. Ler docs/plans/ROADMAP_STATUS.md e escolher o próximo item "todo"
   cujas dependências estão "done".
2. Se o item exige design (ADR), despachar ARCH primeiro.
3. Criar/reutilizar worktree da trilha.
4. Despachar o subagente implementador com o prompt do item.
5. Revisar o diff e rodar os gates.
6. Se falhar: devolver a falha ao mesmo subagente (1 retry).
   Se falhar de novo: marcar "blocked", registrar em BLOCKERS.md e seguir.
7. Fazer merge/rebase na branch de release, commit, atualizar status.
8. Repetir até todos os itens estarem "done" ou "blocked".
9. Executar a Fase 7 (gate de 1.0) e gerar o relatório final.
```

O orquestrador **não** implementa itens grandes diretamente; ele planeja,
despacha, revisa e integra. Correções triviais (typo, conflito de merge) podem
ser feitas diretamente.

### 4.2 Modelo e agentes

Todos os subagentes de trabalho usam **OpenCode Go / Beam Free**:

| Código | Modelo | Uso |
| --- | --- | --- |
| `EXP` | `opencode-go/beam-free#medium` | exploração, levantamento de arquivos |
| `ARCH` | `opencode-go/beam-free#max` | ADR, desenho de protocolo/ABI/config |
| `GEN-H` | `opencode-go/beam-free#high` | implementação complexa (host, bridge, Tauri) |
| `GEN-M` | `opencode-go/beam-free#medium` | implementação padrão (CLI, templates, docs) |
| `REV` | `opencode-go/beam-free#max` | review de arquitetura/diff |
| `QA` | `opencode-go/beam-free#high` | estratégia e revisão de testes |
| `SEC` | `opencode-go/beam-free#high` | review de segurança (bridge, CSP, signing) |
| `PIPE` | `opencode-go/beam-free#high` | CI, release, SBOM, updater |

Mapeamento de `agent` no tool `subagent`:

| Código | `agent` |
| --- | --- |
| `EXP` | `explore` |
| `ARCH` | `aidlc-architect-agent` |
| `GEN-*` | `general` |
| `REV` | `aidlc-architecture-reviewer-agent` |
| `QA` | `aidlc-quality-agent` |
| `SEC` | `aidlc-devsecops-agent` |
| `PIPE` | `aidlc-pipeline-deploy-agent` |

Regras:

- Nunca usar modelo diferente de Beam Free neste plano.
- `#low` não é usado para código; no máximo para status/checklist.
- Se o modelo retornar rate limit: backoff 60s, até 3 tentativas. Se continuar
  indisponível, serializar e aguardar; só marcar bloqueado se nenhum item
  puder avançar.
- Subagentes recebem contexto novo a cada item; não herdam a conversa do
  orquestrador. O prompt precisa ser autossuficiente.
- Saída do subagente: relatório curto (≤12 linhas), sem colar diff inteiro.

### 4.3 Worktrees, branches e commits

- Branch de integração: `release/1.0` (criada na Fase 0 a partir de `main`).
- Worktrees por **trilha/fase**, não por item, para reduzir `npm install` e
  recompilação:
  - `wt-config` → RF, DL-01/DL-05
  - `wt-contracts` → CT
  - `wt-authoring` → AX, PX-02/PX-03
  - `wt-host` → PX-01/PX-07/PX-08/PX-09, CT-08 host side
  - `wt-release` → RL, CI, signing
  - `wt-ui` → UI genérica, formulário, bridge frontend
- Cada item = 1 commit convencional (`feat(cli): ...`, `fix(host): ...`,
  `docs: ...`, `test: ...`), com o ID do item no corpo.
- Antes do merge: `git fetch`/rebase na `release/1.0`, rodar gates na worktree.
- Conflitos em `crates/cli/src/main.rs`, `crates/host/src/lib.rs` e
  `examples/shipyard/ui/src/main.ts` são resolvidos pelo orquestrador, não
  pelo subagente.
- Merge com `--no-ff` para preservar o item.
- Nunca commitar `keys/`, `dist/`, `data/`, `reports/`, manifests gerados,
  `.wasm`, `node_modules/`, `ui/dist/`, `ui/public/plugins/`.

Exemplo de despacho (o orquestrador adapta o prompt ao item):

```js
const wt = await tools.opencode.worktree_create({ name: "wt-config" });
const run = await tools.subagent({
  agent: "general",
  description: "RF-01 config validation",
  model: "opencode-go/beam-free#high",
  prompt: `
    Repo: /home/pablitux/Projetos/wasm-app-framework
    Worktree: ${wt.directory}
    Leia: AGENTS.md, docs/plans/ROADMAP_IMPLEMENTATION_PLAN.md (RF-01).
    Implemente apenas RF-01. Rode os gates da seção 4.6.
    Reporte em <=12 linhas.`,
});
```

Se um agente `aidlc-*` não estiver disponível ou for pesado demais para o item,
usar `general` com o mesmo prompt; o modelo continua Beam Free.

### 4.4 Concorrência

- Máximo **2** subagentes implementadores simultâneos + 1 revisor.
- Trilhas paralelas permitidas apenas quando os arquivos são disjuntos:
  - `wt-host` (host) + `wt-authoring` (CLI/templates) podem rodar juntos.
  - `wt-ui` e `wt-contracts` podem rodar juntos se o contrato de dados já
    estiver congelado.
  - `wt-config` é serializado com `wt-host` quando ambos tocam `AppState`.
- Um item que muda `wasm-app.toml` (schema) bloqueia itens que o leem até o
  merge.
- Reviews (`REV`/`SEC`) não rodam em paralelo com merge do mesmo item.

### 4.5 Quando parar

O plano assume execução contínua. Parar **somente** se:

1. A ação for irreversível/externa: publicar registry, push de tag, assinar com
   certificado de produção, apagar dados.
2. Faltar credencial/segredo ou toolchain que não possa ser instalado e que
   bloqueie **todos** os itens restantes.
3. Houver conflito de requisito que exija decisão de produto (ex.: quebrar ABI
   pública sem política de migração).
4. Houver vulnerabilidade de segurança que exija decisão antes de continuar.

Nesses casos: registrar em `docs/plans/BLOCKERS.md`, marcar o item como
`blocked`, continuar os demais. Só encerrar quando todos estiverem `done` ou
`blocked`, e entregar o relatório final.

### 4.6 Gates de qualidade

Rodar após **cada** item (na worktree):

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Quando o item toca o exemplo/UI:

```bash
cd examples/shipyard
owasidea build
owasidea smoke
owasidea test
```

Quando toca bundle/release:

```bash
cd examples/shipyard
owasidea bundle --no-build --bundles deb
```

Regras:

- Teste que falha 2 vezes com o mesmo erro → `blocked`, com log anexado.
- Nenhum `#[allow(dead_code)]` novo sem justificativa.
- Nenhum `unwrap()` novo em caminho de request; usar erro tipado.
- `unsafe` novo só com revisão `SEC`.
- Performance: chamada de plugin não pode regredir > 2x no p95 sem justificativa.

---

## 5. Definition of Ready / Done

### Ready (item pode ser despachado)

- Dependências `done`.
- Escopo e arquivos listados.
- ADR aprovado, se o item muda protocolo/ABI/config pública.
- Worktree criada e sincronizada.
- Prompt preenchido com contexto, escopo, aceite e restrições.

### Done (item pode ser fechado)

- Código + testes + docs no mesmo commit.
- Gates verdes.
- `docs/plans/ROADMAP_STATUS.md` atualizado com commit e evidência.
- Nenhum artefato gerado commitado.
- Nenhuma meta-anti violada.
- Se aplicável: exemplo Shipyard atualizado e `owasidea test` verde.

---

## 6. Fases e work items

Legenda de tamanho: **S** ≤ 1 dia, **M** 2–3 dias, **L** 4–7 dias (por
subagente; com paralelismo o calendário é menor).

### Fase 0 — Baseline, CI e auditoria

Objetivo: garantir que worktrees, CI e testes representam o estado real antes
de mudar qualquer coisa.

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| B0.1 | Baseline git: commitar mudanças atuais em `release/1.0`, adicionar `examples/shipyard` (fonte, sem artefatos), revisar `.gitignore`, tag `v1.0.0-pre` | M | — | GEN-M | `git status` limpo; `git ls-files examples/shipyard \| wc -l > 0`; worktree novo contém o exemplo |
| B0.2 | CI Linux + Windows: fmt, clippy, test host/cli, build+smoke Shipyard, Playwright, deb | M | B0.1 | PIPE | Workflow roda em PR; Windows compila host/cli |
| B0.3 | Tracker: criar `docs/plans/ROADMAP_STATUS.md` a partir do template da seção 11 | S | B0.1 | GEN-M | Todos os IDs listados como `todo` |
| B0.4 | Auditoria v0–v1.0: executar checklist da seção 9 e abrir `FIX-*` para gaps | M | B0.2 | QA + GEN-M | Relatório em `docs/plans/AUDIT_V1.md`; gaps com ID |
| B0.5 | Seam de configuração: criar `crates/cli/src/config.rs` com `AppConfig` tipado, validação de paths/portas e loader único; migrar `read_host_addr`/`read_app_config` para ele | M | B0.1 | ARCH + GEN-H | Configs atuais carregam; testes de path/porta; sem mudança de comportamento |
| B0.6 | Congelar schema de config v1: documentar campos, `deny_unknown_fields` onde seguro, erro com chave/linha | S | B0.5 | GEN-M | Testes de config inválida; `docs/CLI.md` atualizado |
| B0.7 | Convenção de módulos: novos comandos em `crates/cli/src/<área>.rs`, novos recursos do host em `crates/host/src/<área>.rs`; registrar no `mod` sem refactor global | S | B0.1 | GEN-M | `cargo check`; nenhuma mudança funcional |

**Saída da fase:** branch `release/1.0`, CI verde, exemplo versionado, tracker e
auditoria prontos.

**Formato do CI (B0.2):**

- **Fast (todo PR):** `fmt`, `clippy`, `cargo test -p owasidea-host -p owasidea-cli`,
  `owasidea doctor --json`.
- **Full (push em `release/1.0` e tags):** Shipyard `build` + `smoke` +
  Playwright + `bundle --bundles deb`; AppImage com `--isolate-host-libs`.
- **Windows:** host/cli + plugins Rust/Go/TS + NSIS; C#/C/C++ marcados como
  opcionais até o toolchain estar pinado.
- Cachear cargo, npm, Go e Playwright browsers; instalar Chromium com
  `npx playwright install --with-deps chromium`.
- Rodar a matriz completa no máximo 2x por dia para não estourar cota.

---

### Fase 1 — Configuração única, identidade e runtime (RF)

Objetivo: eliminar as cópias de host/porta/CSP e permitir override em runtime.

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| RF-01 | Validação de config: `addr`, `ui_addr`, `desktop`, `:0` proibido em bundle, paths relativos ao config | M | B0.5 | GEN-H | Testes de todos os campos; erro claro |
| RF-02 | `owasidea config render --target runtime\|tauri\|playwright\|all [--check]` | M | RF-01 | GEN-H | Golden tests; `--check` falha se stale |
| RF-03 | ADR-0003: descoberta de host em runtime (Tauri initialization script, fallback build-time, `HOST_ADDR`) | S | RF-01 | ARCH | ADR aprovado por REV/SEC |
| RF-04 | Injeção runtime no webview: `window.__OWASIDEA__ = {host, app, version, build_id}`; UI lê runtime antes do build-time; shell do exemplo e `crates/desktop` | L | RF-03 | GEN-H | App com `HOST_ADDR` alternativo conecta sem rebuild; Playwright cobre |
| RF-05 | CSP gerada: `connect-src` a partir de `[host].addr`/origens; suporte a `http://127.0.0.1:*` local; bundle usa config gerada | M | RF-02 | GEN-H + SEC | CSP parser tests; bundle com porta não-default funciona |
| RF-06 | `devUrl`/UI port: `[dev].ui_addr`, reserva, geração de `tauri.dev.conf.json`; Playwright lê env | M | RF-02 | GEN-M | Porta ocupada → fallback; `dev` e Tauri alinhados |
| RF-07 | Identidade/build id: `build_id` (git sha + timestamp ou `SOURCE_DATE_EPOCH`) em manifests, host `/overview` e UI | M | RF-01 | GEN-H | Dois builds com epoch fixo → mesmo id; UI mostra id |
| RF-08 | Skew detector: banner "UI bundle anterior ao host" comparando `build_id`; `dev` avisa | S | RF-07 | GEN-M | Playwright simula ids diferentes |
| RF-09 | Remover defaults silenciosos: `vite.config.ts` falha se `OWASIDEA_HOST` ausente quando executado fora do CLI; docs atualizadas | S | RF-04 | GEN-M | Build direto dá erro acionável |

**Gate da fase:** mudar `[host].addr` para `127.0.0.1:8899`, rodar
`owasidea build && owasidea bundle --bundles deb`, instalar e ver o app online.
Sem editar CSP/vite/playwright à mão.

---

### Fase 2 — Loop de desenvolvimento (DL)

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| DL-01 | `owasidea dev --desktop`: host + Vite + `cargo tauri dev` com config gerada, logs prefixados, shutdown limpo | M | RF-06 | GEN-H | App Tauri abre na URL certa; Ctrl+C encerra os 3 |
| DL-02 | `owasidea dev --open` (browser) e `--no-ui/--no-host/--no-watch` | S | DL-01 | GEN-M | Flags testadas |
| DL-03 | `owasidea call <plugin>.<fn> --args <json\|@file\|-> [--json] [--timeout]` | M | RF-04 | GEN-H | Chama core-rust e changelog-ts; erros com exit code |
| DL-04 | `owasidea call --app <plugin> <method> <path>` para executáveis | S | DL-03 | GEN-M | `/app/webhook-go/health` |
| DL-05 | `owasidea logs <plugin> [--follow] [--level] [--json]` | M | DL-03 | GEN-M | Histórico + SSE; Ctrl+C limpo |
| DL-06 | `owasidea status [--watch] [--json]`: health, build_id, plugins, serviços | S | RF-07 | GEN-M | JSON estável |
| DL-07 | UX do `dev`: bloco "next steps", detecção de bundle stale, link do viewer | S | RF-08 | GEN-M | Saída legível; teste snapshot |
| DL-08 | UI plugin rebuild/reload: watcher de `plugins/*/dist`, regenera índice e recarrega iframe | M | DL-07 | GEN-H | Alterar Svelte reflete sem reiniciar |

**Gate:** `owasidea dev --desktop` → editar UI plugin → ver mudança; editar
componente Rust → hot reload; `owasidea call` e `logs` funcionam no terminal.

---

### Fase 3 — Contratos, clients e UI integration (CT)

Objetivo: transformar `signature` em contrato estruturado e gerar client.

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| CT-01 | Manifest com tipos estruturados: `exports[].params[]` e `results[]` (name/type), gerados do WIT real; manter `signature` | L | B0.5 | GEN-H | Todos os 9 plugins têm params corretos; testes golden |
| CT-02 | Parser/validador de tipos (primitivos, list, option, result, tuple, record, enum, flags) | M | CT-01 | GEN-H | Property tests; erros com path |
| CT-03 | `owasidea client --lang ts -o <file> [--check]` com `createClient`, métodos tipados, `CallError`, timeout | L | CT-01/02 | GEN-H | `tsc --noEmit`; chamada real no Shipyard |
| CT-04 | UI genérica de invocação: formulário a partir de `params`, validação, resultado/erro/ms, snippet fetch/curl | L | CT-01/02 | GEN-H | Funciona para Rust/Go/TS/C#/C/C++ sem código por plugin |
| CT-05 | ADR-0004: bridge de UI plugin (MessageChannel, nonce, allowlist, timeout, limites, ameaças) | M | CT-01 | ARCH + SEC | ADR aprovado |
| CT-06 | Bridge parent: `bridge = ["plugin.fn"]`, canal por iframe, validação de args/allowlist, timeout, erros | L | CT-05 | GEN-H | Testes de permitido/negado/aridade/timeout |
| CT-07 | SDK do bridge para UI plugin (`owasidea-bridge.js` gerado/copiado) + exemplo Svelte chamando backend | M | CT-06 | GEN-H | Board-svelte chama `changelog-ts.render`; Playwright |
| CT-08 | ABI versionado: `schema_version`, `host_abi`, `min_host`; host valida; `validate` verifica; doc de migração | M | CT-01 | GEN-H + SEC | Host rejeita ABI incompatível com erro claro |

**Gate:** gerar client, usar no dashboard, chamar todos os plugins; bridge
negado por padrão e permitido só na allowlist; manifest antigo continua aceito.

---

### Fase 4 — Authoring UX (AX)

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| AX-01 | Templates fora do `main.rs`: `crates/cli/templates/` embutidos com `include_dir` | M | B0.7 | GEN-M | Nenhuma string de template no main.rs |
| AX-02 | Scaffolds reais: Svelte, React, Vue, Yew, Blazor, Uno com Vite/base `./`, demo, `ui_build`, README, teste | L | AX-01 | GEN-H | `plugin new` + `build` funciona para Svelte/React; demais documentados |
| AX-03 | Templates headless com README, exemplo de `owasidea call`, smoke e teste; corrigir WIT duplicado do C# | M | AX-01 | GEN-M | Rust/Go/TS buildam no CI; C#/C/C++ documentados |
| AX-04 | `plugin new` flags (`--template`, `--with-test`, `--no-register`, `--dry-run`) e `init` presets (`minimal`, `desktop`, `full`, `--yes`) | M | AX-01 | GEN-M | Testes não interativos |
| AX-05 | Taxonomia de erros `OWASIDEA_E_*`, body `{code,message,hint,docs}`, `owasidea explain <code>` | M | B0.7 | GEN-H | Erros comuns mapeados; UI mostra hint; docs/errors |
| AX-06 | `doctor --fix` com comandos por SO, versões pinadas, `tools.lock`; sem sudo automático | M | AX-05 | GEN-M | Dry-run mostra comandos; lock verificado |
| AX-07 | JSON Schema de `wasm-app.toml` + `owasidea schema` + docs de editor | S | B0.6 | GEN-M | Schema válido; config do Shipyard valida |
| AX-08 | `owasidea plugin add <path\|registry>` com preview de permissões e `--dry-run` | M | AX-05 | GEN-M | Adiciona config, não sobrescreve, mostra diff |

**Gate:** `owasidea init` + `plugin new --lang svelte` → `owasidea build` →
`owasidea dev --desktop` sem editar arquivo à mão.

---

### Fase 5 — Produtividade (PX)

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| PX-01 | Diff de permissões entre builds; `--fail-on-widen`; diff no reload/UI | M | CT-01 | GEN-H | Teste com permissão ampliada falha; relatório legível |
| PX-02 | `owasidea test --generate` a partir de `params` (valores default/overrides, arquivo separado) | M | CT-01 | GEN-H | Gera smoke para os 9 plugins; não sobrescreve config |
| PX-03 | `test --filter <plugin> --watch` e `test --plugin` | S | PX-02 | GEN-M | Reexecuta só o alvo |
| PX-04 | Registry lockfile (`plugins.lock`), `install --locked`, `update`, `outdated` | M | B0.1 | GEN-H | Lock verifica digest/assinatura; teste com registry local |
| PX-05 | `owasidea verify dist/` (manifests, componentes, assinaturas, índice UI) | M | CT-08 | GEN-H | Arquivo adulterado falha; `--json` |
| PX-06 | Logs estruturados: ts, level, plugin, request_id, campos; SSE JSON; filtro | M | DL-05 | GEN-H | UI/CLI filtram; compatibilidade com logs antigos |
| PX-07 | Métricas por plugin: requests, erros, latência, fuel; labels no `/metrics` | M | B0.7 | GEN-H | Prometheus scrape; testes de contadores |
| PX-08 | Serviço: `restart` explícito e status com motivo de falha | M | PX-07 | GEN-H | 409/estado correto; UI mostra |
| PX-09 | Windows parity: paths, `where`, `cmd /C`, temp, portas; CI Windows | M | B0.2 | PIPE + GEN-M | CI Windows verde; docs/WINDOWS atualizado |

---

### Fase 6 — Release e operação (RL)

| ID | Entregável | Tam. | Dep. | Agente | Aceite |
| --- | --- | --- | --- | --- | --- |
| RL-01 | `bundle --profile dev\|staging\|prod` com overlays de config; CSP/host por perfil | M | RF-05 | GEN-H | Perfis geram artefatos distintos e válidos |
| RL-02 | Updater Tauri + `latest.json` + canais; chave de update; docs | L | RL-01 | PIPE + GEN-H | Verificação com servidor estático local |
| RL-03 | Code signing Windows/macOS e notarização; secrets no CI; skip seguro sem cert | L | B0.2 | PIPE | Build sem cert documenta; com cert assina |
| RL-04 | SBOM (cargo/npm/Go) + `--provenance` + `SOURCE_DATE_EPOCH`; builds reproduzíveis onde possível | M | RL-01 | PIPE | SBOM no bundle; dois builds com epoch fixo comparáveis |
| RL-05 | OpenTelemetry opcional (feature `otel`), OTLP endpoint, sem overhead desligado | M | PX-07 | GEN-H | Feature compila; spans de call |
| RL-06 | ADR-0005: streaming/progresso em chamadas longas (jobs, SSE, cancel, WIT `progress`); protótipo atrás de flag | L | CT-08 | ARCH + GEN-H | ADR aprovado; protótipo com 1 plugin |
| RL-07 | Depreciação/ABI: política semver, `min_host`, migration guide, changelog | M | CT-08 | ARCH + GEN-M | Docs publicadas; `validate` avisa |

---

### Fase 7 — Gate 1.0

| ID | Entregável | Agente | Aceite |
| --- | --- | --- | --- |
| G0 | Architecture review dos ADRs e do código | REV | Nenhum blocker aberto |
| G1 | Security review: bridge, CSP, updater, registry, capabilities | SEC | Relatório sem crítico/alto |
| G2 | Matriz completa Linux + Windows: fmt/clippy/test/smoke/Playwright/bundle | PIPE | Tudo verde |
| G3 | Performance: startup cold/warm, p50/p95 de call, tamanho do bundle | QA | Orçamento definido e atendido |
| G4 | Docs: GET_STARTED, API, CLI, UI_PLUGINS, PLUGIN_AUTHORING, WINDOWS, REGISTRY, ROADMAP, llms.txt | GEN-M | Links válidos; exemplos executam |
| G5 | Release notes + migration guide + tag `v1.0.0` (push só com aprovação) | GEN-M | Tag local criada; relatório final |

Orçamentos de performance sugeridos:

- host startup com cache quente: ≤ 300 ms;
- call de plugin pequeno: p95 ≤ 20 ms;
- call de componente Go/TS com cache quente: p95 ≤ 100 ms;
- `owasidea build` incremental do Shipyard: ≤ 60 s;
- bundle deb: ≤ 5 min em runner Linux.

---

## 7. Grafo de dependências e caminho crítico

```
B0.1 ─┬─ B0.2 ─┬─ B0.4
      ├─ B0.3  │
      ├─ B0.5 ─┴─ B0.6
      └─ B0.7

B0.5 → RF-01 → RF-02 → RF-05
             ├─ RF-03 → RF-04 → RF-09
             ├─ RF-06 → DL-01/DL-02
             └─ RF-07 → RF-08 → DL-06/DL-07

CT-01 → CT-02 → CT-03
             ├─ CT-04
             └─ CT-05 → CT-06 → CT-07
CT-01 → CT-08 → PX-05/RL-07

AX-01 → AX-02/AX-03 → AX-04
AX-05 → AX-06/AX-08

PX-02 → PX-03
PX-04 independente (após B0.1)
PX-06/PX-07 → RL-05

RL-01 → RL-02/RL-03/RL-04
```

Caminho crítico:
`B0.1 → B0.5 → RF-01 → RF-03 → RF-04 → DL-01 → CT-01 → CT-02 → CT-03 →
CT-06 → AX-02 → G2 → G5`.

Itens fora do caminho crítico (AX-03, PX-04, RL-04, docs) podem rodar em
paralelo quando houver worktree livre.

---

## 8. Matriz de verificação

| Área | Comando | Quando |
| --- | --- | --- |
| Rust | `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` | todo item |
| CLI | `cargo test -p owasidea-cli` + `assert_cmd` | DL/AX/PX |
| Host | `cargo test -p owasidea-host` | RF/PX/CT-08 |
| Exemplo | `cd examples/shipyard && owasidea build && owasidea smoke` | todo item que toca host/CLI |
| UI | `owasidea test --no-build --no-smoke` (Playwright) | UI/bridge |
| Config | `owasidea config render --check` | RF |
| Bundle | `owasidea bundle --no-build --bundles deb` | RF/RL |
| AppImage | `owasidea bundle --bundles appimage --isolate-host-libs` | RL/CI Linux |
| Windows | workflow `windows-latest` | B0.2/PX-09/RL |
| Segurança | `cargo audit`, assinatura, `verify dist/` | PX/RL/G1 |
| Performance | benchmark script + budgets | G3 |

---

## 9. Auditoria v0–v1.0 (Fase 0)

Executar e registrar em `docs/plans/AUDIT_V1.md`:

- [ ] `max_fuel`, `timeout_ms`, `max_memory_mb`, `max_instances` realmente
      aplicados; teste que estoura cada limite.
- [ ] KV SQLite persiste entre reinícios; preopen `reports/` grava.
- [ ] Reload relê manifest e rejeita serviço rodando (409).
- [ ] HTTP outbound allowlist bloqueia host não declarado.
- [ ] SSE entrega novas linhas e não duplica histórico.
- [ ] `/call` valida aridade/tipo e preserva `result`/`results`.
- [ ] Composição build-time gera manifest sem `imports.plugins`.
- [ ] Serviço persiste instância entre start/status/stop.
- [ ] C# e TS carregam e executam no host.
- [ ] Tauri abre, carrega `ui/dist`, chama host e empacota deb/rpm/AppImage.
- [ ] UI plugins verificam assets e rodam em iframe sandbox.
- [ ] Hot reload de library funciona; serviço exige stop.
- [ ] `dev` reserva portas e watcher ignora `node_modules`/`target`.
- [ ] Templates criam projetos que buildam (Rust/Go/TS).
- [ ] `inspect`, `validate`, `doctor` retornam JSON estável.
- [ ] Registry instala artefato assinado e rejeita checksum inválido.
- [ ] Capabilities negam import não declarado.
- [ ] `/metrics` expõe contadores.
- [ ] `docs/SECURITY.md` reflete o que é e não é isolado.

Gaps viram `FIX-01`, `FIX-02`, ... com o mesmo formato dos itens RF/DL.

---

## 10. Riscos e mitigação

| Risco | Impacto | Mitigação |
| --- | --- | --- |
| Beam Free rate limit/indisponibilidade | atraso | máx. 2 concorrentes; backoff; serializar; bloquear só se persistir |
| Conflito em `main.rs`/`lib.rs`/`main.ts` | retrabalho | worktree por trilha; módulos novos; merge frequente; ownership |
| API do Tauri para initialization script mudar | RF-04 falha | ADR valida API na versão instalada; fallback com eval + retry |
| CI Windows caro/lento | B0.2/PX-09 | cache de cargo/npm; rodar matriz completa só em PR de release |
| Toolchains C#/C/C++ frágeis | AX-03 | marcar como opcional no CI; docs com versões pinadas |
| Certificados de assinatura ausentes | RL-03 | skip seguro + documentação; não bloquear o restante |
| Bridge ampliar superfície de ataque | CT-05/06 | ADR + SEC; allowlist por função; validação de args; timeout; sem CORS `null` |
| Updater quebrar app instalado | RL-02 | testar com servidor local; canal beta; rollback documentado |
| Escopo crescer | atraso | este plano é o contrato; novo item exige ADR e priorização |
| Exemplo untracked | worktrees/CI quebrados | B0.1 antes de tudo |
| AppImage contaminado por VMware | release falha | `--isolate-host-libs`; CI limpo; docs/CLEAN_BUILD |

---

## 11. Tracker

Criar `docs/plans/ROADMAP_STATUS.md` com:

```markdown
| ID | Status | Worktree | Commit | Gates | Notas |
| --- | --- | --- | --- | --- | --- |
| B0.1 | todo | — | — | — | — |
| ... | ... | ... | ... | ... | ... |
```

Status permitidos: `todo`, `doing`, `review`, `done`, `blocked`.
Atualizar no mesmo commit do item (ou commit `docs(status): ...` imediato).

Template de blocker (`docs/plans/BLOCKERS.md`):

```markdown
## <ID> — <título>
- Status: blocked
- Tentativas: 2
- Comando: ...
- Erro:
- Impacto:
- Precisa de:
- Itens que podem continuar:
```

---

## 12. Prompts dos subagentes

### 12.1 Contexto comum (colar em todos)

```text
Repo: /home/pablitux/Projetos/wasm-app-framework
Worktree: <caminho>
Leia antes: AGENTS.md, docs/plans/ROADMAP_IMPLEMENTATION_PLAN.md,
docs/plans/ROADMAP_STATUS.md, e os arquivos listados no item.

Invariantes:
- Manifest é gerado de WIT; nunca editar à mão.
- Sem Extism, sem runtime JS no host, sem Docker obrigatório, sem fork do linuxdeploy.
- Capabilities least-privilege; nada de wildcard.
- Código + teste + doc no mesmo commit.
- Não tocar arquivos fora do escopo do item.
- Não commitar artefatos gerados, chaves ou dados.
- Rodar os gates da seção 4.6 antes de reportar.
- Se o item exigir decisão de arquitetura, pare e devolva a pergunta ao orquestrador.

Responda em ≤12 linhas: status, arquivos, comandos+resultados, riscos,
follow-ups. Não cole diff completo.
```

### 12.2 Implementação

```text
Item: <ID> — <título>
Objetivo: <...>
Dependências satisfeitas: <...>
Escopo permitido: <arquivos/dirs>
Passos:
1. ...
2. ...
Aceite:
- <comando/teste>
- <comportamento>
Docs a atualizar: <...>
Modelo: opencode-go/beam-free#high (ou #medium)
```

### 12.3 ADR / design

```text
Item: <ID> — <título>
Produza um ADR em docs/adr/<NNNN>-<slug>.md com:
contexto, opções, decisão, consequências, ameaças, migração e critérios de teste.
Não implemente código neste passo.
Valide a API real da versão instalada (Tauri/Wasmtime) antes de recomendar.
Modelo: opencode-go/beam-free#max
```

### 12.4 Review

```text
Revise o diff do item <ID> na worktree <...>.
Procure: quebra de invariantes, regressão de segurança, corrida, erro não
tratado, teste ausente, doc desatualizada, mudança de ABI sem migração.
Rode os gates se necessário.
Saída: blocker / major / minor / nit, com arquivo:linha e correção sugerida.
Modelo: opencode-go/beam-free#max
```

### 12.5 QA

```text
Item: <ID> — <título>
Atualize/valide a estratégia de teste: casos felizes, erros, limites,
concorrência, Windows, regressão.
Adicione testes que falham antes da correção e passam depois.
Não altere código de produção sem alinhamento com o implementador.
Modelo: opencode-go/beam-free#high
```

### 12.6 Segurança

```text
Item: <ID> — <título>
Ameaças: plugin não confiável, UI plugin sandbox, bridge, CSP, updater,
registry, assinatura, path traversal, SSRF, DoS.
Valide allowlists, limites, timeouts, origem, tamanho de payload e erros.
Saída: risco + severidade + mitigação + teste.
Modelo: opencode-go/beam-free#high
```

---

## 13. Primeiro despacho (quando a execução começar)

1. `GEN-M` → **B0.1** (baseline git + exemplo versionado). Sem isso, nada de
   worktree.
2. `PIPE` → **B0.2** (CI Linux/Windows).
3. `GEN-M` → **B0.3** (tracker).
4. `QA` + `GEN-M` → **B0.4** (auditoria).
5. `ARCH` → **RF-03** (ADR de runtime config) em paralelo com **B0.5**.
6. `GEN-H` → **RF-01/RF-02** após B0.5.
7. Seguir o caminho crítico da seção 7.

O orquestrador deve atualizar o tracker após cada merge e nunca iniciar um item
cujo pré-requisito não está `done`.

---

## 14. Relatório final de 1.0

Ao concluir a Fase 7, produzir `docs/plans/V1_RELEASE_REPORT.md` com:

- itens done/blocked e commits;
- matriz de testes Linux/Windows;
- benchmarks vs orçamento;
- SBOM e hashes dos artefatos;
- riscos residuais e limitações conhecidas;
- instruções de migração de 0.x para 1.0;
- tag `v1.0.0` local (push somente com aprovação).
