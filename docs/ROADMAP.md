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
- [ ] Limites de memória por instância (não enforced)
- [x] Persistir KV host (SQLite/redb) em vez de memória
- [ ] Re-ler manifest no reload (feito) + atualizar estado em memória
- [ ] Hot-reload de bibliotecas com snapshot de estado (opcional)
- [x] Outbound HTTP allowlist via `WasiHttpHooks::send_request`
- [x] Streaming de logs via SSE

## v0.2 — Genericidade e composição

- [ ] Dispatcher genérico baseado em WIT (chamar exports arbitrários, não só greet/complete)
- [ ] Composição build-time via `wasm-tools compose` (`imports.plugins`)
- [ ] Tipos de plugin: `service` formalizado (lifecycle start/stop/status genérico)
- [ ] Hot-reload explícito para plugins stateful (restart) com status visível
- [ ] Suporte a C# e JS/TS plugins

## v0.3 — Desktop e UX

- [ ] Shell Tauri 2 embutindo o host
- [ ] UI carregando `ui/dist` e comunicação via API local
- [ ] Telas: overview, plugins, detalhe, serviços, dev mode
- [ ] `owasidea bundle` produzindo app Tauri
- [ ] Watch de arquivos + hot reload integrado

## v0.4 — Ferramentas e DX

- [ ] `owasidea dev` orquestrando host + UI dev server + watcher
- [ ] Templates de plugin mais completos (Rust/Go)
- [ ] `owasidea inspect` com WIT formatado e permissões efetivas
- [ ] Testes de integração automatizados (cargo test + Playwright smoke)
- [ ] Doctor com versões mínimas e mensagens acionáveis

## v1.0 — Produção (não-escopo do POC)

- [ ] Assinatura/verificação de plugins
- [ ] Registry remoto / marketplace
- [ ] Isolamento WASI completo (não apenas host interfaces)
- [ ] Multi-instância concorrente com locks por plugin
- [ ] Observabilidade (métricas, tracing distribuído)
- [ ] Documentação de segurança e threat model

## Decisões em aberto

- Nome final do binário (atual: `owasidea`)
- Framework de UI padrão (atual: vanilla TS; visão: qualquer framework que compile para WASM)
- Config filename (atual: `wasm-app.toml`)
- Primeiro plugin de serviço além de tailcat
- js-wasm: suportado como plugin ou apenas webview
- UX de permissões: pickers vs texto vs ambos