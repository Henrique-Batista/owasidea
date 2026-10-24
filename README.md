# Owasidea — WASM App Framework

Owasidea é um framework para construir aplicativos desktop que empacotam
componentes WebAssembly independentes (escritos em Rust, Go e outras linguagens)
em uma única aplicação. Cada componente é descrito por um contrato WIT e carregado
por um host Rust baseado em Wasmtime.

A ideia é simples: você escreve plugins pequenos e reutilizáveis, o framework
cuida de descobrir, validar, carregar, chamar e isolar cada um deles.

## O que já funciona (v0)

- Host Rust com Wasmtime (Component Model, WASI 0.2 e 0.3)
- Manifests gerados automaticamente a partir do WIT real do componente
- Enforcement de capabilities: um componente só acessa o que declarou
- Plugins de exemplo em Rust (`notes`) e Go (`chat-go`)
- CLI `owasidea` para criar, compilar, inspecionar e rodar apps
- API HTTP para a UI e automação
- Frontend web (TypeScript + Vite) que consome a API do host

## Instalar a CLI

```bash
cargo install --path crates/cli
owasidea --help
```

## Como começar

### 1. Pré-requisitos

- Rust + `cargo-component` + `wasm-tools`
- Go 1.27+ (para plugins Go)
- Node 20+ (para a UI)
- `wasmtime` (CLI, para plugins executáveis)

```bash
rustup target add wasm32-wasip1 wasm32-wasip2
cargo install cargo-component wasm-tools
```

### 2. Compilar

```bash
./scripts/build-all.sh
```

Isso compila os plugins, copia os `.wasm` e regenera os manifests a partir do WIT.

### 3. Rodar o host

```bash
./scripts/run-host.sh
# http://127.0.0.1:8787
```

### 4. Rodar a UI

```bash
cd ui
npm install
npm run dev
# http://localhost:5173
```

### 5. Usar a CLI

```bash
cargo run -p owasidea-cli -- doctor
cargo run -p owasidea-cli -- build
cargo run -p owasidea-cli -- inspect notes --json
```

## Comandos úteis

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Documentação

- `docs/USAGE.md` — instalação e uso passo a passo
- `docs/CLI.md` — referência da CLI
- `docs/API.md` — API HTTP
- `docs/ARCHITECTURE.md` — arquitetura
- `docs/PLUGIN_AUTHORING.md` — como escrever plugins
- `docs/ROADMAP.md` — roadmap de funcionalidades
- `CONTRIBUTING.md` — como contribuir

## Estrutura do projeto

```
crates/host        runtime, manifests, capability broker, HTTP API
crates/cli         binário owasidea (clap)
crates/desktop     shell Tauri (planejado)
ui                 frontend (TypeScript + Vite)
plugins            plugins de exemplo
manifests          manifests gerados (não editar à mão)
scripts            build e execução
docs               documentação
```

## Licença

MIT