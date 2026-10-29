# Windows support

## Short answer

- **Host, Wasmtime, CLI, Tauri shell, UI and WASI plugins:** the design is
  cross-platform and the CLI now has Windows-specific paths for tool discovery,
  `npm`/`npx`, wasi-sdk installation, temporary data, C# runtime packages and
  Tauri NSIS packaging.
- **Native Windows development:** the intended supported path. Build the WASI
  components and the Tauri app on Windows.
- **Cross-compiling the Windows installer from Linux/macOS:** not supported by
  `owasidea bundle` today. WASI components cross-compile, but the Tauri/NSIS
  installer must be produced on Windows (or a Windows CI runner).
- **C/C++ plugins on Windows:** require wasi-sdk for Windows and a `make`
  compatible toolchain (MSYS2) or a `[plugins.<name>].build` override.
- This Linux environment cannot execute a real Windows build. The Windows code
  paths are implemented, but a Windows CI run is still the final proof. Do not
  treat the Linux test suite as validation of the Windows toolchain.

## Support matrix

| Area | Native Windows | Cross-compile from Linux |
|---|---|---|
| Host (`owasidea-host`, Wasmtime) | supported (MSVC build) | Rust target possible, not packaged by `owasidea` |
| CLI (`owasidea`) | supported; `where`, `cmd /C npm/npx`, `USERPROFILE`, Windows temp paths | not the supported path |
| Rust / Go / TypeScript / C# WASI plugins | expected, native toolchains | WASI components cross-build |
| C / C++ WASI plugins | wasi-sdk for Windows + MSYS2 `make` or `build` override | wasi-sdk Linux works |
| Tauri shell | supported; NSIS `.exe` | **not supported** by `owasidea bundle` |
| Signed NSIS/MSI (Authenticode) | supported with a code-signing certificate (PFX or Azure Artifact Signing) | **not supported**: signing must run on Windows |
| UI plugins / Vite | supported | n/a |
| UI plugin bridge (WebView2) | **not validated**; must fail closed (`BRIDGE_DISABLED`, iframe stays isolated) | n/a |
| `owasidea dev/test` | supported (Playwright on Windows) | n/a |

## Native Windows prerequisites

Install:

- Rust MSVC toolchain (`rustup default stable-msvc`) and Visual Studio Build
  Tools with **Desktop development with C++**.
- WebView2 Runtime (usually present on Windows 10/11).
- Node.js 20+ and npm.
- Go 1.27.1+.
- .NET 10 SDK + `dotnet workload install wasi-experimental` for C# plugins.
- Tauri CLI (`cargo install tauri-cli`) and the Tauri 2 Windows prerequisites.
- For C/C++: wasi-sdk and a `make` toolchain. The CLI can install wasi-sdk:
  `owasidea doctor --install-wasi-sdk` downloads the Windows archive.
  Install MSYS2 and add `C:\msys64\usr\bin` to `PATH`, or set a per-plugin
  `build` command.

Optional toolchain versions, install commands and `doctor --fix` behavior are
in [`OPTIONAL_TOOLCHAINS.md`](OPTIONAL_TOOLCHAINS.md). A missing optional
toolchain only blocks the plugins that use that language.

## Commands

Use PowerShell:

```powershell
cargo install --path crates/cli
npm install
npx playwright install
owasidea doctor --install-wasi-sdk
cd examples/shipyard
owasidea build
owasidea test
owasidea dev
owasidea bundle
owasidea bundle --bundles nsis
owasidea bundle --bundles nsis,msi
```

`owasidea bundle` selects `desktop\tauri.windows.conf.json` (default targets
`"all"`), builds the Windows installers, and copies the `.exe`/`.msi` artifacts
to `examples/shipyard\dist`. If MSI (WiX) tooling is missing, use
`--bundles nsis`.

## C/C++ build override

If `make` is not available as `make.exe`, set an explicit command in
`wasm-app.toml`:

```toml
[plugins.fingerprint-c]
path = "plugins/fingerprint-c"
language = "c"
type = "library"
build = ["C:/msys64/usr/bin/make.exe"]
```

The same works for `mingw32-make`, CMake wrappers, or a project-specific build
tool. The default for C/C++ is `make`.

The example Makefiles detect `OS=Windows_NT` and use `clang.exe`/`clang++.exe`.
If your wasi-sdk is elsewhere, set `WASI_SDK_PATH` and
`WASI_PREVIEW1_ADAPTER`; the CLI also auto-detects `%USERPROFILE%\.local\wasi-sdk`
and the Cargo registry adapter.

## Tauri packaging

The base config targets Linux `deb`. The platform override files select NSIS on
Windows:

