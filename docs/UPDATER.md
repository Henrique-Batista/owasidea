# Updater (RL-02)

Owasidea bundles ship with the official Tauri 2 updater
(`tauri-plugin-updater`). The framework owns three pieces:

1. **Build-time config** — `[updater]` in `wasm-app.toml` is projected into
   `plugins.updater` in `tauri.generated.conf.json` (endpoints + public key).
2. **Artifacts** — `owasidea bundle` can sign updater artifacts with the Tauri
   CLI and writes `latest.json` next to them.
3. **Runtime** — the desktop shell exposes `updater_check` and `updater_install`
   commands; the UI/bridge can call them through `invoke`.

Nothing here publishes, tags, or signs with a production certificate. Release
hosting and signing keys are operator decisions.

---

## 1. Keys

Generate a minisign keypair with the Tauri CLI (Tauri 2):

```bash
cargo tauri signer generate -w ~/.tauri/owasidea-updater.key
```

It writes:

- `~/.tauri/owasidea-updater.key` — **private** key. Never commit it. In CI it
  lives in the `TAURI_SIGNING_PRIVATE_KEY` secret (content, not path).
- `~/.tauri/owasidea-updater.key.pub` — public key. Safe to commit; point
  `[updater].pubkey` at it (for example `keys/owasidea-updater.pub`; `keys/`
  is gitignored because it holds private keys).

`latest.json` itself is not signed. The updater verifies the signature of the
downloaded artifact against the configured public key; the manifest only
provides the version/URL. That is why the endpoints must be HTTPS and why the
release host must be trusted.

---

## 2. Configuration

```toml
[updater]
channel = "stable"                                    # stable | beta
endpoint = "https://releases.example.com/{{channel}}/latest.json"
pubkey = "keys/owasidea-updater.pub"                  # relative to the app root
notes_file = "docs/RELEASE_NOTES.md"                  # optional
```

| Field | Meaning |
| --- | --- |
| `enabled` | `false` disables rendering and `latest.json` generation. |
| `channel` | `stable` (default) or `beta`. `{{channel}}` is substituted in `endpoint`. |
| `endpoint` | Update manifest URL (or a trailing-`/` base, which gets `latest.json`). `{{channel}}`, `{{target}}`, `{{arch}}`, `{{current_version}}` are supported. HTTPS is required except for loopback. |
| `pubkey` | Minisign public key file. Its **content** is embedded in the Tauri config. |
| `notes` / `notes_file` | Release notes written to `latest.json`. |

Environment overrides (useful for CI and local tests; never committed):

| Variable | Effect |
| --- | --- |
| `OWASIDEA_UPDATE_CHANNEL` | `stable` or `beta`; overrides `[updater].channel`. |
| `OWASIDEA_UPDATER_ENDPOINT` | Comma-separated endpoint(s); overrides `[updater].endpoint`. |
| `OWASIDEA_UPDATER_PUBKEY` | Inline key content **or** a path; overrides `[updater].pubkey`. |
| `OWASIDEA_UPDATER_BASE_URL` | Base URL for artifact `url` fields in `latest.json`. |
| `TAURI_SIGNING_PRIVATE_KEY` | Read by the Tauri CLI to sign artifacts. Presence also enables `bundle.createUpdaterArtifacts`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Key password, when the key is encrypted. |

Generated `tauri.generated.conf.json`:

```json
{
  "plugins": {
    "updater": {
      "endpoints": ["https://releases.example.com/stable/latest.json"],
      "pubkey": "untrusted comment: minisign public key\nRWQ...",
      "dangerousInsecureTransportProtocol": false
    }
  },
  "bundle": { "createUpdaterArtifacts": true }
}
```

`createUpdaterArtifacts` is only set when `TAURI_SIGNING_PRIVATE_KEY` is
present; otherwise `tauri build` would fail for lack of a key. The capability
`updater:default` is declared in `capabilities/default.json`.

---

## 3. Build and generate `latest.json`

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/owasidea-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="…"
owasidea bundle --bundles appimage,nsis
```

Tauri writes `<artifact>.sig` next to each updater artifact. `bundle` then:

1. copies the artifacts into `dist/` (or `dist/<profile>/`),
2. scans for `*.AppImage.sig`, `*.exe.sig`, `*.msi.sig`, `*.app.tar.gz.sig`,
3. writes `dist/latest.json`.

Standalone (also used by `scripts/gen-latest-json.sh`):

```bash
owasidea latest --artifacts dist --url-base https://releases.example.com/stable
owasidea latest --artifacts dist --channel beta --version 1.1.0 \
  --notes-file docs/RELEASE_NOTES.md
