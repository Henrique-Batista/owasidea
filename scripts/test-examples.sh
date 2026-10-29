#!/usr/bin/env bash
# Runs the minimal example apps end to end: `owasidea build`, `owasidea smoke`
# and `owasidea test` (Playwright UI tests) for each app under examples/.
#
# Usage:
#   cargo build -p owasidea-cli
#   scripts/test-examples.sh [hello notes ticker ...]
#
# With no arguments every example in EXAMPLES below runs. Each app uses its own
# host and UI port (see its wasm-app.toml), so the examples can run one after
# another on the same machine. A failing example stops the script.
#
# Environment:
#   OWASIDEA_BIN   CLI binary (default: <cargo target dir>/debug/owasidea)
#   OWASIDEA_EXAMPLES_SKIP_UI  "1" runs build + smoke only (no Playwright)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLES=(hello notes ticker interop)

if [ -n "${OWASIDEA_BIN:-}" ]; then
  BIN="$OWASIDEA_BIN"
else
  # cargo knows the real target directory (CARGO_TARGET_DIR, .cargo/config).
  target_dir="$(cd "$ROOT" && cargo metadata --no-deps --format-version 1 2>/dev/null \
    | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
  BIN="${target_dir:-$ROOT/target}/debug/owasidea"
fi

if [ ! -x "$BIN" ]; then
  echo "error: owasidea binary not found at $BIN; run: cargo build -p owasidea-cli" >&2
  exit 2
fi

if [ $# -gt 0 ]; then
  EXAMPLES=("$@")
fi

failed=()
for name in "${EXAMPLES[@]}"; do
  dir="$ROOT/examples/$name"
  if [ ! -f "$dir/wasm-app.toml" ]; then
    echo "error: no example named '$name' ($dir/wasm-app.toml missing)" >&2
    exit 2
  fi
  # Keep each example's Rust target separate. Examples and the repository app
  # can share crate names (both have `notes`), and a shared CARGO_TARGET_DIR
  # would let one build overwrite the other's .wasm.
  export CARGO_TARGET_DIR="$dir/.owasidea/cargo-target"
  echo "==> $name: build"
  "$BIN" --config "$dir/wasm-app.toml" build
  echo "==> $name: smoke"
  "$BIN" --config "$dir/wasm-app.toml" smoke --no-build
  if [ "${OWASIDEA_EXAMPLES_SKIP_UI:-0}" != "1" ]; then
    echo "==> $name: UI tests"
    "$BIN" --config "$dir/wasm-app.toml" test --no-build --no-smoke
  fi
  echo "==> $name: ok"
done

echo "examples passed: ${EXAMPLES[*]}"
