#!/usr/bin/env bash
# FIX-03: compile the scaffold templates, not just their file registry.
#
# Creates a temporary app with `owasidea init --preset minimal`, scaffolds
# Rust/Go/TypeScript library plugins with `owasidea plugin new`, runs
# `owasidea build`, and validates every generated manifest.
#
# Usage:
#   cargo build -p owasidea-cli
#   scripts/test-templates.sh [--langs rust,go,typescript] [--strict] [--keep]
#
# Environment:
#   OWASIDEA_BIN              CLI binary (default: $CARGO_TARGET_DIR/debug/owasidea,
#                             otherwise target/debug/owasidea)
#   OWASIDEA_TEMPLATE_LANGS   comma-separated languages (default: rust,go,typescript)
#   OWASIDEA_TEMPLATE_STRICT  "1" fails when a toolchain is missing instead of skipping
#   OWASIDEA_TEMPLATE_KEEP    "1" keeps the temporary app and prints its path
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
langs="${OWASIDEA_TEMPLATE_LANGS:-rust,go,typescript}"
strict="${OWASIDEA_TEMPLATE_STRICT:-0}"
keep="${OWASIDEA_TEMPLATE_KEEP:-0}"

usage() {
  sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --langs)
      [ $# -ge 2 ] || { echo "error: --langs needs a value" >&2; exit 2; }
      langs="$2"
      shift 2
      ;;
    --strict)
      strict=1
      shift
      ;;
    --keep)
      keep=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$strict" in 1 | true | yes) strict=1 ;; *) strict=0 ;; esac
case "$keep" in 1 | true | yes) keep=1 ;; *) keep=0 ;; esac

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  case "$CARGO_TARGET_DIR" in
    /*) ;;
    *) export CARGO_TARGET_DIR="$ROOT/$CARGO_TARGET_DIR" ;;
  esac
fi

if [ -n "${OWASIDEA_BIN:-}" ]; then
  bin="$OWASIDEA_BIN"
  explicit_bin=1
else
  explicit_bin=0
  bin="${CARGO_TARGET_DIR:-$ROOT/target}/debug/owasidea"
fi

if [ ! -x "$bin" ]; then
  if [ "$explicit_bin" = 1 ]; then
    echo "error: OWASIDEA_BIN=$bin is not executable" >&2
    exit 1
  fi
  echo "==> building owasidea ($bin not found)"
  (cd "$ROOT" && cargo build -p owasidea-cli)
fi
if [ ! -x "$bin" ]; then
  echo "error: owasidea binary not found at $bin" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required to validate generated manifests" >&2
  exit 1
fi

have() { command -v "$1" >/dev/null 2>&1; }

missing_toolchain() {
  case "$1" in
    rust)
      if ! cargo component --version >/dev/null 2>&1; then
        echo "cargo-component"
        return
      fi
      ;;
    go)
      if ! have go; then
        echo "go"
        return
      fi
      ;;
    typescript)
      if ! have node || ! have npm; then
        echo "node/npm"
        return
      fi
      # jco comes from the scaffold's npm devDependencies; a global jco is not
      # required. The first build runs `npm install` inside the plugin.
      ;;
    *)
      echo "unsupported-language"
      return
      ;;
  esac
}

selected=()
for lang in ${langs//,/ }; do
  [ -n "$lang" ] || continue
  case "$lang" in
    rust | go | typescript) ;;
    *)
      echo "error: unsupported template language: $lang (expected rust, go, or typescript)" >&2
      exit 2
      ;;
  esac
  missing="$(missing_toolchain "$lang")"
  if [ -n "$missing" ]; then
    if [ "$strict" = 1 ]; then
      echo "error: cannot compile the $lang template: missing $missing" >&2
      exit 1
    fi
    echo "skip: $lang template (missing $missing; set OWASIDEA_TEMPLATE_STRICT=1 to fail)"
    continue
  fi
  selected+=("$lang")
done

if [ ${#selected[@]} -eq 0 ]; then
  echo "error: no template toolchains available (requested: $langs)" >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/owasidea-templates.XXXXXX")"
cleanup() {
  if [ "$keep" = 1 ]; then
    echo "kept template app: $work" >&2
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

app="$work/app"
expected=""
echo "==> scaffolding app ($app)"
"$bin" init "$app" --yes --preset minimal >/dev/null

for lang in "${selected[@]}"; do
  case "$lang" in
    rust) name="template-rust" ;;
    go) name="template-go" ;;
    typescript) name="template-ts" ;;
  esac
  echo "==> plugin new $name (--lang $lang)"
  (cd "$app" && "$bin" plugin new "$name" --lang "$lang" >/dev/null)
  expected="${expected:+$expected,}$name"
done

echo "==> owasidea build (${selected[*]})"
if ! (cd "$app" && "$bin" --config wasm-app.toml --manifest-dir manifests build); then
  echo "error: owasidea build failed for scaffold templates (${selected[*]})" >&2
  exit 1
fi

validate_json="$work/validate.json"
echo "==> owasidea validate --json"
if ! (cd "$app" && "$bin" --config wasm-app.toml --manifest-dir manifests validate --json >"$validate_json"); then
  cat "$validate_json" >&2 || true
  echo "error: generated manifests failed validation (${selected[*]})" >&2
  exit 1
fi

python3 - "$app" "$validate_json" "$expected" <<'PY'
import json
import os
import sys

app, validate_path, expected = sys.argv[1], sys.argv[2], sys.argv[3].split(",")
with open(validate_path, encoding="utf-8") as handle:
    report = json.load(handle)
if report.get("ok") is not True:
    raise SystemExit(f"validation not ok: {report}")
reported = {plugin["name"]: plugin for plugin in report.get("plugins", [])}
for name in expected:
    if name not in reported:
        raise SystemExit(f"manifest for {name} was not validated: {sorted(reported)}")
    if reported[name].get("ok") is not True:
        raise SystemExit(f"{name} manifest invalid: {reported[name]}")
    manifest_path = os.path.join(app, "manifests", f"{name}.manifest.json")
    with open(manifest_path, encoding="utf-8") as handle:
        manifest = json.load(handle)
    if manifest.get("name") != name:
        raise SystemExit(f"{manifest_path}: name is {manifest.get('name')!r}, want {name!r}")
    if manifest.get("runtime") != "wasi-component":
        raise SystemExit(f"{manifest_path}: runtime is {manifest.get('runtime')!r}")
    exports = {item.get("name") for item in manifest.get("exports", [])}
    if "greet" not in exports:
        raise SystemExit(f"{manifest_path}: missing greet export: {sorted(exports)}")
    component = os.path.normpath(
        os.path.join(os.path.dirname(manifest_path), manifest.get("component", ""))
    )
    if not os.path.isfile(component) or os.path.getsize(component) == 0:
        raise SystemExit(f"{manifest_path}: component missing or empty: {component}")
print(f"ok   templates build and validate: {', '.join(expected)}")
PY

echo "template checks passed"
