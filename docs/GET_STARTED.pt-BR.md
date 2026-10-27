# Começando com Owasidea (pt-BR)

O Owasidea cria aplicações desktop a partir de componentes WebAssembly
independentes. Cada plugin tem sua linguagem, build, contrato WIT, manifesto e
conjunto de capabilities. O host é uma biblioteca Rust reutilizável; o shell
desktop é Tauri 2; a UI fala com o host por HTTP e nunca linka Wasmtime.

O exemplo completo e funcional é [`examples/shipyard`](../examples/shipyard):
Rust, Go, TypeScript, C#, C e C++, composição em build time, dois frameworks de
UI isolados (Svelte e React), persistência KV/filesystem, serviço stateful,
plugin HTTP executável e um `.deb` Tauri.

Guia completo em inglês: [`GET_STARTED.md`](GET_STARTED.md).
Playbook para agentes de IA: [`AI_AGENTS.md`](AI_AGENTS.md).
Windows nativo, NSIS e limites de cross-compilação: [`WINDOWS.md`](WINDOWS.md).

## 1. Pré-requisitos

```bash
rustup target add wasm32-wasip1 wasm32-wasip2
cargo install cargo-component wasm-tools
# Go 1.27.1+, Node 20+, Python 3
```

Opcionais por linguagem:

- TypeScript: `npm install` no plugin (jco é dependência local).
- C#: .NET 10 + `dotnet workload install wasi-experimental`.
- C/C++: wasi-sdk + `WASI_PREVIEW1_ADAPTER`.
- Tauri: dependências de sistema (`webkit2gtk-4.1`, `gtk3`, `libsoup3`) e
  `cargo-tauri`.

Versões, instalação por SO, `doctor --fix`/`tools.lock` e limitações estão em
[`OPTIONAL_TOOLCHAINS.md`](OPTIONAL_TOOLCHAINS.md).

Instale a CLI:

```bash
cargo install --path crates/cli
owasidea --help
```

## 2. Rodar o exemplo Shipyard

```bash
cd examples/shipyard
owasidea doctor --install-wasi-sdk   # opcional, C/C++
owasidea build                       # compila, compõe, gera e assina
owasidea smoke                       # host temporário + asserts da API
owasidea test                        # build + smoke + Playwright
owasidea dev                         # build + host + Vite + watcher
```

O host sobe em `127.0.0.1:8790`. O `owasidea dev` usa a porta fixa `[dev].ui_addr` (5180) e imprime as URLs. A interface tem
Workbench (chama todas as linguagens), Inventory (manifestos), Runtime (serviço
+ webhook), Isolated UI (Svelte/React em iframes), Logs (SSE) e Dev (reload).

Para empacotar:

```bash
owasidea bundle
```

O `owasidea bundle` faz build + assinatura + UI + Tauri e copia o artefato para
`dist/`. A chave de demonstração fica em `keys/` (ignorada pelo git); crie-a uma
vez com `owasidea keygen --out keys` (o exemplo não gera chave implicitamente).
O shell Tauri usa `OWASIDEA_REQUIRE_SIGNATURES=true` e a chave pública em
`trust/owasidea.pub`. Em produção, use uma chave de CI protegida.

## 3. Criar um app novo

```bash
owasidea init meu-app
cd meu-app
owasidea doctor

owasidea plugin new core --lang rust --type library
owasidea plugin new audit --lang go --type library
owasidea plugin new changelog --lang typescript --type library
owasidea plugin new policy --lang csharp --type library
owasidea plugin new native --lang c --type library
owasidea plugin new metrics --lang cpp --type library
owasidea plugin new watchtower --lang rust --type service
owasidea plugin new webhook --lang go --type executable
owasidea plugin new board --lang svelte --type ui
```

Os caminhos no `wasm-app.toml` são relativos ao arquivo de configuração. Isso
permite rodar de qualquer diretório:

