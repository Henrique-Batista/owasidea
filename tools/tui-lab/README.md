# TUI lab

Three throwaway prototypes of the same dev dashboard, one per framework. Same
fake data, same keys, same states (including a failed build). They exist only to
compare feel and cost; they are not wired into `owasidea`.

| Prototype | Run | Runtime |
| --- | --- | --- |
| ratatui | `cd ratatui && cargo run` | Rust, single binary |
| Bubble Tea | `cd bubbletea && go run .` | Go, single binary |
| OpenTUI | `cd opentui && bun install && bun run src/index.ts` | Bun/TypeScript |

Keys: `1`–`5` tabs, `left`/`right`, `q` quit, `r` fake rebuild.

Measured on this Linux host (debug/profile defaults, not tuned):

| Prototype | Artifact | Size | Runtime dependency |
| --- | --- | --- | --- |
| ratatui | `tui-lab-ratatui` (debug) | 11 MB | none |
| Bubble Tea | `tui-lab-bubbletea` | 5.6 MB | none |
| OpenTUI | `node_modules` + native module | 53 MB + 5 MB | Bun ≥1.3.14 (or Node ≥26.4 with `--experimental-ffi`) |

Notes:

- Ratatui and Bubble Tea need a real TTY; `timeout` without `/dev/tty` fails.
- OpenTUI runs on Bun today. `bun build` fails on optional platform packages;
  run it with `bun run` (or mark `@opentui/core-*` external when bundling).
- The ratatui prototype already matches the current `owasidea` stack, so it is
  the lowest-risk baseline. Bubble Tea and OpenTUI are here to compare feel,
  not to be adopted without a port plan.

Comparison notes (fill after running):

- Startup time:
- Async host streams (SSE/logs) fit:
- Build-error visibility:
- Charts/tables:
- Windows support:
- Team familiarity:
