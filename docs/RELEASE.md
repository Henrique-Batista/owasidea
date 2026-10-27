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
2. Update release notes (`CHANGELOG.md` or the file set in
   `[updater].notes_file`). The ABI section of the changelog follows
   `docs/ABI_POLICY.md`.
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
- `sbom.cdx.json` / `sbom.spdx.json` (RL-04)
- `provenance.intoto.json` when `bundle --provenance` is used (RL-04)

Run `owasidea latest --artifacts <dir> --check` before publishing. It fails on
a missing platform, empty signature, non-HTTPS URL, non-semver version, or a
channel mismatch.

### 4.1 SBOM and provenance (RL-04)

`bundle` writes CycloneDX 1.5 (`sbom.cdx.json`) and SPDX 2.3
(`sbom.spdx.json`) into the output directory by default; `--no-sbom` skips
them. The same documents can be generated standalone:

```bash
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
owasidea sbom --output dist/sbom.cdx.json --provenance --artifacts dist
owasidea sbom --format spdx --output dist/sbom.spdx.json
owasidea sbom --check          # fails when the SBOM is stale
```

The SBOM covers every lockfile under the app root:

| Ecosystem | Source | purl |
| --- | --- | --- |
| Rust | `Cargo.lock` (root, desktop, plugins) | `pkg:cargo/<name>@<version>` |
| npm | `package-lock.json` (root, UI, JS/TS plugins) | `pkg:npm/<name>@<version>` |
| Go | `go.mod` + `go.sum` | `pkg:golang/<module>@<version>` |

Hashes recorded in the lockfiles (`Cargo.lock` checksum, npm `integrity`,
`go.sum` h1) are copied into the component when present.

**Reproducibility.** The documents are deterministic: components are sorted by
purl, the CycloneDX serial number and the SPDX document namespace are derived
from a content hash, and no wall clock is read. `SOURCE_DATE_EPOCH` is the only
timestamp source. Two builds of the same commit with the same epoch therefore
produce byte-identical SBOMs; `owasidea sbom --check` enforces that in CI.
Artifact binaries are only as reproducible as the underlying toolchains — Tauri
and `linuxdeploy` are not byte-reproducible on every host, so compare SBOMs and
provenance subjects rather than whole bundles.

**Provenance.** `--provenance` (or `bundle --provenance`) writes
`provenance.intoto.json`, an in-toto statement with a SLSA provenance v1
predicate:

- `subject`: relative path + SHA-256 of every file in the artifact directory
  (including the SBOM files);
- `buildDefinition.externalParameters`: build id, app name/version, profile,
  `sourceDateEpoch`;
- `internalParameters`: git commit, dirty flag, `owasidea` version;
- `runDetails`: builder id and invocation id (the build id);
- `predicate.owasidea.limitations`: what the attestation does **not** claim.

Limitations, stated explicitly:

- It is **unsigned** and has no builder identity, transparency log, or
  provenance signature; it is SLSA-*like*, not SLSA Build L3.
- It is not a hermetic build: components come from lockfiles in the working
  tree, not from an isolated, resolved dependency graph.
- Artifact hashes describe the output directory at generation time. A dirty
  working tree is recorded (`gitDirty: true`) but does not fail the build.
- `SOURCE_DATE_EPOCH` must be supplied for reproducible timestamps; without it
  the CycloneDX timestamp is omitted and SPDX `created` is the Unix epoch.

CI generates both SBOM formats plus provenance with
`SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)` and uploads them as the
`sbom-provenance` artifact. No secret is needed.

### 4.2 CI bundle verification (FIX-05)

The `full` job builds `deb` and `rpm`, and the `appimage` job builds the
AppImage with `--isolate-host-libs`. Both run
`owasidea verify dist/ --public-key keys/owasidea.pub` and upload their
artifacts:

| Job | Command | Artifact |
| --- | --- | --- |
| `full` | `owasidea bundle --no-build --bundles deb,rpm` | `linux-packages` (`*.deb`, `*.rpm`) |
| `appimage` | `owasidea bundle --no-build --bundles appimage --isolate-host-libs` | `appimage` (`*.AppImage`) |

`bundle` stages `manifests/`, `plugins/*.component.wasm`, `ui/public/` and
`trust/owasidea.pub` into `dist/` next to the installers, so `verify dist/`
checks the manifests, component checksums, Ed25519 signatures and UI asset
digests that actually ship. The release key is pinned out of band
(`--public-key`/`OWASIDEA_PUBLIC_KEY`); the key staged inside `dist/` is not
accepted as its own trust root. A dist without manifests fails with
`NO_MANIFESTS` instead of reporting a vacuous success.

CI creates the example's ephemeral demo key (`owasidea keygen --out keys`)
before `build` because `examples/shipyard/wasm-app.toml` no longer sets
`[signing].generate = true`. Signing secrets are only available to push/tag
builds: Windows certificate import, the NSIS bundle and macOS signing steps are
guarded by `github.event_name != 'pull_request'`.

Third-party actions are currently referenced by major tag (`@v4`). The CI
workflow header records SHA pinning as the intended hardening; when updating a
tag, review the release notes and keep `permissions: contents: read`.

The AppImage job is **conditional**: it installs `bubblewrap`, probes
`bwrap --dev-bind / / true`, and skips with a message when user namespaces are
unavailable or `bwrap` is missing. `deb`/`rpm` still cover Linux in that case.
The job stays `continue-on-error` because `linuxdeploy` and the GTK plugin are
not fully hermetic; see [`APPIMAGE.md`](APPIMAGE.md) and
[`CLEAN_BUILD.md`](CLEAN_BUILD.md).

The Linux jobs install .NET 10, the `wasi-experimental` workload and wasi-sdk
so the reference app builds every language. Optional toolchain details are in
[`OPTIONAL_TOOLCHAINS.md`](OPTIONAL_TOOLCHAINS.md).

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

SBOM and provenance generation (RL-04) needs no secret. The `full` job sets
`SOURCE_DATE_EPOCH` from the last commit, writes `dist/sbom.cdx.json`,
`dist/sbom.spdx.json` and `dist/provenance.intoto.json`, validates them with
`owasidea sbom --check`, and uploads the `sbom-provenance` artifact.

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
