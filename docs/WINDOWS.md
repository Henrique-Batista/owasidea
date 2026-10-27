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
| UI plugins / Vite | supported | n/a |
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
crates/desktop/tauri.windows.conf.json
examples/shipyard/desktop/tauri.windows.conf.json
```

They also select the `.ico` icon. Keep `bundle.resources` in the base config;
platform configs merge with it. The built installer is a normal Tauri/NSIS
executable and installs the host plus the generated manifests/components under
the resource directory.

The Tauri updater uses the NSIS `.exe` (`windows-x86_64` in `latest.json`);
`.msi` is only used when no `.exe` was produced. Updater artifacts must be
signed on Windows too (`TAURI_SIGNING_PRIVATE_KEY` in the Windows runner), and
the same `latest.json` is published for every platform. See
[`UPDATER.md`](UPDATER.md).

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
