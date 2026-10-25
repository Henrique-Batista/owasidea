# Source Generators

Owasidea suporta geração automática de WIT a partir de anotações no código-fonte.

## Rust

Use atributos do crate `owasidea-macros`:

```rust
use owasidea_macros::*;

#[owasidea::host_log]
#[owasidea::host_kv]
#[owasidea::world("tester")]
struct Tester;

#[owasidea::export]
fn greet(name: String) -> String { ... }

#[owasidea::export]
fn compute(a: i32, b: i32) -> i32 { ... }
```

Execute `owasidea build` (ou `owasidea wit generate`) para gerar `wit/tester.wit`.

## Go / C# / TypeScript / C / C++

Geradores similares estão em desenvolvimento. Consulte `owasidea doctor` e `docs/PLUGIN_AUTHORING.md`.

## Validação

`owasidea validate` confere checksum e capabilities do manifest gerado.