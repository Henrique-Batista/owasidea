#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Build the Go client component (imports poc:notes/api).
pushd plugins/composed-go >/dev/null
go tool componentize-go -d wit -w poc:composed-go/composed-go build -o main.wasm
cp main.wasm composed-go.client.wasm
popd >/dev/null

# Compose with the Rust notes component.
cp plugins/notes/notes.component.wasm /tmp/notes.wasm
wasm-tools compose plugins/composed-go/composed-go.client.wasm \
  -d /tmp/notes.wasm \
  -o plugins/composed-go/composed-go.component.wasm

python3 scripts/gen-manifest.py \
  --component plugins/composed-go/composed-go.component.wasm \
  --name composed-go --version 0.1.0 --language go --type library \
  --out manifests/composed-go.manifest.json \
  --description "Composed Go client importing Rust notes (cross-language)"

echo "cross-language composition complete"
