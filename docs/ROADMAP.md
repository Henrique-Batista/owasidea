# Roadmap

## v0 — Vertical slice (done)

- Host Rust + Wasmtime (WASI 0.2/0.3), manifests gerados do WIT
- Capability enforcement (host interfaces)
- Plugins Rust (`notes`) e Go (`chat-go`)
- CLI `owasidea` (init, doctor, plugin new, build, dev, run, inspect)
- API HTTP + UI (TypeScript + Vite)
- Documentação base

## v0.1 — Hardening do runtime

- [x] Aplicar `max_fuel` no Store (`consume_fuel`)
- [x] Aplicar `timeout_ms` como timeout de requisição (tokio)
- [x] Limites de memória por instância (StoreLimits)
- [x] Persistir KV host (SQLite/redb) em vez de memória
- [x] Re-ler manifest no reload (feito)
- [x] Atualizar estado em memória após reload
- [x] Hot-reload preserva estado gerenciado pelo host (SQLite KV + preopens); memória guest de library é request-scoped por desenho
- [x] Outbound HTTP allowlist via `WasiHttpHooks::send_request`
- [x] Streaming de logs via SSE

## v0.2 — Genericidade e composição

- [x] Dispatcher genérico baseado em WIT (endpoint `/call`)
- [x] Composição build-time via `wasm-tools compose` (exemplo `composed`)
- [x] Tipos de plugin: `service` formalizado (lifecycle genérico via /service/*)
- [x] Hot-reload explícito para plugins stateful (restart) com status visível
- [x] Suporte a C# e JS/TS plugins

## v0.3 — Desktop e UX

- [x] Shell Tauri 2 embutindo o host (scaffold)
- [x] UI carregando `ui/dist` e comunicação via API local (configurado)
- [x] UI genérica chamando exports via /call
- [x] Telas: overview, plugins, detalhe, serviços, dev mode (hash router)
- [x] `owasidea bundle` produzindo app Tauri
- [x] Validar bundle Tauri (deb/rpm gerados com sucesso)
- [x] Suporte a bundles UI WASM de Yew, Blazor e Uno na webview (contrato estático)
- [x] Integração `js-wasm` como plugin UI webview-only, iframe sandbox + verificação dos assets
- [x] Watch de arquivos + hot reload integrado

## v0.4 — Ferramentas e DX

- [x] `owasidea dev` orquestrando host + UI dev server, URLs dinâmicas e watcher recursivo multiplataforma
- [x] Templates de plugin para Rust/Go/TS/C/C++/C#
- [x] `owasidea inspect` com WIT formatado e permissões efetivas
- [x] Testes de integração automatizados (cargo test + Playwright smoke)
- [x] Doctor com ferramentas obrigatórias/opcionais

## v1.0 — Production pilot: polyglot desktop framework

- [x] Reference app `examples/shipyard`: Rust, Go, TypeScript, C#, C, C++,
      build-time composition, service + executable plugins, isolated Svelte and
      React UI bundles, Tauri `.deb`, smoke + Playwright tests
- [x] `owasidea bundle` resolves app-relative UI/desktop paths and packages
      only manifests + component artifacts
- [x] Verificação de integridade por checksum (sha256)
- [x] Assinatura criptográfica de plugins (Ed25519 via `owasidea keygen/sign`)
- [x] Registry HTTPS estático: search/install de versões assinadas para WASI e UI; publicação via CI
- [x] Isolamento por capabilities WASI: imports validados, filesystem/socket/HTTP deny-by-default; limites OS documentados
- [x] Execução concorrente por requisição (instância por chamada)
- [x] Observabilidade básica (métricas /metrics + tracing)
- [x] Documentação de segurança e threat model (docs/SECURITY.md)

## Decisões resolvidas

- Nome e binário: Owasidea / `owasidea`.
- UI padrão: vanilla TypeScript + Vite; Yew/Blazor/Uno suportados como bundles UI.
- Config: `wasm-app.toml`.
- Primeiro serviço genérico: `heartbeat` (Rust); Tailcat segue opcional.
- `js-wasm`: apenas plugin UI na webview; nunca runtime headless do host.
- UX de permissões: manifest gerado é fonte da verdade; UI exibe permissões efetivas, sem picker/runtime escalation.

Limite do piloto: a sandbox WASI reduz capabilities, mas não substitui isolamento
de processo/usuário do sistema operacional nem auditoria de vulnerabilidades do
runtime. Ver `docs/SECURITY.md`.

## Backlog de DX e hardening para 1.0 (proposto)

O slice funcional está pronto. O que falta para 1.0 é reduzir o custo de
configuração e o tempo entre "editei o plugin" e "vi o resultado".

### P0 — bloqueadores de experiência

| Item | Problema hoje | Proposta |
| --- | --- | --- |
| Fonte única de configuração | `[host].addr`, CSP do Tauri, Vite, Playwright e `devUrl` repetem portas e origens | `owasidea` gera `runtime-config.json` e injeta `window.__OWASIDEA__` no webview; CSP e `devUrl` derivados de `wasm-app.toml` |
| `owasidea dev --desktop` | `dev` sobe host + Vite, mas não o Tauri; o binário pode ficar velho em relação aos manifests | iniciar `cargo tauri dev` com a URL correta, logs unificados e `--open` para browser |
| `owasidea call` / `owasidea logs` | testar um export exige `curl` ou a UI; logs só no navegador | `owasidea call <plugin>.<fn> --args '<json>'` e `owasidea logs <plugin> --follow` |
| Invocação genérica na UI | o workbench é escrito à mão por plugin | gerar formulário a partir de `exports[].signature` (string, número, lista, record) com validação e snippet `fetch`/`curl` |
| Client tipado | cada frontend reescreve nomes e tipos de argumentos | `owasidea client --lang ts` gera funções e tipos a partir dos manifests/WIT |
| Bridge para UI plugin | iframe isolado não tem como chamar o host (CORS `Origin: null`) | `bridge = ["changelog-ts.render"]` no config; painel cria `MessageChannel`, valida allowlist/aridade e faz proxy |
| Scaffolds reais | `plugin new --lang svelte` gera um HTML placeholder | gerar projeto Vite/Svelte/React/Vue com `base: './'`, demo e `ui_build` já configurado |
| Erros acionáveis | erros crus de Wasmtime/CSP/assinatura | códigos `OWASIDEA_E_*`, dica de correção, link para doc e `owasidea explain <code>` |

### P1 — produtividade

- Diff de permissões entre builds; falhar por padrão quando uma capability for ampliada.
- Hot reload de UI plugin: rebuild + refresh do iframe quando `dist/` ou `ui-plugins.json` mudar.
- `owasidea test --generate`: smoke a partir das assinaturas WIT, com valores default e overrides.
- `owasidea test --filter <plugin> --watch`.
- `owasidea doctor --fix` com versões pinadas e `tools.lock`.
- JSON Schema de `wasm-app.toml` e `owasidea schema` para autocomplete no editor.
- `owasidea verify dist/` para checar assinaturas/checksums no CI.
- Registry com lockfile (`plugins.lock`), `registry update` e `outdated`.
- Selo de build no app: `build_id` no host, UI e manifests; avisar "UI bundle anterior ao host".

### P2 — operação e release

- Métricas por plugin: latência, erros, fuel, memória de pico; export OpenTelemetry.
- Logs estruturados com nível e request id; filtro `--level`.
- Assinatura de código Windows/macOS, updater Tauri e canais de release.
- SBOM, builds reproduzíveis e provenance.
- Política de depreciação do ABI (`schema_version`, `host_abi`, `min_host`).
- Bridges opcionais para streaming/progresso em chamadas longas.

### Anti-metas

Não adicionar QuickJS/Node ao host, não exigir Docker, não forkar linuxdeploy e
não aceitar manifest editado à mão como fonte de verdade.