owasidea latest --artifacts dist --check      # validate without writing
```

`latest.json` format (Tauri static manifest):

```json
{
  "version": "1.0.0",
  "notes": "…",
  "pub_date": "2026-10-27T12:00:00Z",
  "channel": "stable",
  "platforms": {
    "linux-x86_64": {
      "signature": "…minisign signature…",
      "url": "https://releases.example.com/stable/Shipyard_1.0.0_amd64.AppImage"
    },
    "windows-x86_64": {
      "signature": "…",
      "url": "https://releases.example.com/stable/Shipyard_1.0.0_x64-setup.exe"
    }
  }
}
```

Platform keys: `linux-x86_64` (AppImage), `windows-x86_64` (NSIS `.exe`, or MSI
when no `.exe`), `darwin-x86_64` / `darwin-aarch64` (`.app.tar.gz`). `deb`, `rpm`
and `dmg` are not updater targets and are skipped.

`--check` validates the existing file (semver version, RFC 3339 `pub_date`,
non-empty platforms, URL + signature per platform, channel match) and exits
non-zero on drift — use it in CI before publishing.

---

## 4. Channels

Two channels are supported: `stable` and `beta`. The channel selects the
endpoint and the artifact directory:

```text
dist/stable/latest.json   + stable artifacts
dist/beta/latest.json     + beta artifacts
```

Build each channel separately (profiles are orthogonal — `--profile` changes
identity/artifacts, the channel changes the release line):

```bash
OWASIDEA_UPDATE_CHANNEL=beta owasidea bundle --bundles appimage --profile staging
OWASIDEA_UPDATE_CHANNEL=stable owasidea bundle --bundles appimage
```

Because the endpoint template is resolved at build time, a beta build embeds
`.../beta/latest.json`. Runtime `OWASIDEA_UPDATE_CHANNEL` only labels the
channel in `updater_check` output; it does not rewrite the endpoint. Use
`OWASIDEA_UPDATER_ENDPOINT` to point a build at another manifest.

Profiles can also override the updater block; tables merge, so
`wasm-app.staging.toml` may set `[updater].channel = "beta"` without repeating
the endpoint or key.

---

## 5. Local test with a static server

No publication is needed. Build a debug desktop app, serve `latest.json` over
loopback, and check from the app:

```bash
# 1. artifacts + manifest (a fake signature is enough for a check; install
#    would reject it, which is the point)
mkdir -p /tmp/owasidea-updates/beta
cp dist/Shipyard_*.AppImage /tmp/owasidea-updates/beta/
for f in /tmp/owasidea-updates/beta/*.AppImage; do
  printf 'untrusted comment: local\nRWQlocal\n' > "$f.sig"
done
printf 'untrusted comment: local\nRWQlocal\n' > /tmp/owasidea-updates/pub.key

# 2. write and serve the manifest
OWASIDEA_UPDATER_ENDPOINT=http://127.0.0.1:9000/beta/latest.json \
OWASIDEA_UPDATER_PUBKEY=/tmp/owasidea-updates/pub.key \
owasidea latest --artifacts /tmp/owasidea-updates/beta \
  --url-base http://127.0.0.1:9000/beta --channel beta
cd /tmp/owasidea-updates && python3 -m http.server 9000

# 3. run the desktop shell against it (debug builds allow http endpoints)
OWASIDEA_UPDATER_ENDPOINT=http://127.0.0.1:9000/beta/latest.json \
OWASIDEA_UPDATER_PUBKEY=/tmp/owasidea-updates/pub.key \
owasidea dev --desktop
```

Then call the command from the webview console:

```js
await window.__TAURI__.core.invoke("updater_check");
// { available: true, version: "1.1.0", channel: "beta", … }
```

`app.withGlobalTauri` is enabled in the example shells for this kind of manual
check; the dashboard itself does not depend on it.

For a purely format-level check without a GUI, validate the generated file:

```bash
python3 -m json.tool /tmp/owasidea-updates/beta/latest.json >/dev/null
owasidea latest --artifacts /tmp/owasidea-updates/beta --check
```

Release builds reject plain HTTP endpoints unless
`dangerousInsecureTransportProtocol` is set (only rendered for loopback
endpoints); never enable it for a real release.

---

## 6. CI

`.github/workflows/ci.yml` (`full` job) reads:

- secret `TAURI_SIGNING_PRIVATE_KEY` (+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`),
- repository variables `OWASIDEA_UPDATER_ENDPOINT`, `OWASIDEA_UPDATER_PUBKEY`
  (inline public key), optional `OWASIDEA_UPDATER_BASE_URL` and
  `OWASIDEA_UPDATE_CHANNEL`.

With all three present, the bundle signs artifacts, writes `dist/latest.json`,
and CI runs `owasidea latest --check`. Without them the updater steps print a
skip message and the build stays green. No key material is written to the repo
or to the generated Tauri config.

---

## 7. Runtime behaviour

The Shipyard shell (`examples/shipyard/desktop/src/updater.rs`) registers the plugin and
exposes:

- `updater_check` — builds the updater from the generated config plus the
  `OWASIDEA_UPDATER_*` overrides, calls `check()`, and returns
  `{current_version, available, version, notes, pub_date, download_url, channel}`.
  It never installs.
- `updater_install` — checks, downloads, verifies the signature, installs, and
  restarts. It fails if the key does not match the signature, which is the
  expected outcome for an unsigned or tampered artifact.

The plugin is registered even when unconfigured; `updater_check` then returns an
actionable `updater is not configured` error instead of failing at startup.

---

## 8. Rollback and safety

- The default comparator installs only a **newer** version. To roll back,
  publish a new higher version built from the previous code (or explicitly
  enable `allowDowngrades` in `plugins.updater`, which weakens the guard).
- Keep the previous artifact + `latest.json` on the release host before
  overwriting them; `latest.json` is a single mutable pointer.
- Enable `requireSignedVersion` once all updatable releases are signed with a
  Tauri CLI that records the signed version; it prevents pairing an inflated
  version with an older signed artifact.
- Test the update path on the beta channel and with a local server first
  (this document, section 5). Never test an install against a production
  endpoint.
- Never commit: `*.key`, `TAURI_SIGNING_PRIVATE_KEY` content, or `.sig`/artifact
  files (they are build outputs).
