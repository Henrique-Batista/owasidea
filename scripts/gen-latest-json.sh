#!/usr/bin/env bash
# Generate (or validate) latest.json for the Tauri updater from signed bundle
# artifacts. See docs/UPDATER.md.
#
# Usage:
#   scripts/gen-latest-json.sh [--artifacts DIR] [--url-base URL]
#                              [--channel stable|beta] [--version X]
#                              [--notes-file FILE] [--check] [--json]
#
# Environment:
#   OWASIDEA_BIN                 CLI binary (default: owasidea)
#   OWASIDEA_CONFIG              app config (default: ./wasm-app.toml)
#   OWASIDEA_UPDATER_BASE_URL    artifact base URL (default: endpoint parent)
#   OWASIDEA_UPDATE_CHANNEL      stable|beta (default: [updater].channel)
#
# The private signing key (TAURI_SIGNING_PRIVATE_KEY) is only used by the
# Tauri bundler to produce the .sig files; this script never reads it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="${OWASIDEA_BIN:-owasidea}"
config="${OWASIDEA_CONFIG:-$root/wasm-app.toml}"

if [[ ! -f "$config" ]]; then
  echo "gen-latest-json: config not found: $config (set OWASIDEA_CONFIG)" >&2
  exit 1
fi

if ! command -v "$bin" >/dev/null 2>&1; then
  echo "gen-latest-json: '$bin' not found (set OWASIDEA_BIN or build the CLI)" >&2
  exit 1
fi

exec "$bin" --config "$config" latest "$@"
