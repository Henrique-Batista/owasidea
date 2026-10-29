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
- [x] Composição build-time via `wasm-tools compose` (exemplo `examples/shipyard`)
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
- [x] ~~Assinatura criptográfica de plugins~~ (removida em 1.1; SHA-256 + code signing do instalador)
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

## Backlog de DX e hardening 1.0 — entregue

Este backlog foi entregue na 1.0. O detalhe por item (worktree, commit, gates)
está em [`plans/ROADMAP_STATUS.md`](plans/ROADMAP_STATUS.md).

### P0 — experiência

- [x] Fonte única de configuração: `runtime.json`, `window.__OWASIDEA__`, CSP e `devUrl` derivados de `wasm-app.toml`.
- [x] `owasidea dev --desktop` com Tauri, logs unificados e `--open`.
- [x] `owasidea call <plugin>.<fn>` e `owasidea logs <plugin> --follow`.
- [x] Invocação genérica na UI a partir de `exports[].params/results`, com snippet `fetch`/`curl`.
- [x] Client tipado `owasidea client --lang ts` com `createClient` e `CallError`.
- [x] Bridge opt-in para UI plugin (`bridge = ["plugin.export"]`), `MessageChannel` + nonce + allowlist.
- [x] Scaffolds reais Vite/Svelte/React/Vue e bundles Yew/Blazor/Uno com `base: './'`.
- [x] Erros acionáveis `OWASIDEA_E_*`, dica, link de doc e `owasidea explain`.

### P1 — produtividade

- [x] Diff de permissões (`permissions.lock.json`, `--fail-on-widen`).
- [x] Hot reload de UI plugin (rebuild + iframe novo quando `dist/`/`ui-plugins.json` muda).
- [x] `owasidea test --generate`, `--filter`, `--plugin` e `--watch`.
- [x] `owasidea doctor --fix` com versões pinadas e `tools.lock`.
- [x] JSON Schema de `wasm-app.toml` e `owasidea schema`.
- [x] `owasidea verify dist/` para assinaturas/checksums no CI.
- [x] Selo de build (`build_id` em host, UI e manifests) com aviso de UI anterior ao host.

### P2 — operação e release

- [x] Métricas por plugin (latência, erros, fuel) e export OpenTelemetry opt-in (`--features otel`).
- [x] Logs estruturados com nível e request id; filtro `--level`.
- [x] Assinatura de código Windows/macOS, updater Tauri e canais stable/beta.
- [x] SBOM (CycloneDX/SPDX), builds reproduzíveis e provenance.
- [x] Política de depreciação do ABI (`schema_version`, `host_abi`, `min_host`):
  [`ABI_POLICY.md`](ABI_POLICY.md) + [`MIGRATION_0_X_TO_1_0.md`](MIGRATION_0_X_TO_1_0.md);
  `owasidea validate` avisa manifestos legados.
- [ ] Bridge com streaming/progresso em chamadas longas. Hoje a API de jobs/SSE é
  experimental e host-only (`--features streaming`, ADR-0005); o bridge de UI
  continua request/response.

## Pendências conhecidas para depois da 1.0

- Jobs/SSE são experimentais e exigem `--features streaming`; não há fila nem
  persistência entre reinícios do host.
- WebView2 (Tauri Windows) não foi validado para o bridge de UI; engines sem
  suporte devem falhar fechado (`BRIDGE_DISABLED`, iframe isolado).
- Orçamentos de performance do gate G3 foram medidos em
  [`plans/PERFORMANCE.md`](plans/PERFORMANCE.md); a medição cobre um host Linux
  com cache quente e chamadas sequenciais, não Windows/macOS nem carga.
- Release notes finais e tag `v1.0.0` pertencem ao gate G5.

### Anti-metas

Não adicionar QuickJS/Node ao host, não exigir Docker, não forkar linuxdeploy e
não aceitar manifest editado à mão como fonte de verdade.
