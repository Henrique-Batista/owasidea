#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export OWASIDEA_MANIFESTS="${OWASIDEA_MANIFESTS:-$ROOT/manifests}"
export HOST_ADDR="${HOST_ADDR:-127.0.0.1:8787}"
cargo run -p owasidea-host -- "$OWASIDEA_MANIFESTS" --addr "$HOST_ADDR"