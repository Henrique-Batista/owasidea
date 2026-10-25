#!/usr/bin/env python3
"""Generate a plugin manifest from a WebAssembly component.

Capabilities (imports) are extracted from the component's real WIT world, so
the manifest can never under-declare what the plugin needs. The host validates
the manifest against the component at load time.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path


def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True)
    if p.returncode != 0:
        sys.stderr.write(p.stderr)
        raise SystemExit(f"command failed: {' '.join(cmd)}")
    return p.stdout


def wit_json(component: Path):
    return json.loads(run(["wasm-tools", "component", "wit", "--json", str(component)]))


def wit_text(component: Path):
    return run(["wasm-tools", "component", "wit", str(component)])


def package_parts(pkg_name: str):
    # "wasi:io@0.2.3" -> ("wasi:io", "0.2.3")
    if "@" in pkg_name:
        base, version = pkg_name.rsplit("@", 1)
    else:
        base, version = pkg_name, None
    return base, version


def collect_imports(doc):
    interfaces = {i: iface for i, iface in enumerate(doc.get("interfaces", []))}
    packages = doc.get("packages", [])
    world = doc.get("worlds", [{}])[0]

    wasi, host = [], []
    for _key, entry in (world.get("imports") or {}).items():
        iface_ref = entry.get("interface", {}).get("id")
        if iface_ref is None:
            continue
        iface = interfaces.get(iface_ref)
        if iface is None:
            continue
        pkg = packages[iface["package"]]
        base, version = package_parts(pkg["name"])
        iface_name = iface["name"]
        full = f"{base}/{iface_name}"
        if version:
            full += f"@{version}"
        if base.startswith("wasi:"):
            wasi.append(full)
        else:
            host.append(full)
    return sorted(set(wasi)), sorted(set(host))


EXPORT_RE = re.compile(r"^\s*export\s+([A-Za-z0-9_-]+)\s*:\s*(?:async\s+)?(func\(.*?\)(?:\s*->\s*[^;]+)?);", re.M)
EXPORT_IFACE_RE = re.compile(r"^\s*export\s+([A-Za-z0-9:_/-]+(?:@[0-9.]+)?)\s*;", re.M)


def collect_exports(component: Path):
    text = wit_text(component)
    out = []
    for name, sig in EXPORT_RE.findall(text):
        out.append({"name": name, "signature": sig.strip()})
    for name in EXPORT_IFACE_RE.findall(text):
        out.append({"name": name, "signature": "interface"})
    return out


def parse_fs(values):
    out = []
    for v in values or []:
        if ":" in v:
            path, access = v.rsplit(":", 1)
        else:
            path, access = v, "read-write"
        out.append({"path": path, "access": access, "preopen": True})
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--component", required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--version", default="0.1.0")
    ap.add_argument("--language", default="rust",
                    choices=["rust", "csharp", "go", "typescript", "javascript",
                             "c", "cpp"])
    ap.add_argument("--runtime", default="wasi-component",
                    choices=["wasi-component", "js-wasm"])
    ap.add_argument("--type", dest="kind", default="library",
                    choices=["library", "executable", "hybrid"])
    ap.add_argument("--out", required=True)
    ap.add_argument("--description", default="")
    ap.add_argument("--fs", action="append", default=[],
                    help="filesystem permission as path:access (repeatable)")
    ap.add_argument("--kv", default=None,
                    help="host KV permission as namespace:access")
    ap.add_argument("--http-outbound", action="append", default=[],
                    help="allowed outbound host (repeatable)")
    ap.add_argument("--persistent", action="store_true")
    ap.add_argument("--storage", default=None,
                    choices=["wasi-filesystem", "host-kv", "none"])
    ap.add_argument("--entry", action="store_true")
    args = ap.parse_args()

    component = Path(args.component)
    doc = wit_json(component)
    wasi, host = collect_imports(doc)
    exports = collect_exports(component)

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    component_ref = os.path.relpath(component.resolve(), out.parent.resolve())

    kv = None
    if args.kv:
        ns, access = (args.kv.rsplit(":", 1) if ":" in args.kv else (args.kv, "read-write"))
        kv = {"namespace": ns, "access": access}

    storage = args.storage
    if storage is None:
        storage = "wasi-filesystem" if args.persistent and args.fs else (
            "host-kv" if args.persistent and kv else "none")

    manifest = {
        "name": args.name,
        "version": args.version,
        "language": args.language,
        "abi": "wasi-0.3" if any("@0.3." in w for w in wasi) else "wasi-0.2",
        "runtime": args.runtime,
        "type": args.kind,
        "component": component_ref,
        "exports": exports,
        "imports": {"wasi": wasi, "host": host, "plugins": []},
        "permissions": {
            "filesystem": parse_fs(args.fs),
            "http": {
                "outbound": args.http_outbound,
                "inbound": args.kind in ("executable", "hybrid"),
            },
            "kv": kv,
            "resources": {"max_memory_mb": 64, "max_fuel": 1000000000,
                          "timeout_ms": 30000, "max_instances": 1},
        },
        "state": {"persistent": args.persistent, "storage": storage,
                  "root": f"./data/{args.name}" if storage == "wasi-filesystem" else None},
        "composition": {"strategy": "build-time", "entry": args.entry or args.kind == "executable"},
        "metadata": {"author": "", "license": "MIT", "description": args.description,
                 "sha256": hashlib.sha256(component.read_bytes()).hexdigest()},
    }

    out.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {out} ({len(wasi)} wasi imports, {len(host)} host imports, {len(exports)} exports)")


if __name__ == "__main__":
    main()
