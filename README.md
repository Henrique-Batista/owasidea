# 🚀 Owasidea — Build Desktop Apps with Any Language

> **One framework. Many languages. Zero friction.**

Owasidea lets teams build desktop applications where **Rust, Go, C, C++, C# and
TypeScript/JavaScript components coexist** in a single app — each written in the
language and framework its developers already know. No one blocks another. No one
rewrites. Everyone ships.

```
┌─────────────────────────────────────────────────────────────┐
│  Your App (Tauri)                                           │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐            │
│  │  Rust lib   │ │  Go service │ │  C# module  │  ...       │
│  │  (notes)    │ │  (chat-go)  │ │  (tester)   │            │
│  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘            │
│         │               │               │                    │
│         └───────┬───────┴───────┬───────┘                    │
│                 │  WIT contracts │  Component Model          │
│                 └───────┬───────┘                            │
│                         │                                    │
│              Host (Rust + Wasmtime)                          │
│              HTTP API · Capability broker · Manifests       │
└─────────────────────────────────────────────────────────────┘
```

## ✨ Why Owasidea?

- **Polyglot by design** — Rust, Go, C, C++, C#, TypeScript all build to WASI
  components described by WIT.
- **Manifests auto-generated** from real WIT — never hand-written, never stale.
- **Capability enforcement** — plugins only get what they declare.
- **Compose at build time** — wire components together with `wasm-tools compose`.
- **One CLI** — `owasidea init/build/dev/run/inspect/validate/bundle`.
- **Interactive & headless** — beautiful prompts for humans, `--json` for agents.

## 🏁 Get Started in 60 Seconds

```bash
# 1. Install the CLI
cargo install --path crates/cli

# 2. Scaffold an app
owasidea init my-app && cd my-app

# 3. Add plugins in any language
owasidea plugin new notes --lang rust
owasidea plugin new chat  --lang go
owasidea plugin new ui    --lang typescript

# 4. Build everything (compiles + generates manifests)
owasidea build

# 5. Run
owasidea run        # host on an ephemeral port
cd ui && npm run dev  # http://localhost:5173
```

## 🧩 Multi-Language Example

The repository ships with plugins in **every supported language**:

| Plugin | Language | Type | What it does |
|--------|----------|------|--------------|
| `notes` | Rust | library | KV cache + greet/complete |
| `chat-go` | Go | executable | HTTP inbound/outbound |
| `example` | Rust | library | greet/compute/remember |
| `attr-go` | Go | library | source-generated WIT |
| `attr-ts` | TypeScript | library | source-generated WIT |
| `tester` (C#) | C# | library | componentize-dotnet |
| C/C++ examples | C/C++ | library | wit-bindgen + wasi-sdk |

All of them expose WIT contracts and are callable via `/plugin/{name}/call`.
Cross-language composition is validated (Go client → Rust notes).

## 🛠 Commands

```bash
owasidea doctor          # check toolchain
owasidea init <dir>      # scaffold app (interactive)
owasidea plugin new ...  # scaffold plugin (interactive)
owasidea build           # build + generate manifests
owasidea validate        # verify checksums & capabilities
owasidea run             # run host (ephemeral port)
owasidea dev             # host + UI dev server
owasidea inspect <name>  # show manifest
owasidea bundle          # build Tauri desktop app
```

## 📚 Documentation

- `docs/USAGE.md` — setup and running
- `docs/CLI.md` — CLI reference
- `docs/API.md` — HTTP API
- `docs/ARCHITECTURE.md` — architecture
- `docs/PLUGIN_AUTHORING.md` — writing plugins
- `docs/SOURCE_GENERATORS.md` — source generators
- `docs/SECURITY.md` — security model
- `docs/ROADMAP.md` — roadmap

## 📄 License

MIT