```text
examples/shipyard/desktop/tauri.windows.conf.json
examples/shipyard/desktop/tauri.windows.conf.json
```

They also select the `.ico` icon. `owasidea bundle` generates
`bundle.resources` in `tauri.generated.conf.json`; keep only targets/icons in
the base config, since platform configs merge with the generated file. The
built installer is a normal Tauri/NSIS executable and installs the host plus the
generated manifests/components under the resource directory.

The Tauri updater uses the NSIS `.exe` (`windows-x86_64` in `latest.json`);
`.msi` is only used when no `.exe` was produced. Updater artifacts must be
signed on Windows too (`TAURI_SIGNING_PRIVATE_KEY` in the Windows runner), and
the same `latest.json` is published for every platform. See
[`UPDATER.md`](UPDATER.md).

## Code signing (Authenticode)

The Windows CI job signs the NSIS/MSI installers when a certificate is
configured; without one it still builds them and prints a skip message. The
decision comes from `owasidea signing plan --platform windows --json`, so a
local build can be checked without a certificate.

Two supported setups:

- **PFX in secrets** — `WINDOWS_CERTIFICATE` (base64 `.pfx`) and
  `WINDOWS_CERTIFICATE_PASSWORD`. CI imports it into
  `Cert:\CurrentUser\My`, and the CLI writes the thumbprint into
  `tauri.generated.conf.json` so Tauri calls `signtool`.
- **Azure Artifact Signing / custom tool** — set the
  `OWASIDEA_WINDOWS_SIGN_COMMAND` variable (for example
  `artifact-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> -d Owasidea %1`)
  and the Azure secrets. CI installs `artifact-signing-cli` only when that
  command is configured.

Verify an installer with:

```powershell
Get-ChildItem .\dist\*-setup.exe | Get-AuthenticodeSignature |
  Format-List Status, StatusMessage, SignerCertificate
```

`Status` must be `Valid`. Full variable list, skip semantics and limitations:
[`CODE_SIGNING.md`](CODE_SIGNING.md). Signing cannot be cross-compiled from
Linux: `signtool` and the certificate store are Windows-only.

## Cross-compilation from Linux

What works today:

- Building WASI components (Rust, Go, TypeScript, C#, C/C++) from Linux.
- Running the cross-built WASI components in a Linux host.
- Cross-checking Rust code for Windows (`cargo check --target ...`) if a
  Windows target/toolchain is installed.

What is not wired/validated:

- `owasidea bundle` for Windows from Linux. Tauri needs Windows WebView2/NSIS
  tooling; `cargo-xwin` and Wine are not integrated or tested.
- Producing a signed Windows installer from a non-Windows host.

Recommended CI split:

- Linux job: `owasidea build`, `owasidea test` (host/API/UI).
- Windows job: `owasidea build`, `owasidea test`, `owasidea bundle`.

The Windows job must install Node, Go, .NET/wasi workload and wasi-sdk; C/C++
also needs MSYS2 `make` or a `build` override.

## Known platform-specific limits

- **UI plugin bridge on WebView2 is not validated.** The bridge handshake
  (ADR-0004) is validated on Chromium and WebKit only. There is no fallback
  implementation: if WebView2 rejects the `MessagePort` transfer or blocks the
  control message, `connect()` must reject with `BRIDGE_DISABLED` and the iframe
  must stay isolated (fail-closed). Do not claim Windows UI-bridge support until
  a WebView2 run passes the CT-06 gate.
- The optional tailcat integration has Linux network hints
  (`/proc/net/route`, Linux gateway detection). It is not the core framework and
  is not required by Shipyard.
- C# needs the Windows LLVM runtime package; the generated `.csproj` now selects
  `runtime.win-x64...` on Windows and `runtime.linux-x64...` on Linux.
- Paths written in `wasm-app.toml` should use `/` in TOML; the CLI accepts them
  on Windows (`PathBuf` normalizes them).
- `OWASIDEA_DATA_DIR`, manifests, components and UI assets are resolved with
  `PathBuf`; no POSIX shell is required by the CLI.

## PX-09 parity notes

- External command shims (`npm`/`npx`/etc.) route through `cmd /C` on Windows via `external_command`/`external_command_path`.
- Shell command strings (doctor install) use `cmd /C` on Windows and `sh -c` on Unix via `shell_command`.
- Tool lookup uses `where` on Windows and `which` on Unix.
- Temp paths use `std::env::temp_dir()` (never hardcoded `/tmp`).
- CI runs `windows-latest` job: fmt, clippy, host/cli tests, build, optional Shipyard build/bundle.