```bash
owasidea --config examples/shipyard/wasm-app.toml build
```

## 4. Configuração e permissões

```toml
[app]
name = "meu-app"
version = "0.1.0"
identifier = "dev.example.meuapp"

[host]
addr = "127.0.0.1:8787"

[plugins.core]
path = "plugins/core"
language = "rust"
type = "library"
kv = "core:read-write"
fs = ["reports:read-write"]

[desktop]
path = "desktop"
ui = "ui"
```

Regras:

- Manifestos são gerados do WIT real. Nunca edite `manifests/*.json`.
- Permissões são deny-by-default e sem wildcards.
- `poc:host/host-kv` exige `kv` no config.
- `wasi:filesystem` exige `fs`.
- `wasi:http/outgoing-handler` exige `http_outbound` com hosts exatos.
- `wasi:sockets` exige `network_outbound` / `network_inbound`.
- Serviço persistente: `type = "service"` com `start-service` e
  `service-status`.
- Plugin HTTP: `type = "executable"` e export
  `wasi:http/incoming-handler`.

## 5. Chamar plugins

```bash
curl -sS -X POST http://127.0.0.1:8790/plugin/core-rust/call \
  -H 'content-type: application/json' \
  -d '{"function":"score","args":[["feat: add","fix: bug"],9,10]}'
```

Use o nome exato do WIT. Kebab-case continua kebab-case: `save-report`,
`next-version`, `load-report`.

## 6. UI isolada

```toml
[plugins.board]
path = "plugins/board"
language = "svelte"
type = "ui"
runtime = "js-wasm"
ui_entry = "index.html"
ui_build = ["npm", "run", "build"]
```

O `owasidea build` copia `dist/` para `ui/public/plugins/board/`, calcula SHA-256
de cada arquivo e gera um manifesto `webview-v1`. O dashboard verifica os hashes
antes de criar o iframe com `sandbox="allow-scripts allow-forms"`.

## 7. Composição entre linguagens

```toml
[plugins.pipeline-go]
path = "plugins/pipeline-go"
language = "go"
type = "library"
compose_with = ["core-rust"]
```

O cliente Go importa `shipyard:core/readiness` no WIT; o build compila
`core-rust`, compila o cliente e roda `wasm-tools compose`. O manifesto final não
tem imports de plugins.

## 8. Publicar com Tauri

O shell Tauri deve:

1. Resolver `$RESOURCE/manifests` no app instalado.
2. Definir `OWASIDEA_DATA_DIR` para o diretório de dados da plataforma.
3. Rodar `owasidea_host::run(manifests, addr)` em background.
4. Deixar o `owasidea bundle` gerar `bundle.resources` (manifestos, componentes,
   chave de confiança e `runtime.json`) a partir de `wasm-app.toml`.

Depois:

```bash
owasidea build
owasidea bundle
```

O bundle falha se o Tauri config ou a UI não existirem. Veja
`examples/shipyard/desktop/tauri.conf.json`.

## 9. Verificação

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
owasidea --config examples/shipyard/wasm-app.toml validate
owasidea --config examples/shipyard/wasm-app.toml smoke
owasidea --config examples/shipyard/wasm-app.toml test
```

## 10. Problemas comuns

| Sintoma | Correção |
|---|---|
| `go: no such tool "componentize-go"` | manter `tool` + `require` no `go.mod` |
| build TS falha | `npm install` no plugin |
| C/C++ falha | exportar `WASI_SDK_PATH` e `WASI_PREVIEW1_ADAPTER` |
| UI offline | conferir `[host].addr`, `OWASIDEA_HOST` e CSP `connect-src` |
| CORS | usar `owasidea dev` ou definir `OWASIDEA_ALLOWED_ORIGINS` |
| reload 409 | parar o serviço stateful antes |
| hash de UI inválido | rebuildar plugins UI e o app Tauri |
| bundle sem plugins | incluir os `.wasm` em `bundle.resources` |
