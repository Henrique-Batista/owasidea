#!/usr/bin/env bash
# RL-04: exercise `owasidea sbom` / `--provenance` without a full build.
#
# Covers: valid CycloneDX/SPDX JSON, Cargo/npm/Go components, deterministic
# output with a fixed SOURCE_DATE_EPOCH, `--check` staleness detection and a
# provenance statement that hashes the artifacts.
#
# Usage:
#   cargo build -p owasidea-cli
#   scripts/test-sbom.sh [path-to-owasidea]
set -euo pipefail

bin="${1:-${OWASIDEA_BIN:-target/debug/owasidea}}"
if [ ! -x "$bin" ]; then
  echo "error: $bin not found; run 'cargo build -p owasidea-cli' first" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat >"$work/wasm-app.toml" <<'EOF'
[app]
name = "sbom-demo"
version = "0.1.0"
EOF

cat >"$work/Cargo.lock" <<'EOF'
version = 4

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
EOF

cat >"$work/package-lock.json" <<'EOF'
{
  "name": "sbom-demo",
  "lockfileVersion": 3,
  "packages": {
    "": {"name": "sbom-demo", "version": "0.1.0"},
    "node_modules/@scope/pkg": {"version": "2.0.0", "integrity": "sha512-3q2+7w=="},
    "node_modules/plain": {"version": "1.2.3"}
  }
}
EOF

cat >"$work/go.mod" <<'EOF'
module example.com/demo

go 1.27.1

require golang.org/x/sys v0.37.0
EOF

cat >"$work/go.sum" <<'EOF'
golang.org/x/sys v0.37.0 h1:fdNQudmxPjkdUTPnLn5mdQv7Zwvbvpaxqs831goi9kQ=
EOF

export SOURCE_DATE_EPOCH=1700000000
cdx="$work/dist/sbom.cdx.json"
spdx="$work/dist/sbom.spdx.json"

"$bin" sbom --config "$work/wasm-app.toml" --output "$cdx" >/dev/null
first_hash="$(sha256sum "$cdx" | cut -d' ' -f1)"

"$bin" sbom --config "$work/wasm-app.toml" --output "$cdx" >/dev/null
second_hash="$(sha256sum "$cdx" | cut -d' ' -f1)"

if [ "$first_hash" != "$second_hash" ]; then
  echo "FAIL determinism: SBOM changed between identical runs" >&2
  exit 1
fi
echo "ok   determinism (SOURCE_DATE_EPOCH=1700000000)"

python3 - "$cdx" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc["bomFormat"] == "CycloneDX", doc.get("bomFormat")
assert doc["specVersion"] == "1.5"
assert doc["serialNumber"].startswith("urn:uuid:")
assert doc["metadata"]["timestamp"] == "2023-11-14T22:13:20Z"
purls = {c["purl"] for c in doc["components"]}
for expected in (
    "pkg:cargo/serde@1.0.0",
    "pkg:npm/%40scope/pkg@2.0.0",
    "pkg:npm/plain@1.2.3",
    "pkg:golang/golang.org/x/sys@v0.37.0",
):
    assert expected in purls, (expected, purls)
PY
echo "ok   CycloneDX JSON (cargo+npm+go components)"

"$bin" sbom --config "$work/wasm-app.toml" --output "$cdx" --check >/dev/null
echo "ok   --check accepts an up-to-date SBOM"

printf ' ' >>"$cdx"
if "$bin" sbom --config "$work/wasm-app.toml" --output "$cdx" --check >/dev/null 2>&1; then
  echo "FAIL --check accepted a stale SBOM" >&2
  exit 1
fi
echo "ok   --check rejects a stale SBOM"

"$bin" sbom --config "$work/wasm-app.toml" --format spdx --output "$spdx" >/dev/null
python3 - "$spdx" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc["spdxVersion"] == "SPDX-2.3"
assert len(doc["packages"]) >= 4
assert all(p["externalRefs"][0]["referenceType"] == "purl" for p in doc["packages"])
PY
echo "ok   SPDX JSON"

printf 'deb-bytes' >"$work/dist/sbom-demo.deb"
"$bin" sbom --config "$work/wasm-app.toml" --output "$cdx" --provenance --artifacts "$work/dist" >/dev/null
python3 - "$work/dist/provenance.intoto.json" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
assert doc["_type"] == "https://in-toto.io/Statement/v1"
assert doc["predicateType"] == "https://slsa.dev/provenance/v1"
assert doc["predicate"]["buildDefinition"]["externalParameters"]["buildId"]
names = {s["name"] for s in doc["subject"]}
assert "sbom-demo.deb" in names, names
assert "sbom.cdx.json" in names, names
assert len(doc["predicate"]["owasidea"]["limitations"]) >= 3
PY
echo "ok   provenance JSON (artifact + SBOM hashes, limitations)"

# Without SOURCE_DATE_EPOCH the CycloneDX document must still be valid and
# omit the volatile timestamp entirely.
unset SOURCE_DATE_EPOCH
"$bin" sbom --config "$work/wasm-app.toml" --output "$work/dist/no-epoch.json" >/dev/null
python3 - "$work/dist/no-epoch.json" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
assert "timestamp" not in doc["metadata"]
PY
echo "ok   no wall-clock timestamp without SOURCE_DATE_EPOCH"

echo "sbom checks passed"
