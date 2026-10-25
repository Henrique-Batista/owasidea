#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export OWASIDEA_MANIFESTS="$ROOT/manifests"
HOST_ADDR="${HOST_ADDR:-127.0.0.1:8787}"
# start host
cargo run -p owasidea-host -- "$ROOT/manifests" --addr "$HOST_ADDR" >/tmp/opencode/uihost.log 2>&1 &
HOSTPID=$!
sleep 8
# run playwright
npx playwright test --config "$ROOT/playwright.config.ts" || true
kill $HOSTPID 2>/dev/null || true
wait $HOSTPID 2>/dev/null || true