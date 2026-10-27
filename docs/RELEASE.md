# Release guide

How to produce a release build of an Owasidea app. The canonical example is
`examples/shipyard`; the same flow applies to any app with `[desktop]` and a
Tauri shell.

This guide does not publish, tag, or push anything. Those are approval-gated
steps performed by a maintainer.

---

## 1. Prepare

1. Set the version once in `wasm-app.toml` (`[app].version`) — it drives the
   Tauri `version`, the runtime descriptor, and `latest.json`.
2. Update release notes (`docs/RELEASE_NOTES.md` or `[updater].notes_file`).
3. Run the gates:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cd examples/shipyard
owasidea build
owasidea smoke
owasidea test
owasidea validate
```

`owasidea validate` must be green before bundling: it checks manifest checksums,
capability drift, ABI fields, and UI bundle digests.

---

## 2. Choose profile and channel

| Knob | Values | Effect |
| --- | --- | --- |
| `--profile` | `dev`, `staging`, `prod` | Overlay `wasm-app.<profile>.toml`; artifacts in `dist/<profile>/`. |
| `OWASIDEA_UPDATE_CHANNEL` | `stable`, `beta` | Selects the updater endpoint (`{{channel}}`) and the `latest.json` channel. |

Typical pairs:

```bash
# beta, staging identity
OWASIDEA_UPDATE_CHANNEL=beta owasidea bundle --profile staging --bundles appimage,nsis

# stable, production identity
OWASIDEA_UPDATE_CHANNEL=stable owasidea bundle --profile prod --bundles appimage,nsis
```

Profiles and channels are independent: profiles change app identity/output
directory, channels change the release line.

---

## 3. Signing

Three independent keys exist; do not mix them:

| Key | Purpose | Where |
| --- | --- | --- |
| `[signing]` Ed25519 | plugin manifests (`owasidea build`/`bundle`) | `keys/owasidea.key` locally; CI secret in release |
| `TAURI_SIGNING_PRIVATE_KEY` (minisign) | updater artifacts (`.sig`) | CI secret only |
| OS code-signing certificate | Windows Authenticode / macOS notarization | CI secrets only (RL-03) |

Without the updater key, `bundle` still succeeds but produces no `.sig` files
and skips `latest.json` with a message. That is the expected local/CI default.
See [`UPDATER.md`](UPDATER.md) for key generation and verification.

### 3.1 Code signing and notarization (RL-03)

Code signing is independent from the updater key. CI asks the CLI for a plan
before bundling:

```bash
owasidea signing plan --platform windows --json   # skip | windows-pfx | windows-trusted-signing
owasidea signing plan --platform macos --json     # skip | macos-developer-id
```

With no certificate secrets the plan is `skip`: the bundle still runs, the
artifacts stay unsigned, and a workflow step lists the missing names. A partial
configuration (certificate without password/identity) is `invalid` and fails the
plan step instead of silently producing an unsigned release.

Windows uses `WINDOWS_CERTIFICATE` + `WINDOWS_CERTIFICATE_PASSWORD` (PFX
imported into the runner store) or `OWASIDEA_WINDOWS_SIGN_COMMAND` (Azure
Artifact Signing and other tools). macOS uses `APPLE_CERTIFICATE` +
`APPLE_CERTIFICATE_PASSWORD` + `APPLE_SIGNING_IDENTITY`, plus one notarization
credential set. Signed artifacts are verified in CI (`Get-AuthenticodeSignature`,
`codesign --verify`, `xcrun stapler validate`). Full variable list, commands,
verification and limitations: [`CODE_SIGNING.md`](CODE_SIGNING.md).

---

## 4. Build artifacts

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/owasidea-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="…"
owasidea bundle --bundles appimage,nsis
```

Output (`dist/` or `dist/<profile>/`):

- Linux: `*.AppImage`, `*.deb`, `*.rpm` (only the AppImage is an updater target)
- Windows: `*-setup.exe` (NSIS), `*.msi`
- macOS: `*.app.tar.gz`
- `*.sig` next to each updater artifact
- `latest.json`
- `tauri.generated.conf.json` (effective config, auditable)

Run `owasidea latest --artifacts <dir> --check` before publishing. It fails on
a missing platform, empty signature, non-HTTPS URL, non-semver version, or a
channel mismatch.

---

## 5. CI secrets and variables

The `full` job in `.github/workflows/ci.yml` reads:

| Name | Kind | Required for updater |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | secret | yes |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | secret | only for encrypted keys |
| `OWASIDEA_UPDATER_ENDPOINT` | variable | yes |
| `OWASIDEA_UPDATER_PUBKEY` | variable (inline public key) | yes |
| `OWASIDEA_UPDATER_BASE_URL` | variable | optional |
| `OWASIDEA_UPDATE_CHANNEL` | variable | optional (`stable`) |

Code signing (RL-03) is optional and independent; see
[`CODE_SIGNING.md`](CODE_SIGNING.md):

| Name | Kind | Required for |
| --- | --- | --- |
| `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD` | secret | Windows PFX signing |
| `OWASIDEA_WINDOWS_SIGN_COMMAND` | variable | Windows custom signing (Azure Artifact Signing, relic, ...) |
| `OWASIDEA_WINDOWS_DIGEST_ALGORITHM` / `OWASIDEA_WINDOWS_TIMESTAMP_URL` | variable | Windows PFX metadata |
| `OWASIDEA_WINDOWS_CERT_THUMBPRINT` | variable | preinstalled Windows certificate (self-hosted) |
| `AZURE_CLIENT_ID` / `AZURE_CLIENT_SECRET` / `AZURE_TENANT_ID` | secret | Azure Artifact Signing |
| `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` | secret | macOS Developer ID signing |
| `APPLE_API_KEY` / `APPLE_API_ISSUER` / `APPLE_API_KEY_P8` | secret | macOS notarization (API key) |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | secret | macOS notarization (Apple ID) |

Missing values are not an error: the bundle step runs without updater signing,
the updater verification step is skipped, and a message explains what to set.
No secret is written to the repo, to `latest.json`, or to the generated Tauri
config.

---

## 6. Publish (maintainer, approval required)

1. Upload artifacts + `.sig` + `latest.json` to the channel path
   (`…/<channel>/`), keeping the previous release until the new one is verified.
2. Smoke-test an installed copy against the beta channel first.
3. Only then promote the same artifacts to stable (re-generate `latest.json`
   with `--channel stable --url-base …/stable` if the base URL differs).
4. Tag and push only with explicit approval; this repository's agents do not
   push or tag.

---

## 7. Rollback

`latest.json` is a single mutable pointer. Keep the previous manifest and
artifact; restoring them rolls new checks back to the older release. The
updater only installs newer versions by default, so a true downgrade needs a
higher version number built from the previous code (or an explicit
`allowDowngrades`, which weakens the guard). See [`UPDATER.md`](UPDATER.md#8-rollback-and-safety).
