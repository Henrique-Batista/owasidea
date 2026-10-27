# {{name}}

Language: {{lang}}
Type: {{kind}}

## Contract

Rust plugins generate the WIT contract from `#[owasidea::...]` markers into
`.owasidea/wit`; no WIT file is needed. Go, TypeScript, C#, C and C++ use
`wit/<name>.wit`. `owasidea build` regenerates the contract before compiling and
fails if markers and a hand-written WIT disagree.

## Build

```bash
owasidea build
```

## Call

```bash
owasidea call {{name}} greet --arg name=world
```

## Permissions

Inspect effective permissions:

```bash
owasidea inspect {{name}}
```

## Test

```bash
owasidea smoke
```
