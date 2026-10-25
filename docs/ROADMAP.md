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
- [ ] Hot-reload de bibliotecas com snapshot de estado (opcional, non-goal v1.0)
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
- [ ] Suporte a UI em frameworks WASM (Yew, Blazor, Uno) na webview (vision)
- [ ] Integração `js-wasm` como plugin de UI (webview-only) (vision)
- [x] Watch de arquivos + hot reload integrado

## v0.4 — Ferramentas e DX

- [x] `owasidea dev` orquestrando host + UI dev server (watcher pendente)
- [x] Templates de plugin para Rust/Go/TS/C/C++/C#
- [x] `owasidea inspect` com WIT formatado e permissões efetivas
- [x] Testes de integração automatizados (cargo test + Playwright smoke)
- [x] Doctor com ferramentas obrigatórias/opcionais

## v1.0 — Achieved (POC scope): polyglot desktop framework

- [x] Verificação de integridade por checksum (sha256)
- [x] Assinatura criptográfica de plugins (Ed25519 via `owasidea keygen/sign`)
- [ ] Registry remoto / marketplace (non-goal for POC)
- [ ] Isolamento WASI completo (parcial: host interfaces + preopens + network allowlist)
- [x] Execução concorrente por requisição (instância por chamada)
- [x] Observabilidade básica (métricas /metrics + tracing)
- [x] Documentação de segurança e threat model (docs/SECURITY.md)

## Decisões em aberto

- Nome final do binário (atual: `owasidea`)
- Framework de UI padrão (atual: vanilla TS; visão: qualquer framework que compile para WASM)
- Config filename (atual: `wasm-app.toml`)
- Primeiro plugin de serviço além de tailcat
- js-wasm: suportado como plugin ou apenas webview
- UX de permissões: pickers vs texto vs ambos