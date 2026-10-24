#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Building Rust library plugin (notes)"
pushd plugins/notes >/dev/null
cargo component build --release
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')"
popd >/dev/null
cp "$TARGET_DIR/wasm32-wasip1/release/notes.wasm" \
   plugins/notes/notes.component.wasm

echo "==> Generating notes manifest"
python3 scripts/gen-manifest.py \
  --component plugins/notes/notes.component.wasm \
  --name notes --version 0.1.0 --language rust --type library \
  --out manifests/notes.manifest.json \
  --fs ./data/notes:read-write \
  --kv notes:read-write \
  --persistent --storage wasi-filesystem \
  --description "Rust library plugin (host KV cache + host log)"

echo "==> Building Go executable plugin (chat-go)"
pushd plugins/chat-go >/dev/null
go tool componentize-go build
popd >/dev/null
cp plugins/chat-go/main.wasm plugins/chat-go/chat-go.component.wasm

echo "==> Generating chat-go manifest"
python3 scripts/gen-manifest.py \
  --component plugins/chat-go/chat-go.component.wasm \
  --name chat-go --version 0.1.0 --language go --type executable \
  --out manifests/chat-go.manifest.json \
  --http-outbound example.com --http-outbound 127.0.0.1 \
  --description "Go executable plugin (wasi:http inbound + outbound)"

echo "==> Done"
ls -lh plugins/notes/notes.component.wasm plugins/chat-go/chat-go.component.wasm