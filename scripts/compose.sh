#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Compose the notes-client component with the notes component.
cp plugins/notes/notes.component.wasm /tmp/notes.wasm 2>/dev/null || true
wasm-tools compose plugins/notes-client/notes-client.component.wasm \
  -d /tmp/notes.wasm \
  -o plugins/composed/composed.component.wasm

python3 scripts/gen-manifest.py \
  --component plugins/composed/composed.component.wasm \
  --name composed --version 0.1.0 --language rust --type library \
  --kv composed:read-write \
  --out manifests/composed.manifest.json \
  --description "Composed notes client (build-time composition)"

echo "composed manifest generated"
