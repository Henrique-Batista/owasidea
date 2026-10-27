# Code signing and notarization (RL-03)

How Owasidea signs Windows installers (Authenticode) and macOS bundles
(Developer ID + notarization), and how CI skips signing safely when no
certificate is configured.

This document does **not** ship a certificate. The repository, the CI workflow
and the example app contain no private key; every credential is an operator
secret. The Linux packages (`deb`, `rpm`, `AppImage`) are not code-signed.

---

## 1. How CI decides

The CLI has a read-only planning command that inspects **presence** of the
signing variables. It never prints or serializes their values:

```bash
owasidea signing plan --platform windows --json
owasidea signing plan --platform macos --json
owasidea signing plan --platform linux --json   # always skip
owasidea signing plan --platform windows --require  # fail if not configured
```

Modes:

| Mode | Meaning | CI behavior |
| --- | --- | --- |
| `skip` | No credentials at all | Bundle stays unsigned; a step prints what to set |
| `windows-pfx` | PFX secrets, or a preinstalled thumbprint | Import PFX, bundle, verify Authenticode |
| `windows-trusted-signing` | `OWASIDEA_WINDOWS_SIGN_COMMAND` | Bundle with the custom `signCommand` |
| `macos-developer-id` | Developer ID identity; `notarize` tells whether Apple credentials are complete | Bundle, verify `codesign`, validate the staple |
| `invalid` | Partial or contradictory variables | The plan step exits non-zero (no silent skip) |

The plan is produced before the bundle, and its JSON drives the workflow `if`
conditions. `invalid` is deliberate: a missing password or identity is a
configuration error, not "no certificate".

The CLI also projects the **non-secret** values into
`tauri.generated.conf.json`:

| Variable | Generated Tauri field |
| --- | --- |
| `OWASIDEA_WINDOWS_CERT_THUMBPRINT` | `bundle.windows.certificateThumbprint` |
| `OWASIDEA_WINDOWS_SIGN_COMMAND` | `bundle.windows.signCommand` |
| `OWASIDEA_WINDOWS_DIGEST_ALGORITHM` | `bundle.windows.digestAlgorithm` |
| `OWASIDEA_WINDOWS_TIMESTAMP_URL` | `bundle.windows.timestampUrl` |
| `OWASIDEA_MACOS_SIGNING_IDENTITY` | `bundle.macOS.signingIdentity` |

Setting both a thumbprint and a `signCommand` is rejected. Certificates,
passwords and API keys are never rendered; Tauri reads them from its own
environment.

---

## 2. Windows

Signing must happen on Windows (native host or `windows-latest`); there is no
cross-signing from Linux.

### 2.1 PFX certificate (OV/EV)

1. Export the code-signing certificate as `.pfx` (with private key) and base64
   it:

   ```bash
   base64 -w0 certificate.pfx > certificate-base64.txt          # Linux/macOS
   ```

   ```powershell
   # Windows: single-line base64 without certutil's BEGIN/END headers
   [Convert]::ToBase64String([IO.File]::ReadAllBytes("certificate.pfx")) |
     Set-Content certificate-base64.txt -NoNewline
   ```

2. Add the repository secrets:

   | Secret | Content |
   | --- | --- |
   | `WINDOWS_CERTIFICATE` | base64 of the `.pfx` (single line) |
   | `WINDOWS_CERTIFICATE_PASSWORD` | export password |

3. Add the repository variables used for signing metadata:

   | Variable | Example |
   | --- | --- |
   | `OWASIDEA_WINDOWS_DIGEST_ALGORITHM` | `sha256` |
   | `OWASIDEA_WINDOWS_TIMESTAMP_URL` | `http://timestamp.digicert.com` |

   `OWASIDEA_WINDOWS_CERT_THUMBPRINT` is optional: the CI import step writes the
   imported certificate thumbprint to the environment automatically. Set the
   variable only for a self-hosted runner with a preinstalled certificate.

4. CI decodes the PFX to `$RUNNER_TEMP`, imports it into
   `Cert:\CurrentUser\My`, deletes the PFX, and bundles. Tauri calls `signtool`
   with the thumbprint, digest algorithm and timestamp URL.

Verify locally or on the runner:

```powershell
Get-ChildItem .\dist\*-setup.exe | Get-AuthenticodeSignature |
  Format-List Status, StatusMessage, SignerCertificate
```

`Status` must be `Valid`. `UnknownError` usually means the timestamp server was
unreachable.

### 2.2 Azure Artifact Signing (formerly Trusted Signing)

Add secrets `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` and the
variable `OWASIDEA_WINDOWS_SIGN_COMMAND`:

```text
artifact-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> -d Owasidea %1
```

CI installs `artifact-signing-cli` (`cargo install artifact-signing-cli`) only
when that command is configured. The same `signCommand` field works for any
other signing tool (for example `relic sign --file %1 ...`).

### 2.3 Skip behavior

With none of the variables above set, the plan is `skip`: the NSIS/MSI
installers are still built, a workflow step prints the missing names, and the
job does not fail. SmartScreen may warn until the certificate builds
reputation; signing does not guarantee an immediate warning-free download.

---

## 3. macOS

Signing and notarization must run on macOS (`macos-latest`); Apple tooling
(`codesign`, `notarytool`, `stapler`) is required.

### 3.1 Certificate and identity

Export the *Developer ID Application* certificate from Keychain Access as
`.p12`, then base64 it:

