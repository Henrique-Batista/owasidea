#!/usr/bin/env bash
# RL-03: exercise the code-signing planner (skip/invalid matrix) without any
# real certificate. Safe to run locally and in CI; no secret is required.
#
# Usage:
#   cargo build -p owasidea-cli
#   scripts/test-signing-plan.sh [path-to-owasidea]
set -euo pipefail

bin="${1:-${OWASIDEA_BIN:-target/debug/owasidea}}"
if [ ! -x "$bin" ]; then
  echo "error: $bin not found; run 'cargo build -p owasidea-cli' first" >&2
  exit 1
fi

# Every variable the planner inspects. Cleared before each case so the host
# environment (or a developer shell) cannot influence the result.
SIGNING_VARS=(
  WINDOWS_CERTIFICATE
  WINDOWS_CERTIFICATE_PASSWORD
  OWASIDEA_WINDOWS_CERT_THUMBPRINT
  OWASIDEA_WINDOWS_SIGN_COMMAND
  OWASIDEA_WINDOWS_DIGEST_ALGORITHM
  OWASIDEA_WINDOWS_TIMESTAMP_URL
  APPLE_CERTIFICATE
  APPLE_CERTIFICATE_PASSWORD
  APPLE_SIGNING_IDENTITY
  APPLE_API_KEY
  APPLE_API_ISSUER
  APPLE_API_KEY_PATH
  APPLE_API_KEY_P8
  APPLE_ID
  APPLE_PASSWORD
  APPLE_TEAM_ID
)

cleared=()
for var in "${SIGNING_VARS[@]}"; do
  cleared+=(-u "$var")
done

failures=0

json_field() {
  python3 -c 'import json,sys; print(json.load(sys.stdin)[sys.argv[1]])' "$1"
}

# plan <platform> [VAR=value ...] -> prints JSON, returns the CLI exit code
plan() {
  local platform="$1"
  shift
  env "${cleared[@]}" "$@" "$bin" signing plan --platform "$platform" --json
}

expect_ok() {
  local name="$1" platform="$2" expected="$3"
  shift 3
  local output actual
  if ! output="$(plan "$platform" "$@")"; then
    echo "FAIL $name: planner exited non-zero" >&2
    failures=$((failures + 1))
    return
  fi
  actual="$(printf '%s' "$output" | json_field mode)"
  if [ "$actual" != "$expected" ]; then
    echo "FAIL $name: mode=$actual (expected $expected)" >&2
    failures=$((failures + 1))
    return
  fi
  echo "ok   $name"
}

expect_invalid() {
  local name="$1" platform="$2" missing="$3"
  shift 3
  local output status
  set +e
  output="$(plan "$platform" "$@" 2>&1)"
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    echo "FAIL $name: expected non-zero exit" >&2
    failures=$((failures + 1))
    return
  fi
  if ! printf '%s' "$output" | grep -q "$missing"; then
    echo "FAIL $name: output does not name $missing" >&2
    failures=$((failures + 1))
    return
  fi
  echo "ok   $name"
}

expect_ok "windows: no certificate skips" windows skip \
  WINDOWS_CERTIFICATE="" WINDOWS_CERTIFICATE_PASSWORD=""
expect_ok "windows: pfx signs and imports" windows windows-pfx \
  WINDOWS_CERTIFICATE="ZmFrZQ==" WINDOWS_CERTIFICATE_PASSWORD="not-a-real-secret"
expect_ok "windows: signCommand uses custom signing" windows windows-trusted-signing \
  OWASIDEA_WINDOWS_SIGN_COMMAND="artifact-signing-cli -e https://example %1"
expect_invalid "windows: partial pfx is invalid" windows WINDOWS_CERTIFICATE_PASSWORD \
  WINDOWS_CERTIFICATE="ZmFrZQ=="
expect_ok "macos: no credentials skips" macos skip
expect_ok "macos: developer id + apple id notarizes" macos macos-developer-id \
  APPLE_CERTIFICATE="ZmFrZQ==" \
  APPLE_CERTIFICATE_PASSWORD="not-a-real-secret" \
  APPLE_SIGNING_IDENTITY="Developer ID Application: Example" \
  APPLE_ID="dev@example.com" \
  APPLE_PASSWORD="not-a-real-secret" \
  APPLE_TEAM_ID="TEAMID1234"
expect_invalid "macos: partial certificate is invalid" macos APPLE_CERTIFICATE_PASSWORD \
  APPLE_CERTIFICATE="ZmFrZQ==" \
  APPLE_SIGNING_IDENTITY="Developer ID Application: Example"
expect_ok "linux: always skips" linux skip

# --require turns a skip into a failure.
set +e
env "${cleared[@]}" "$bin" signing plan --platform windows --require >/dev/null 2>&1
status=$?
set -e
if [ "$status" -eq 0 ]; then
  echo "FAIL --require: expected non-zero on skip" >&2
  failures=$((failures + 1))
else
  echo "ok   --require fails when unconfigured"
fi

# The JSON plan must never contain certificate material.
leaked="$(env "${cleared[@]}" \
  WINDOWS_CERTIFICATE="ZmFrZQ==" \
  WINDOWS_CERTIFICATE_PASSWORD="not-a-real-secret" \
  "$bin" signing plan --platform windows --json)"
if printf '%s' "$leaked" | grep -q "not-a-real-secret"; then
  echo "FAIL json: secret value leaked" >&2
  failures=$((failures + 1))
else
  echo "ok   json: no secret values"
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures signing-plan check(s) failed" >&2
  exit 1
fi
echo "signing-plan checks passed"
