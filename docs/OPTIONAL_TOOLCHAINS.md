# Optional toolchains

The framework core is Rust + Wasmtime and the CLI only needs Rust, Node/npm and
(optionally) Go to scaffold and run an app. Everything else is **per language**
or **per packaging target**: a missing toolchain only blocks the plugins that
use it. `owasidea doctor` reports both groups; the required group gates the
command, the optional group is informational.

See also [`GET_STARTED.md`](GET_STARTED.md#2-prerequisites) for the short
prerequisite table and [`PLUGIN_AUTHORING.md`](PLUGIN_AUTHORING.md) for the
language-specific build commands.

## What doctor checks

Required (exit non-zero when missing):

| Tool | Probe | Used by |
|---|---|---|
| `rustc`, `cargo` | `rustc --version`, `cargo --version` | host, CLI, desktop, Rust plugins |
| `cargo-component` | `cargo component --version` | Rust plugins |
| `wasm-tools` | `wasm-tools --version` | manifest generation, composition |
| `go` | `go version` | Go plugins |
| `node`, `npm` | `node --version`, `npm --version` | UI, TypeScript plugins |

Optional (reported only):

| Probe | Needed for | Notes |
|---|---|---|
| `clang` | C/C++ plugins | wasi-sdk ships its own `clang`; a system `clang` is not required |
| `dotnet` | C# plugins | .NET 10 SDK, `net10.0` + `wasi-wasm` |
| `jco` | TypeScript plugins | normally a **local** npm devDependency, not a global binary |
| `componentize-dotnet` | C# (legacy probe) | the example builds through the NuGet SDK, not a global CLI |
| `trunk` | Yew UI plugins | `ui_build = ["trunk", "build", "--release"]` |
| `python3` | helper scripts | not used by the CLI itself |
| `wasi-sdk` | C/C++ plugins | detected at `/opt/wasi-sdk`, `~/.local/wasi-sdk`, `./wasi-sdk` |
| `wasi reactor adapter` | C/C++ plugins | `WASI_PREVIEW1_ADAPTER`, wasi-sdk `share/`, or Cargo registry |

`owasidea doctor --json` prints one object per probe with
`{"tool", "ok", "optional"}`; wasi-sdk and the adapter also carry `path`.
Scripts should read that JSON instead of parsing the table.

## `doctor --fix` and `tools.lock`

```bash
owasidea doctor --fix        # print install commands for missing required tools
owasidea doctor --fix --yes  # run them when they contain no sudo
```

- `tools.lock` (repository root) maps a **required** tool name to a per-OS
  install command. `doctor` reads `./tools.lock` from the current working
  directory, so run it from the repository root (or copy the file).
- Names must match the required probe names exactly: `rustc`, `cargo`,
  `cargo-component`, `wasm-tools`, `go`, `node`, `npm`.
- Without `--yes` nothing is executed. With `--yes`, a command containing
  `sudo` is still only printed, never run.
- `min_version` is documentation: doctor does not compare versions today.
- Optional tools are **not** auto-installed. Install them with the commands
  below; this keeps `--yes` from pulling large toolchains into a machine that
  does not build those languages.

Example entry:

```toml
[[tools]]
name = "wasm-tools"
[tools.install]
linux = "cargo install wasm-tools --locked"
macos = "cargo install wasm-tools --locked"
windows = "cargo install wasm-tools --locked"
```

## TypeScript (`jco`)

TypeScript plugins compile with `jco componentize` from a **local** npm
dependency; `npm install` in the plugin is enough. The Shipyard example pins:

| Package | Version |
|---|---|
| `@bytecodealliance/jco` | `1.39.0` |
| `@bytecodealliance/componentize-js` | `0.23.0` |
| Node.js | 20+ |

```bash
cd plugins/<name>
npm install
```

`owasidea build` runs `npm install` automatically when `node_modules` is
missing, then executes the plugin's `build` script. A global `jco` is only
convenient for ad-hoc use:

```bash
npm install -g @bytecodealliance/jco@1.39.0
```

`doctor` probes the global `jco` binary, so "MISSING jco" is expected and
harmless when the plugin has its own `node_modules`.

## C# (`componentize-dotnet`)

The example targets .NET 10 and builds with `dotnet build -c Release`:

| Component | Version / value |
|---|---|
| .NET SDK | 10.0.x |
| Target framework | `net10.0` |
| Runtime identifier | `wasi-wasm` |
| `BytecodeAlliance.Componentize.DotNet.Wasm.SDK` | `0.8.0-preview*` |
| `runtime.{linux,win}-x64.microsoft.dotnet.ilcompiler.llvm` | `10.0.0-rc.1.26306.1` (pinned in the `.csproj`) |

```bash
# Linux / macOS / Windows
dotnet workload install wasi-experimental   # historically required; CI does it best-effort
dotnet build -c Release                     # inside the plugin directory
```

The project pins the ILCompiler packages itself, so the workload is not always
required; if restore fails, check that both `dotnet-experimental` and
`nuget.org` sources are reachable.

`componentize-dotnet` is a NuGet SDK, not a mandatory global tool. `doctor`
probes a `componentize-dotnet` executable if one happens to be on `PATH`; its
absence does not block the Shipyard C# plugin.

## C and C++ (`wasi-sdk` + reactor adapter)

The CLI auto-detects wasi-sdk in `/opt/wasi-sdk`, `~/.local/wasi-sdk` and
`./wasi-sdk`, and the reactor adapter from `WASI_PREVIEW1_ADAPTER`, the wasi-sdk
`share/` directory, or the Cargo registry. Install the default (wasi-sdk 34.0)
into `~/.local/wasi-sdk` and copy the adapter when found:

```bash
owasidea doctor --install-wasi-sdk
```

- Override the version with `WASI_SDK_VERSION` and the location with
  `WASI_SDK_DIR`.
- `WASI_SDK_PATH` and `WASI_PREVIEW1_ADAPTER` override detection per build.
- `make` (or a per-plugin `[plugins.<name>].build` command) is required; on
  Windows use MSYS2 `make` or an explicit build command, see
  [`WINDOWS.md`](WINDOWS.md#cc-build-override).
- A system `clang` is optional; wasi-sdk's own `clang` is what the Makefiles
  invoke.

## Yew UI (`trunk`)

Yew plugins are static UI bundles built by their own tool:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
```

`owasidea plugin new <name> --lang yew --type ui` writes
`ui_build = ["trunk", "build", "--release"]`; the output must land in
`plugins/<name>/dist/` with `ui_entry` pointing at the produced HTML. Trunk is
not version-pinned here and `doctor` only checks that the binary exists.

## Tauri packaging

`owasidea bundle` needs the Tauri 2 CLI (`cargo install tauri-cli --version
"^2" --locked`) and the platform system libraries. Linux needs WebKitGTK 4.1,
GTK3, libsoup3, `patchelf` and `librsvg`; AppImage additionally uses
`linuxdeploy`, which the Tauri CLI downloads. See
[`CLEAN_BUILD.md`](CLEAN_BUILD.md) and [`APPIMAGE.md`](APPIMAGE.md).

## CI behavior

| Job | Optional toolchains |
|---|---|
| `fast` | none; compiles Rust/Go/TS templates only |
| `full` (Linux) | installs .NET 10, the `wasi-experimental` workload and wasi-sdk so the reference app builds every language, then bundles `deb`/`rpm`, verifies `dist/` and uploads the packages |
| `appimage` | same toolchains, runs only when `bwrap` works; uses `--isolate-host-libs`, verifies and uploads the AppImage, skips safely otherwise |
| `windows` | `owasidea build` and NSIS are `continue-on-error`; C#/C/C++ gaps do not fail the job |
| `macOS` | .NET and wasi-sdk setup are best-effort; packaging is `continue-on-error` |

## Limitations

- `doctor --fix` never installs optional tools and never runs `sudo`.
- Version checks are not enforced by doctor; the pins above are the versions
  the reference app and CI use.
- The TypeScript global `jco` probe does not reflect a plugin's local
  `node_modules`; trust `npm install` plus `owasidea build`.
- wasi-sdk download supports `x86_64` and `aarch64` only; other architectures
  must install it manually and set `WASI_SDK_PATH`.
- Missing optional toolchains are a build error **for that plugin only**; they
  are not a framework or host limitation.
