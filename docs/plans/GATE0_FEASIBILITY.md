# Gate 0 — runtime feasibility spike

Status: **PASS** (2026-10-28, Linux, Wasmtime 50, componentize-go 0.5.1)

## Question

Can a Rust component call a Go component through an async host import, in a
separate store, without holding a lock that prevents active-plugin re-entry
detection?

## Spike

Disposable files under `/tmp/opencode` (not committed):

- `gate:provider@0.1.0` world exported by a Go component built with
  `componentize-go`.
- Rust consumer importing `gate:provider/api@0.1.0`, exporting
  `run(value: string) -> string`.
- Host using `LinkerInstance::func_wrap_async`; the callback enters the
  provider in a new `Store`, calls the provider's typed `echo`, and returns the
  result.
- Call-stack vector checked before dispatch; depth capped at 16.

## Results

- Typed Rust→Go call returned `Go:typed`.
- A slow provider suspended the caller; the outer timeout cancelled it.
- The caller store was dropped after cancellation (re-entry through the same
  component instance is disallowed by Wasmtime: `cannot enter component
  instance`). Host calls create one store per request, so this is the production
  behavior.
- Trap and active-plugin re-entry propagated as errors without hanging.
- No plugin-map lock was held across guest execution.

## Consequence for the implementation

`build_linker` snapshots the plugin map before registering dependency imports;
each nested call instantiates the provider in its own store. The stable error
codes are `OWASIDEA_E_DEPENDENCY_REENTRANT_CALL` and
`OWASIDEA_E_DEPENDENCY_DEPTH_EXCEEDED`.
