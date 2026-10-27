# Source Generators

Owasidea gera WIT automaticamente a partir de anotações no código-fonte.

- **Rust, Go, TypeScript e C#**: o contrato é um artefato em
  `.owasidea/wit/` (`deps/host/host.wit` incluído). O template/toolchain aponta
  para esse diretório; nenhum WIT precisa existir no repositório. C# usa o
  package do plugin e o world `library`, com uma interface `api`.
- **C e C++**: as ferramentas ainda leem `wit/`; o gerador escreve
  `wit/<world>.wit` quando esses marcadores são usados.

`owasidea wit generate [path]` faz o mesmo sob demanda. `wit_source` em
`wasm-app.toml` escolhe `auto` (padrão), `wit` ou `attributes`; ver
[`PLUGIN_AUTHORING.md`](PLUGIN_AUTHORING.md#rust-cargo-component).

Todos os geradores produzem o mesmo formato WIT:

```wit
package poc:<world>@<version>;

world <world> {
    import poc:host/host-log@0.1.0;
    import poc:host/host-kv@0.1.0;

    export <name>: func(<params>) -> <result>;
}
```

Capabilities (`host-log`, `host-kv`) são marcadores no código-fonte e viram
`import` no WIT gerado. O host rejeita componentes que importem interfaces não
declaradas no manifest.

## Rust

Atributos do crate `owasidea` (re-exportado no template). O `wit_bindgen` lê o
artefato gerado:

```rust
wit_bindgen::generate!({ world: "tester", path: ".owasidea/wit" });

#[owasidea::host_log]
#[owasidea::host_kv]
#[owasidea::world("tester")]
struct Tester;

#[owasidea::export]
fn greet(name: String) -> String { ... }

#[owasidea::export]
fn compute(a: i32, b: i32) -> i32 { ... }
```

## Go

Comentários `// owasidea:...`:

```go
// owasidea:world tester
// owasidea:host_log
// owasidea:host_kv

// owasidea:export
func Greet(name string) string { ... }

// owasidea:export
func Compute(a int32, b int32) int32 { ... }
```

## TypeScript / JavaScript

Anotações JSDoc:

```ts
/** @owasidea-world tester */
/** @owasidea-host-log */
/** @owasidea-host-kv */
/** @owasidea-export */
export function greet(name: string): string { ... }

/** @owasidea-export */
export async function compute(a: number, b: number): Promise<number> { ... }
```

## C#

Atributos:

```csharp
[OwasideaWorld("tester")]
[OwasideaHostLog]
[OwasideaHostKv]
public static class TesterExports
{
    [OwasideaExport]
    public static string Greet(string name) { ... }

    [OwasideaExport]
    public static int Compute(int a, int b) { ... }
}
```

## C

Comentários `// owasidea:...` (funções podem usar ponteiros `char *` para strings):

```c
// owasidea:world tester
// owasidea:host_log
// owasidea:host_kv

// owasidea:export
int compute(int a, int b);

// owasidea:export
char *greet(const char *name);
```

## C++

Comentários `// owasidea:...`:

```cpp
// owasidea:world tester
// owasidea:host_log

// owasidea:export
int compute(int a, int b);

// owasidea:export
std::string greet(std::string name);
```

## Mapeamento de tipos

| Fonte | WIT |
|---|---|
| Rust `String`/`str`, Go `string`, TS `string`, C# `string`, C `char *`, C++ `std::string` | `string` |
| Rust `i8`/Go `int8`/C `signed char`, C++ `char` | `s8` |
| Rust `i16`/Go `int16`/C# `short`/C `short` | `s16` |
| Rust `i32`/Go `int`/C# `int`/C `int`/C++ `int` | `s32` |
| Rust `i64`/Go `int64`/C# `long`/C `long`/C++ `long` | `s64` |
| Rust `u8`/Go `uint8`/C# `byte`/C `unsigned char` | `u8` |
| Rust `u16`/Go `uint16`/C# `ushort`/C `unsigned short` | `u16` |
| Rust `u32`/Go `uint32`/C# `uint`/C `unsigned int` | `u32` |
| Rust `u64`/Go `uint64`/C# `ulong`/C `unsigned long` | `u64` |
| Rust `f32`/Go `float32`/C# `float`/C `float`/C++ `float` | `f32` |
| Rust `f64`/Go `float64`/C# `double`/TS `number`/C `double`/C++ `double` | `f64` |
| Rust `bool`/Go `bool`/C# `bool`/TS `boolean`/C `_Bool`/C++ `bool` | `bool` |
| Rust `Option<T>`/C++ `std::optional<T>` | `option<T>` |
| Rust `Result<T,E>`/Go `(T, error)`/C# `Task<T>` | `result<T, E>` |
| Rust `Vec<T>`/Go `[]T`/C# `List<T>`/C++ `std::vector<T>`/TS `Array<T>` | `list<T>` |
| C#/TS `Promise<T>`/Go marker `async`/Rust `async fn` | `async func` |

## Validação

`owasidea validate` confere checksum e capabilities do manifest gerado. `owasidea build`
regenera o WIT a partir do código-fonte antes de compilar; um manifesto stale é
rejeitado. Se um plugin Rust tiver marcadores **e** um `wit/` escrito à mão, o
build falha em vez de escolher silenciosamente: remova o `wit/` ou declare
`wit_source = "wit"` (e mantenha os dois em sincronia).
