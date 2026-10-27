# Remote plugin registry

Owasidea v1 uses a static HTTPS registry index. A registry can be hosted by any
static web server or object store; publishing is a CI/CD upload of signed
manifests, artifacts/assets, and a new index. The CLI is read-only and never
executes registry-provided build scripts.

## Index format v1

The registry URL points directly to `index.json`:

```json
{
  "version": 1,
  "plugins": [
    {
      "name": "notes",
      "version": "1.0.0",
      "description": "Example library",
      "manifest": "packages/notes/notes.manifest.json",
      "artifact": "packages/notes/notes.component.wasm"
    },
    {
      "name": "dashboard",
      "version": "1.0.0",
      "description": "Webview UI",
      "manifest": "packages/dashboard/dashboard.manifest.json",
      "assets": [
        { "path": "index.html", "url": "packages/dashboard/index.html" },
        { "path": "assets/app.js", "url": "packages/dashboard/assets/app.js" }
      ]
    }
  ]
}
```

Manifest and artifact URLs must remain on the index origin. Redirects are
rejected. WASI artifacts are limited to 512 MiB; UI assets to 64 MiB each.
Component paths must be relative and remain beneath the manifest directory.
UI paths are installed only beneath `ui/public/plugins/<name>/`.

## Signing and trust

```bash
owasidea keygen --out keys
owasidea build
owasidea sign notes --manifest-dir manifests --key keys/owasidea.key
```

Ed25519 signature format `owasidea-manifest-v1` authenticates canonical manifest
JSON (including effective permissions, checksums, and UI asset hashes) plus WASI
component bytes. For UI plugins, the signed manifest binds the bundle digest and
per-file hashes. The private key is written mode `0600` on Unix.

Consumers pin a trusted public key and install exact versions:

```bash
owasidea registry search notes --url https://plugins.example/index.json
owasidea registry install notes --version 1.0.0 \
  --url https://plugins.example/index.json \
  --public-key trusted-publisher.pub
```

Install verifies checksum and signature before writing. HTTPS is required except
for localhost tests. Existing files are not replaced unless `--force` is given.
Unsigned, untrusted, malformed, over-sized, cross-origin, and path-traversal
packages are rejected. Search metadata/index itself is not signed; install trust
comes from the plugin signature and explicitly pinned publisher key.

## Publishing checklist

1. Build and validate manifests from actual WIT/static output.
2. Sign each plugin with a protected CI key.
3. Upload versioned files to immutable paths.
4. Publish/update `index.json` over HTTPS.
5. Distribute trusted publisher public keys out-of-band.

The pilot does not provide a hosted marketplace, accounts, billing, reviews, or
an upload API. Registry hosting and publisher identity remain deployment-owned.

## Lockfile

`plugins.lock` (TOML, next to the manifest directory's parent) records installed
registry plugins:

```toml
version = 1

[[plugins]]
name = "notes"
version = "1.0.0"
source = "https://plugins.example/index.json"
sha256 = "<artifact digest>"
signature_format = "owasidea-manifest-v1"
```

Entries are sorted by name then version. `registry install` writes/updates the
lock after checksum and signature verification. `registry install --locked`
requires an existing matching entry (version, source, digest, signature format)
and performs no new resolution; it fails if the entry is missing or differs.
`registry update` consults the registry and updates lock entries (lock only,
no artifact install). `registry outdated` reports lock entries whose version
differs from the registry.