```bash
openssl base64 -A -in certificate.p12 -out certificate-base64.txt
```

| Secret | Content |
| --- | --- |
| `APPLE_CERTIFICATE` | base64 of the `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | export password |
| `APPLE_SIGNING_IDENTITY` | exact identity, e.g. `Developer ID Application: Example Corp (TEAMID1234)` |

Find the identity on a Mac with `security find-identity -v -p codesigning`.
Tauri imports the certificate into a temporary keychain, signs the `.app` with
the hardened runtime (Tauri default) and signs the `.dmg`.

### 3.2 Notarization

Set **one** complete credential set:

| App Store Connect API key | Apple ID |
| --- | --- |
| `APPLE_API_KEY` (Key ID) | `APPLE_ID` |
| `APPLE_API_ISSUER` (issuer UUID) | `APPLE_PASSWORD` (app-specific password) |
| `APPLE_API_KEY_P8` (`.p8` content) or `APPLE_API_KEY_PATH` | `APPLE_TEAM_ID` |

`APPLE_API_KEY_P8` is the practical CI form: the workflow writes it to
`$RUNNER_TEMP/owasidea-AuthKey.p8` with mode `0600`, exports
`APPLE_API_KEY_PATH`, and Tauri removes the keychain afterwards. The file is
never committed and is deleted with the runner.

With complete notarization credentials, Tauri runs `notarytool submit --wait`
and staples the ticket. Without them, the plan still signs (`notarize=false`)
with a warning: Gatekeeper will warn on first launch.

Ad-hoc signing (`APPLE_SIGNING_IDENTITY=-`) is supported for local/ARM testing
but cannot be notarized; combining it with notarization variables is rejected as
`invalid`.

### 3.3 Verify

```bash
codesign --verify --deep --strict --verbose=2 "dist/Shipyard.app"
codesign -dv --verbose=4 "dist/Shipyard.app"     # Authority/TeamIdentifier
xcrun stapler validate "dist/Shipyard.app"
xcrun stapler validate "dist/Shipyard.dmg"
spctl --assess --type execute --verbose=2 "dist/Shipyard.app"  # local Gatekeeper check
```

CI runs the first four (the `spctl` assessment depends on the runner's
Gatekeeper state). A missing staple means notarization did not finish.

---

## 4. Local dry runs (no certificate)

The planner is the testable part of the feature and works offline:

```bash
# skip (nothing configured)
owasidea signing plan --platform windows --json

# simulate a complete PFX configuration without a real certificate
WINDOWS_CERTIFICATE=ZmFrZQ== \
WINDOWS_CERTIFICATE_PASSWORD=fake \
  owasidea signing plan --platform windows --json

# partial configuration: exits non-zero and names the missing variable
WINDOWS_CERTIFICATE=ZmFrZQ== \
  owasidea signing plan --platform windows --json

# simulate macOS signing + Apple ID notarization
APPLE_CERTIFICATE=ZmFrZQ== \
APPLE_CERTIFICATE_PASSWORD=fake \
APPLE_SIGNING_IDENTITY="Developer ID Application: Example" \
APPLE_ID=dev@example.com APPLE_PASSWORD=fake APPLE_TEAM_ID=TEAMID1234 \
  owasidea signing plan --platform macos --json
```

The fake values only exercise the decision logic; they do not sign anything.
`scripts/test-signing-plan.sh` runs this matrix (skip, PFX, signCommand,
notarization, partial/invalid, `--require`, no secret leakage) against a built
binary and is executed by the `fast` CI job:

```bash
cargo build -p owasidea-cli
scripts/test-signing-plan.sh
```

To check the generated Tauri config without a certificate:

```bash
OWASIDEA_WINDOWS_CERT_THUMBPRINT=A1B2C3 \
OWASIDEA_WINDOWS_TIMESTAMP_URL=http://timestamp.example \
  owasidea config render --target tauri
```

`tauri.generated.conf.json` must contain `bundle.windows.certificateThumbprint`
and no secret material.

---

## 5. Limitations

- **No cross-signing.** Windows signing needs Windows (`signtool`), macOS
  signing needs macOS (`codesign`/`notarytool`). Linux runners only produce
  unsigned Linux packages.
- **No certificate in this repository.** CI skips signing until an operator adds
  the secrets; `invalid` catches half-configured credentials.
- **Timestamp and notarization need network access.** A blocked timestamp server
  or Apple outage fails the signing step, not the whole build matrix (the
  Windows/macOS bundle steps are best-effort; verification steps surface the
  failure).
- **Notarization is asynchronous.** `notarytool --wait` adds minutes; Apple can
  reject an app for entitlements or hardened-runtime issues that only appear at
  submission.
- **SmartScreen reputation is not instant** for OV/EV certificates issued after
  June 2023.
- **The macOS job is not part of the blocking matrix.** It runs on
  `release/1.0` pushes and tags with `continue-on-error`; the Linux and Windows
  jobs remain the gates.

---

## 6. Secret hygiene

- Secrets are passed only to the steps that need them; the planner receives
  presence only.
- No step enables shell tracing (`set -x`) around credentials; the PFX, `.p8`
  and passwords are written to `$RUNNER_TEMP` with restrictive permissions and
  removed afterwards.
- `tauri.generated.conf.json` and `latest.json` never contain certificate
  content, passwords or API keys.
- Rotate a certificate by replacing the secret; no code change is required.
