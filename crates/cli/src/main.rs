use anyhow::Context;
use clap::{Parser, Subcommand};
mod build_id;
mod call;
mod client_gen;
mod config;
mod config_render;
mod dev_desktop;
mod dev_next_steps;
mod logs;
mod status;
mod type_parser;
mod wit_types;
use config::*;
use config_render::*;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "owasidea", version, about = "Owasidea WASM app framework CLI")]
struct Cli {
    #[arg(long, global = true, default_value = "wasm-app.toml")]
    config: PathBuf,
    #[arg(
        long,
        global = true,
        default_value = "manifests",
        env = "OWASIDEA_MANIFESTS"
    )]
    manifest_dir: PathBuf,
    #[arg(long, global = true, env = "HOST_ADDR")]
    http_addr: Option<String>,
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new application directory with wasm-app.toml and folders.
    #[command(about = "Scaffold a new application directory with wasm-app.toml and folders.")]
    Init {
        /// Target directory (optional; prompted interactively).
        #[arg(help = "Target directory.")]
        dir: Option<PathBuf>,
    },
    /// Check required and optional toolchain dependencies.
    #[command(about = "Check required and optional toolchain dependencies.")]
    Doctor {
        /// Download and install wasi-sdk + the reactor adapter when missing.
        #[arg(long)]
        install_wasi_sdk: bool,
    },
    /// Manage plugins (create new plugins).
    #[command(about = "Manage plugins (create new plugins).")]
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// Build all plugins, regenerate manifests, sign, and copy artifacts.
    #[command(about = "Build all plugins, regenerate manifests, sign, and copy artifacts.")]
    Build {
        /// Do not sign manifests even if [signing] is configured.
        #[arg(long)]
        no_sign: bool,
    },
    /// Build, start a temporary host, run smoke checks, then UI tests.
    #[command(about = "Build, start a temporary host, run smoke checks, then UI tests.")]
    Test {
        /// Reuse the current build instead of running `owasidea build`.
        #[arg(long)]
        no_build: bool,
        /// Skip declarative API smoke checks.
        #[arg(long)]
        no_smoke: bool,
        /// Skip the configured UI test command.
        #[arg(long)]
        no_ui: bool,
    },
    /// Build and run the configured API smoke checks.
    #[command(about = "Build and run the configured API smoke checks.")]
    Smoke {
        /// Reuse the current build instead of running `owasidea build`.
        #[arg(long)]
        no_build: bool,
    },
    /// Build, then run the app with the UI dev server and hot reload.
    #[command(about = "Build, then run the app with the UI dev server and hot reload.")]
    Dev {
        /// Reuse the current build instead of running `owasidea build`.
        #[arg(long)]
        no_build: bool,
        /// Also launch `cargo tauri dev` with a generated dev config.
        #[arg(long)]
        desktop: bool,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        no_ui: bool,
        #[arg(long)]
        no_host: bool,
        #[arg(long)]
        no_watch: bool,
    },
    /// Run the current build without file watching.
    #[command(about = "Run the current build without file watching.")]
    Run,
    /// Show host health, build id, plugins and services.
    #[command(about = "Show host health, build id, plugins and services.")]
    Status {
        #[arg(long)]
        watch: bool,
    },
    /// Show plugin logs.
    #[command(about = "Show plugin logs.")]
    Logs {
        /// Plugin name.
        #[arg(help = "Plugin name.")]
        plugin: String,
        /// Follow log stream (SSE).
        #[arg(long)]
        follow: bool,
        /// Filter by level.
        #[arg(long)]
        level: Option<String>,
    },
    /// Inspect a plugin manifest (WIT imports/exports, permissions).
    #[command(about = "Inspect a plugin manifest (WIT imports/exports, permissions).")]
    Inspect {
        /// Plugin name.
        #[arg(help = "Plugin name.")]
        plugin: String,
    },
    /// Call a plugin export over the host HTTP API.
    #[command(about = "Call a plugin export over the host HTTP API.")]
    Call {
        /// Target in the form `<plugin>.<function>` (omit when using --app).
        #[arg(help = "Target in the form <plugin>.<function>.")]
        target: Option<String>,
        /// Executable app plugin name (wasi:http).
        #[arg(long)]
        app: Option<String>,
        /// HTTP method (default GET).
        #[arg(long, default_value = "GET")]
        method: String,
        /// Request path (default /).
        #[arg(long, default_value = "/")]
        path: String,
        /// JSON body, @file, or - for stdin.
        #[arg(long)]
        body: Option<String>,
        /// JSON array of arguments, @file, or - for stdin.
        #[arg(long, default_value = "[]")]
        args: String,
        /// Request timeout in seconds.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Build and package the desktop app with Tauri.
    #[command(about = "Build and package the desktop app with Tauri.")]
    Bundle {
        /// Reuse the current build instead of running `owasidea build`.
        #[arg(long)]
        no_build: bool,
        /// Bundle targets, comma separated (for example deb,rpm,appimage or nsis,msi).
        /// Defaults to the app/tauri config, which uses "all" for Shipyard.
        #[arg(long, value_delimiter = ',')]
        bundles: Vec<String>,
        /// Hide host-private library directories from the bundler (Linux only).
        /// Uses bubblewrap (bwrap) to mask vendor directories such as
        /// /usr/lib/vmware, which linuxdeploy would otherwise scan recursively.
        #[arg(long)]
        isolate_host_libs: bool,
        /// Extra directory to hide when `--isolate-host-libs` is used.
        #[arg(long = "mask-path", value_name = "PATH")]
        mask_paths: Vec<PathBuf>,
    },
    /// Generate an Ed25519 keypair for signing plugins.
    #[command(about = "Generate an Ed25519 keypair for signing plugins.")]
    Keygen {
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Sign a plugin component and update its manifest.
    #[command(about = "Sign a plugin component and update its manifest.")]
    Sign {
        plugin: String,
        #[arg(long)]
        key: PathBuf,
    },
    /// WIT utilities (generate WIT from Rust attributes).
    #[command(about = "WIT utilities (generate WIT from Rust attributes).")]
    Wit {
        #[command(subcommand)]
        cmd: WitCmd,
    },
    /// Validate manifests, checksums and capabilities against config.
    #[command(about = "Validate manifests, checksums and capabilities against config.")]
    Validate,
    /// Generate a typed client from manifests.
    #[command(about = "Generate a typed client from manifests.")]
    Client {
        /// Target language (default ts).
        #[arg(long, default_value = "ts")]
        lang: String,
        /// Output file path.
        #[arg(short = 'o', long = "output")]
        output: PathBuf,
        /// Check mode: do not write; fail if missing/different.
        #[arg(long)]
        check: bool,
    },
    /// Render derived configuration artifacts (runtime/tauri/playwright).
    #[command(about = "Render derived configuration artifacts (runtime/tauri/playwright).")]
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Search or install plugins from a remote HTTPS registry.
    #[command(about = "Search or install plugins from a remote HTTPS registry.")]
    Registry {
        #[command(subcommand)]
        cmd: RegistryCmd,
    },
}

#[derive(Subcommand)]
enum RegistryCmd {
    /// Search a static registry index.
    Search {
        query: String,
        #[arg(long)]
        url: String,
    },
    /// Install a signed plugin artifact from a registry.
    Install {
        name: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        public_key: PathBuf,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Render configuration artifacts.
    #[command(about = "Render configuration artifacts.")]
    Render {
        /// Target: runtime, tauri, playwright, or all.
        #[arg(long, default_value = "all")]
        target: String,
        /// Check mode: do not write; fail if stale/missing.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
enum PluginCmd {
    /// Create a new plugin scaffold.
    #[command(about = "Create a new plugin scaffold.")]
    New {
        name: Option<String>,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, name = "type")]
        r#type: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
    },
}

#[derive(Subcommand)]
enum WitCmd {
    /// Generate WIT from Rust source attributes.
    #[command(about = "Generate WIT from Rust source attributes.")]
    Generate {
        /// Plugin/project directory to scan (defaults to current dir).
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// npm/npx are .cmd shims on Windows and cannot be launched directly by
/// CreateProcess. Route shims through cmd.exe; native executables are unchanged.
fn external_command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let lower = program.to_ascii_lowercase();
        if matches!(lower.as_str(), "npm" | "npx" | "pnpm" | "yarn")
            || lower.ends_with(".cmd")
            || lower.ends_with(".bat")
        {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", program]);
            return command;
        }
    }
    std::process::Command::new(program)
}

fn external_command_path(path: &Path) -> std::process::Command {
    #[cfg(windows)]
    {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(extension.to_ascii_lowercase().as_str(), "cmd" | "bat")
            })
        {
            let mut command = std::process::Command::new("cmd");
            command.arg("/C").arg(path);
            return command;
        }
    }
    std::process::Command::new(path)
}

fn sdk_has_clang(path: &Path) -> bool {
    path.join("bin/clang").is_file() || path.join("bin/clang.exe").is_file()
}

fn tool_path(name: &str) -> Option<std::path::PathBuf> {
    let finder = if cfg!(windows) { "where" } else { "which" };
    if let Ok(output) = std::process::Command::new(finder).arg(name).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines = stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            let selected = lines
                .iter()
                .copied()
                .find(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower.ends_with(".exe") || lower.ends_with(".cmd") || lower.ends_with(".bat")
                })
                .or_else(|| lines.first().copied());
            if let Some(path) = selected {
                return Some(std::path::PathBuf::from(path));
            }
        }
    }
    let candidates = match name {
        "dotnet" => vec![
            "/usr/share/dotnet/dotnet",
            r"C:\Program Files\dotnet\dotnet.exe",
        ],
        "clang" => vec![
            "/usr/bin/clang",
            "/opt/wasi-sdk/bin/clang",
            r"C:\Program Files\LLVM\bin\clang.exe",
        ],
        _ => vec![],
    };
    for c in candidates {
        let p = std::path::Path::new(c);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    None
}

fn find_wasi_sdk() -> Option<PathBuf> {
    for key in ["WASI_SDK_PATH", "WASI_SDK_DIR"] {
        if let Ok(path) = std::env::var(key) {
            let path = PathBuf::from(path);
            if sdk_has_clang(&path) {
                return Some(path);
            }
        }
    }
    let mut candidates = vec![PathBuf::from("/opt/wasi-sdk")];
    if let Some(home) = home_dir() {
        candidates.push(home.join(".local/wasi-sdk"));
        candidates.push(home.join("wasi-sdk"));
    }
    candidates.into_iter().find(|path| sdk_has_clang(path))
}

fn find_wasi_adapter(sdk: Option<&Path>) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("WASI_PREVIEW1_ADAPTER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(sdk) = sdk {
        let bundled = sdk.join("share/wasi_snapshot_preview1.reactor.wasm");
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".cargo")))?;
    let registry = cargo_home.join("registry/src");
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for index in std::fs::read_dir(registry).ok()?.flatten() {
        let index = index.path();
        if !index.is_dir() {
            continue;
        }
        for package in std::fs::read_dir(index).ok()?.flatten() {
            let path = package.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(version) = name.strip_prefix("wasi-preview1-component-adapter-provider-")
            else {
                continue;
            };
            let adapter = path.join("artefacts/wasi_snapshot_preview1.reactor.wasm");
            if !adapter.is_file() {
                continue;
            }
            let parsed = version
                .trim_start_matches('v')
                .split('.')
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>();
            if best.as_ref().is_none_or(|(current, _)| parsed > *current) {
                best = Some((parsed, adapter));
            }
        }
    }
    best.map(|(_, path)| path)
}

fn wasi_toolchain() -> (Option<PathBuf>, Option<PathBuf>) {
    let sdk = find_wasi_sdk();
    let adapter = find_wasi_adapter(sdk.as_deref());
    (sdk, adapter)
}

async fn install_wasi_sdk() -> anyhow::Result<PathBuf> {
    let dest = std::env::var_os("WASI_SDK_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/wasi-sdk")))
        .ok_or_else(|| anyhow::anyhow!("cannot determine install directory; set WASI_SDK_DIR"))?;
    if sdk_has_clang(&dest) {
        if let Some(adapter) = find_wasi_adapter(Some(&dest)) {
            let bundled = dest.join("share/wasi_snapshot_preview1.reactor.wasm");
            if adapter != bundled && !bundled.is_file() {
                if let Some(parent) = bundled.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&adapter, &bundled)?;
            }
        }
        println!("wasi-sdk already installed at {}", dest.display());
        return Ok(dest);
    }
    let version = std::env::var("WASI_SDK_VERSION").unwrap_or_else(|_| "34.0".to_string());
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => anyhow::bail!("wasi-sdk download is not configured for architecture {other}"),
    };
    let release = version.split('.').next().unwrap_or("34");
    let platform = if cfg!(windows) { "windows" } else { "linux" };
    let url = format!(
        "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{release}/wasi-sdk-{version}-{arch}-{platform}.tar.gz"
    );
    println!("downloading {}", url);
    let response = reqwest::get(&url).await?.error_for_status()?;
    let bytes = response.bytes().await?;
    let temp = std::env::temp_dir().join(format!("owasidea-wasi-sdk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp)?;
    let archive = temp.join("wasi-sdk.tar.gz");
    std::fs::write(&archive, &bytes)?;
    let status = std::process::Command::new("tar")
        .arg("xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&temp)
        .status()?;
    if !status.success() {
        anyhow::bail!("failed to extract wasi-sdk archive");
    }
    let extracted = std::fs::read_dir(&temp)?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("wasi-sdk-"))
        })
        .ok_or_else(|| anyhow::anyhow!("wasi-sdk archive did not contain a wasi-sdk directory"))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        anyhow::bail!(
            "{} already exists but is not a wasi-sdk install",
            dest.display()
        );
    }
    // /tmp and $HOME are commonly different filesystems (tmpfs vs disk), so
    // rename can fail with EXDEV. Fall back to a copy staged on the target
    // filesystem, then rename the staged tree into place.
    if std::fs::rename(&extracted, &dest).is_err() {
        let staging = dest.with_file_name(format!(".wasi-sdk-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        copy_dir_all(&extracted, &staging)?;
        if let Err(error) = std::fs::rename(&staging, &dest) {
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_dir_all(&temp);
            return Err(error.into());
        }
    }
    if let Some(adapter) = find_wasi_adapter(Some(&dest)) {
        let bundled = dest.join("share/wasi_snapshot_preview1.reactor.wasm");
        if adapter != bundled {
            std::fs::copy(&adapter, &bundled)?;
        }
    }
    let _ = std::fs::remove_dir_all(&temp);
    println!("installed wasi-sdk at {}", dest.display());
    Ok(dest)
}

fn copy_dir_all(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_all(&path, &target)?;
        } else if file_type.is_symlink() {
            let link = std::fs::read_link(&path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link, &target)?;
            #[cfg(not(unix))]
            std::fs::copy(&path, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let manifest_dir = app_path(&app_root(&cli.config), &cli.manifest_dir);
    match cli.cmd {
        Commands::Init { dir } => cmd_init(dir, cli.json),
        Commands::Doctor { install_wasi_sdk } => cmd_doctor(cli.json, install_wasi_sdk).await,
        Commands::Plugin { cmd } => match cmd {
            PluginCmd::New {
                name,
                lang,
                r#type,
                runtime,
            } => cmd_plugin_new(name, lang, r#type, runtime, &cli.config, cli.json),
        },
        Commands::Build { no_sign } => cmd_build(&cli.config, &cli.manifest_dir, !no_sign),
        Commands::Test {
            no_build,
            no_smoke,
            no_ui,
        } => {
            cmd_test(
                &cli.config,
                &cli.manifest_dir,
                !no_build,
                !no_smoke,
                !no_ui,
                cli.json,
            )
            .await
        }
        Commands::Smoke { no_build } => {
            cmd_test(
                &cli.config,
                &cli.manifest_dir,
                !no_build,
                true,
                false,
                cli.json,
            )
            .await
        }
        Commands::Dev {
            no_build,
            desktop,
            open,
            no_ui,
            no_host,
            no_watch,
        } => {
            let addr = cli
                .http_addr
                .clone()
                .or_else(|| read_host_addr(&cli.config))
                .unwrap_or_else(|| "127.0.0.1:0".to_string());
            cmd_dev(
                &cli.config,
                &cli.manifest_dir,
                &addr,
                !no_build,
                desktop,
                open,
                no_ui,
                no_host,
                no_watch,
            )
            .await
        }
        Commands::Run => {
            let addr = cli
                .http_addr
                .clone()
                .or_else(|| read_host_addr(&cli.config))
                .unwrap_or_else(|| "127.0.0.1:0".to_string());
            apply_signing_env(&cli.config);
            cmd_run(&manifest_dir, &addr).await
        }
        Commands::Status { watch } => {
            let addr = cli
                .http_addr
                .clone()
                .or_else(|| read_host_addr(&cli.config))
                .unwrap_or_else(|| "127.0.0.1:8787".to_string());
            status::cmd_status(&addr, watch, cli.json).await
        }
        Commands::Logs {
            plugin,
            follow,
            level,
        } => {
            let addr = cli
                .http_addr
                .clone()
                .or_else(|| read_host_addr(&cli.config));
            logs::run_logs(
                &plugin,
                follow,
                level.as_deref(),
                cli.json,
                addr.as_deref(),
                None,
            )
            .await
        }
        Commands::Inspect { plugin } => cmd_inspect(&manifest_dir, &plugin, cli.json),
        Commands::Call {
            target,
            app,
            method,
            path,
            body,
            args,
            timeout,
        } => {
            call::cmd_call(
                &cli.config,
                cli.http_addr.as_deref(),
                target.as_deref(),
                app.as_deref(),
                &method,
                &path,
                body.as_deref(),
                &args,
                timeout,
                cli.json,
            )
            .await
        }
        Commands::Bundle {
            no_build,
            bundles,
            isolate_host_libs,
            mask_paths,
        } => cmd_bundle(
            &cli.config,
            &cli.manifest_dir,
            !no_build,
            &bundles,
            isolate_host_libs,
            &mask_paths,
        ),
        Commands::Keygen { out } => cmd_keygen(&out),
        Commands::Sign { plugin, key } => cmd_sign(&manifest_dir, &plugin, &key),
        Commands::Wit { cmd } => match cmd {
            WitCmd::Generate { path } => cmd_wit_generate(&path, cli.json),
        },
        Commands::Validate => cmd_validate(&cli.config, &cli.manifest_dir),
        Commands::Client {
            lang,
            output,
            check,
        } => cmd_client(&manifest_dir, &lang, &output, check),
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Render { target, check } => cmd_config_render(&cli.config, &target, check),
        },
        Commands::Registry { cmd } => match cmd {
            RegistryCmd::Search { query, url } => cmd_registry_search(&query, &url, cli.json).await,
            RegistryCmd::Install {
                name,
                version,
                url,
                public_key,
                force,
            } => {
                cmd_registry_install(
                    &name,
                    &version,
                    &url,
                    &public_key,
                    &manifest_dir,
                    force,
                    cli.json,
                )
                .await
            }
        },
    }
}

fn cmd_init(dir: Option<PathBuf>, json: bool) -> anyhow::Result<()> {
    let interactive = !json && std::io::stdin().is_terminal();
    let target_dir = match dir {
        Some(d) => d,
        None => {
            if interactive {
                let input: String =
                    dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Project directory")
                        .default(".".to_string())
                        .interact_text()?;
                PathBuf::from(input)
            } else {
                PathBuf::from(".")
            }
        }
    };
    std::fs::create_dir_all(&target_dir)?;
    let cfg = target_dir.join("wasm-app.toml");
    if !cfg.exists() {
        let app_name = if interactive {
            dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("App name")
                .default("my-app".to_string())
                .interact_text()?
        } else {
            "my-app".to_string()
        };
        let content = format!(
            r#"[app]
name = "{app_name}"
version = "0.1.0"
identifier = "dev.example.{app_name}"

[host]
addr = "127.0.0.1:8787"

[dev]
watch = true
hot_reload = true
"#
        );
        std::fs::write(&cfg, content)?;
    }
    for sub in ["plugins", "manifests", "ui", "docs"] {
        std::fs::create_dir_all(target_dir.join(sub))?;
    }
    for (relative, contents) in [
        ("package.json", include_str!("../../../ui/package.json")),
        ("index.html", include_str!("../../../ui/index.html")),
        ("main.ts", include_str!("../../../ui/main.ts")),
        ("vite.config.ts", include_str!("../../../ui/vite.config.ts")),
        ("tsconfig.json", include_str!("../../../ui/tsconfig.json")),
    ] {
        let path = target_dir.join("ui").join(relative);
        if !path.exists() {
            std::fs::write(path, contents)?;
        }
    }
    println!("scaffolded {}", target_dir.display());
    Ok(())
}

async fn cmd_doctor(json: bool, install_wasi_sdk_requested: bool) -> anyhow::Result<()> {
    if install_wasi_sdk_requested {
        let sdk = install_wasi_sdk().await?;
        if find_wasi_adapter(Some(&sdk)).is_none() {
            eprintln!(
                "warning: reactor adapter not found; run a Rust/Wasmtime build or set WASI_PREVIEW1_ADAPTER"
            );
        }
    }
    let checks = [
        ("rustc", "rustc --version"),
        ("cargo", "cargo --version"),
        ("cargo-component", "cargo component --version"),
        ("wasm-tools", "wasm-tools --version"),
        ("go", "go version"),
        ("node", "node --version"),
        ("npm", "npm --version"),
    ];
    let optional = [
        ("clang (C/C++)", "clang --version"),
        ("dotnet (C#)", "dotnet --version"),
        ("jco (JS/TS)", "jco --version"),
        ("componentize-dotnet (C#)", "componentize-dotnet --version"),
        ("trunk (Yew UI)", "trunk --version"),
        ("python3 (optional)", "python3 --version"),
    ];
    let (wasi_sdk, wasi_adapter) = wasi_toolchain();
    let mut results: Vec<(&str, bool, bool)> = Vec::new();
    for (name, cmd) in checks {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let ok = external_command(parts[0])
            .args(&parts[1..])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        results.push((name, ok, false));
    }
    for (name, cmd) in optional {
        let tool = cmd.split_whitespace().next().unwrap_or("");
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let mut command = tool_path(tool)
            .map(|path| external_command_path(&path))
            .unwrap_or_else(|| external_command(tool));
        let ok = command
            .args(&parts[1..])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        results.push((name, ok, true));
    }
    if json {
        let mut v: Vec<_> = results
            .iter()
            .map(|(n, ok, opt)| serde_json::json!({"tool": n, "ok": ok, "optional": opt}))
            .collect();
        v.push(serde_json::json!({"tool": "wasi-sdk", "ok": wasi_sdk.is_some(), "optional": true, "path": wasi_sdk}));
        v.push(serde_json::json!({"tool": "wasi reactor adapter", "ok": wasi_adapter.is_some(), "optional": true, "path": wasi_adapter}));
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        for (name, ok, opt) in &results {
            let tag = if *opt { " (optional)" } else { "" };
            let status = if *ok {
                console::style("ok").green().to_string()
            } else {
                console::style("MISSING").red().to_string()
            };
            println!("{:20} {}{}", name, status, tag);
        }
        for (name, path) in [
            ("wasi-sdk", &wasi_sdk),
            ("wasi reactor adapter", &wasi_adapter),
        ] {
            let label = path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "MISSING (needed for C/C++)".to_string());
            println!("{:20} {}", name, label);
        }
    }
    if results.iter().any(|(_, ok, opt)| !ok && !*opt) {
        std::process::exit(1);
    }
    Ok(())
}

#[allow(clippy::useless_format)]
fn cmd_plugin_new(
    name: Option<String>,
    lang: Option<String>,
    kind: Option<String>,
    runtime: Option<String>,
    config_path: &Path,
    json: bool,
) -> anyhow::Result<()> {
    let interactive = !json && std::io::stdin().is_terminal();
    let name = match name {
        Some(n) => n,
        None => {
            if interactive {
                dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Plugin name")
                    .interact_text()?
            } else {
                anyhow::bail!("plugin name required in non-interactive mode");
            }
        }
    };
    let lang = match lang {
        Some(l) => l,
        None => {
            if interactive {
                let items = [
                    "rust",
                    "go",
                    "typescript",
                    "csharp",
                    "c",
                    "cpp",
                    "yew",
                    "blazor",
                    "uno",
                    "svelte",
                    "react",
                ];
                let sel =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Language")
                        .items(&items)
                        .default(0)
                        .interact()?;
                items[sel].to_string()
            } else {
                "rust".to_string()
            }
        }
    };
    let kind = match kind {
        Some(k) => k,
        None => {
            if interactive {
                let items = ["library", "executable", "service", "ui"];
                let sel =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Type")
                        .items(&items)
                        .default(0)
                        .interact()?;
                items[sel].to_string()
            } else {
                "library".to_string()
            }
        }
    };

    validate_plugin_name(&name)?;
    if ![
        "rust",
        "go",
        "typescript",
        "javascript",
        "csharp",
        "c",
        "cpp",
        "yew",
        "blazor",
        "uno",
        "svelte",
        "react",
    ]
    .contains(&lang.as_str())
    {
        anyhow::bail!("unsupported plugin language: {lang}");
    }
    if !["library", "executable", "service", "ui"].contains(&kind.as_str()) {
        anyhow::bail!("unsupported plugin type: {kind}");
    }
    if ["yew", "blazor", "uno", "svelte", "react"].contains(&lang.as_str()) && kind != "ui" {
        anyhow::bail!("{lang} is supported as a webview UI plugin; use --type ui");
    }
    ensure_plugin_config_available(config_path, &name)?;
    let root = app_root(config_path);
    let dir = root.join("plugins").join(&name);
    if dir.exists() {
        anyhow::bail!("plugin directory already exists: {}", dir.display());
    }
    let runtime = runtime.unwrap_or_else(|| {
        if kind == "ui" {
            "js-wasm".to_string()
        } else {
            "wasi-component".to_string()
        }
    });
    if runtime == "js-wasm" {
        if kind != "ui" {
            anyhow::bail!("js-wasm runtime is webview-only; use --type ui");
        }
        if ![
            "typescript",
            "javascript",
            "yew",
            "blazor",
            "uno",
            "svelte",
            "react",
        ]
        .contains(&lang.as_str())
        {
            anyhow::bail!(
                "UI plugins support typescript, javascript, yew, blazor, uno, svelte, or react"
            );
        }
        let dist = dir.join("dist");
        std::fs::create_dir_all(&dist)?;
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head><body><main id=\"app\">UI plugin scaffold</main></body></html>\n",
        )?;
        std::fs::write(
            dir.join("README.md"),
            format!("# {name} UI plugin\n\nLanguage/framework: {lang}\nRuntime: js-wasm (webview-only)\n\nBuild a static browser bundle into `dist/` with an `index.html` entry, then configure this plugin in `wasm-app.toml` with `type = \\\"ui\\\"`, `runtime = \\\"js-wasm\\\"`, and an optional `ui_build = [...]` command. The framework loads it in a sandboxed iframe.\n"),
        )?;
        register_plugin_config(config_path, &name, &lang, &kind, &runtime)?;
        if json {
            println!(
                "{}",
                serde_json::json!({"created": name, "language": lang, "type": kind, "runtime": runtime})
            );
        } else {
            println!("created webview UI plugin scaffold at {}", dir.display());
        }
        return Ok(());
    }
    if runtime != "wasi-component" {
        anyhow::bail!("unsupported plugin runtime: {runtime}");
    }
    if kind == "ui" {
        anyhow::bail!("UI plugins must use --runtime js-wasm");
    }
    if kind == "service" && lang != "rust" {
        anyhow::bail!("service scaffolding currently supports Rust; other languages can use the documented WIT lifecycle contract");
    }

    std::fs::create_dir_all(dir.join("wit"))?;
    std::fs::create_dir_all(dir.join("src"))?;
    let wit = if kind == "service" {
        format!("package poc:{name}@0.1.0;\n\nworld {name} {{\n    export start-service: func() -> result<string, string>;\n    export service-status: func() -> bool;\n}}\n")
    } else {
        format!("package poc:{name}@0.1.0;\n\nworld {name} {{\n    export greet: func(name: string) -> string;\n}}\n")
    };
    std::fs::write(dir.join("wit").join(format!("{name}.wit")), wit)?;
    match lang.as_str() {
        "rust" => {
            std::fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[workspace]\n\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nwit-bindgen = \"0.41\"\n\n[package.metadata.component]\npackage = \"poc:{name}\"\n[package.metadata.component.target]\npath = \"wit\"\nworld = \"{name}\"\n"
                ),
            )?;
            let source = if kind == "service" {
                format!("wit_bindgen::generate!({{ world: \"{name}\", path: \"wit\" }});\n\nuse std::sync::atomic::{{AtomicBool, Ordering}};\nstatic RUNNING: AtomicBool = AtomicBool::new(false);\n\nstruct Component;\nimpl Guest for Component {{\n    fn start_service() -> Result<String, String> {{\n        RUNNING.store(true, Ordering::SeqCst);\n        Ok(\"service started\".to_string())\n    }}\n    fn service_status() -> bool {{\n        RUNNING.load(Ordering::SeqCst)\n    }}\n}}\nexport!(Component);\n")
            } else {
                format!("wit_bindgen::generate!({{ world: \"{name}\", path: \"wit\", generate_all }});\n\nstruct Component;\nimpl Guest for Component {{\n    fn greet(name: String) -> String {{\n        format!(\"Hello, {{}}! (from Rust)\", name)\n    }}\n}}\nexport!(Component);\n")
            };
            std::fs::write(dir.join("src").join("lib.rs"), source)?;
        }
        "go" => {
            std::fs::write(
                dir.join("go.mod"),
                format!("module owasidea/{name}\n\ngo 1.27.1\n\nrequire github.com/bytecodealliance/componentize-go v0.5.1\n\ntool github.com/bytecodealliance/componentize-go\n"),
            )?;
            std::fs::write(
                dir.join("main.go"),
                "package main\n\nimport \"fmt\"\n\nfunc main() {}\n\nfunc Greet(name string) string {\n    return fmt.Sprintf(\"Hello, %s! (from Go)\", name)\n}\n",
            )?;
        }
        "typescript" | "javascript" => {
            std::fs::write(
                dir.join("package.json"),
                format!(
                    "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"scripts\": {{\n    \"build\": \"jco componentize src/index.ts --wit wit --world-name {name} -o {name}.component.wasm\"\n  }},\n  \"devDependencies\": {{\n    \"@bytecodealliance/jco\": \"1.39.0\",\n    \"@bytecodealliance/componentize-js\": \"0.23.0\"\n  }}\n}}\n"
                ),
            )?;
            std::fs::write(
                dir.join("src").join("index.ts"),
                format!(
                    "/** @owasidea-world {name} */\n/** @owasidea-export */\nexport function greet(name: string): string {{\n  return `Hello, ${{name}}! (from TypeScript)`;\n}}\n"
                ),
            )?;
        }
        "csharp" => {
            if kind == "executable" {
                std::fs::write(
                    dir.join(format!("{name}.csproj")),
                    format!(
                        r#"<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <RuntimeIdentifier>wasi-wasm</RuntimeIdentifier>
    <PublishTrimmed>true</PublishTrimmed>
  </PropertyGroup>
</Project>
"#
                    ),
                )?;
                std::fs::write(
                    dir.join("Program.cs"),
                    "using System;\nConsole.WriteLine(\"Hello from C# WASI component!\");\n",
                )?;
            } else {
                std::fs::create_dir_all(dir.join("wit"))?;
                std::fs::create_dir_all(dir.join("wit/deps/host"))?;
                std::fs::write(
                    dir.join(format!("{name}.csproj")),
                    format!(
                        r#"<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <OutputType>Library</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <RootNamespace>{name}</RootNamespace>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <RuntimeIdentifier>wasi-wasm</RuntimeIdentifier>
    <UseAppHost>false</UseAppHost>
    <PublishTrimmed>true</PublishTrimmed>
    <InvariantGlobalization>true</InvariantGlobalization>
    <SelfContained>true</SelfContained>
  </PropertyGroup>

  <ItemGroup>
    <PackageReference Include="BytecodeAlliance.Componentize.DotNet.Wasm.SDK" Version="0.8.0-preview*" />
    <PackageReference Include="runtime.linux-x64.microsoft.dotnet.ilcompiler.llvm" Version="10.0.0-rc.1.26306.1" Condition="$([MSBuild]::IsOSPlatform('Linux'))" />
    <PackageReference Include="runtime.win-x64.microsoft.dotnet.ilcompiler.llvm" Version="10.0.0-rc.1.26306.1" Condition="$([MSBuild]::IsOSPlatform('Windows'))" />
  </ItemGroup>

  <ItemGroup>
    <Wit Update="wit/{name}.wit" World="library" />
  </ItemGroup>
</Project>
"#
                    ),
                )?;
                std::fs::write(
                    dir.join("wit").join(format!("{name}.wit")),
                    format!(
                        "package poc:{name}@0.1.0;\n\ninterface api {{\n    greet: func(name: string) -> string;\n}}\n\nworld library {{\n    export api;\n}}\n"
                    ),
                )?;
                std::fs::write(
                    dir.join("wit/deps/host/host.wit"),
                    "package poc:host@0.1.0;\n\ninterface host-log {\n    log: func(msg: string);\n}\n",
                )?;
                std::fs::write(
                    dir.join(format!("{}Impl.cs", name)),
                    format!(
                        "namespace LibraryWorld.wit.Exports.poc.{name}.v0_1_0;\n\npublic class ApiExportsImpl : IApiExports\n{{\n    public static string Greet(string name)\n    {{\n        return $\"Hello, {{name}}! (from C#)\";\n    }}\n}}\n",
                        name = name
                    ),
                )?;
                std::fs::write(
                    dir.join("nuget.config"),
                    r#"<configuration>
  <packageSources>
    <clear />
    <add key="dotnet-experimental" value="https://pkgs.dev.azure.com/dnceng/public/_packaging/dotnet-experimental/nuget/v3/index.json" />
    <add key="nuget" value="https://api.nuget.org/v3/index.json" />
  </packageSources>
</configuration>
"#,
                )?;
            }
            std::fs::write(
                dir.join("wit").join(format!("{name}.wit")),
                format!(
                    "package poc:{name}@0.1.0;\n\ninterface api {{\n    greet: func(name: string) -> string;\n}}\n\nworld library {{\n    export api;\n}}\n"
                ),
            )?;
            std::fs::write(
                dir.join("wit/deps/host/host.wit"),
                "package poc:host@0.1.0;\n\ninterface host-log {\n    log: func(msg: string);\n}\n",
            )?;
            std::fs::write(
                dir.join(format!("{}Impl.cs", name)),
                format!(
                    "namespace LibraryWorld.wit.Exports.poc.{name}.v0_1_0;\n\npublic class ApiExportsImpl : IApiExports\n{{\n    public static string Greet(string name)\n    {{\n        return $\"Hello, {{name}}! (from C#)\";\n    }}\n}}\n",
                    name = name
                ),
            )?;
            std::fs::write(
                dir.join("nuget.config"),
                r#"<configuration>
  <packageSources>
    <clear />
    <add key="dotnet-experimental" value="https://pkgs.dev.azure.com/dnceng/public/_packaging/dotnet-experimental/nuget/v3/index.json" />
    <add key="nuget" value="https://api.nuget.org/v3/index.json" />
  </packageSources>
</configuration>
"#,
            )?;
        }
        "c" | "cpp" | "c++" => {
            let ext = if lang == "c" { "c" } else { "cpp" };
            let compiler = if lang == "c" { "clang" } else { "clang++" };
            let stdflag = if lang == "c" {
                ""
            } else {
                "-std=c++23 -fno-exceptions"
            };
            std::fs::create_dir_all(dir.join("wit"))?;
            std::fs::create_dir_all(dir.join("src"))?;
            std::fs::write(
                dir.join("wit").join(format!("{name}.wit")),
                format!(
                    "package poc:{name}@0.1.0;\n\ninterface api {{\n    greet: func(name: string) -> string;\n}}\n\nworld {name} {{\n    export api;\n}}\n"
                ),
            )?;
            std::fs::write(
                dir.join("Makefile"),
                format!(
                    "WASI_SDK ?= $(WASI_SDK_PATH)\nADAPTER ?= $(WASI_PREVIEW1_ADAPTER)\nifeq ($(OS),Windows_NT)\nEXE := .exe\nelse\nEXE :=\nendif\nifndef WASI_SDK\n$(error Set WASI_SDK or WASI_SDK_PATH)\nendif\nifndef ADAPTER\n$(error Set WASI_PREVIEW1_ADAPTER to the reactor adapter wasm)\nendif\nCC = $(WASI_SDK)/bin/{compiler}$(EXE)\n\nall:\n\twit-bindgen {lang} wit/{name}.wit --out-dir src\n\t$(CC) --target=wasm32-wasip1 {stdflag} -O2 -c {source} -o src/{name}.o\n\t$(CC) --target=wasm32-wasip1 {stdflag} -O2 -c src/impl.{ext} -o src/impl.o\n\t$(CC) --target=wasm32-wasip1 {stdflag} -O2 src/{name}_component_type.o src/{name}.o src/impl.o -o {name}.wasm -mexec-model=reactor\n\twasm-tools component new {name}.wasm --adapt $(ADAPTER) -o {name}.component.wasm\n",
                    lang = if lang == "c" { "c" } else { "cpp" },
                    compiler = compiler,
                    stdflag = stdflag,
                    source = if lang == "c" { format!("src/{name}.c") } else { format!("src/{name}.cpp") },
                    name = name,
                    ext = ext,
                ),
            )?;
            let impl_content = if lang == "c" {
                format!(
                    "#include \"{name}.h\"\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\nvoid exports_poc_{name}_api_greet({name}_string_t *name, {name}_string_t *ret) {{\n    size_t len = 7 + name->len + 11 + 1;\n    char *buf = (char*)malloc(len);\n    snprintf(buf, len, \"Hello, %.*s! (from C)\", (int)name->len, (char*)name->ptr);\n    {name}_string_dup(ret, buf);\n    free(buf);\n}}\n",
                    name = name
                )
            } else {
                format!(
                    "#include \"{name}_cpp.h\"\n#include <cstring>\n#include <cstdlib>\n\nnamespace exports::poc::{name}::api {{\n    wit::string Greet(wit::string name) {{\n        const char* prefix = \"Hello, \";\n        const char* suffix = \"! (from C++)\";\n        size_t len = 7 + name.size() + 12;\n        char* buf = (char*)malloc(len + 1);\n        memcpy(buf, prefix, 7);\n        memcpy(buf + 7, name.data(), name.size());\n        memcpy(buf + 7 + name.size(), suffix, 12);\n        buf[len] = '\\0';\n        auto result = wit::string(buf, len);\n        return result;\n    }}\n}}\n",
                    name = name
                )
            };
            std::fs::write(dir.join("src").join(format!("impl.{ext}")), impl_content)?;
        }
        _ => {}
    }
    std::fs::write(
        dir.join("README.md"),
        format!("# {name}\n\nLanguage: {lang}\nType: {kind}\n"),
    )?;
    register_plugin_config(config_path, &name, &lang, &kind, &runtime)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"created": name, "language": lang, "type": kind, "runtime": runtime})
        );
    } else {
        println!(
            "created plugin scaffold at {} (lang={}, type={})",
            dir.display(),
            lang,
            kind
        );
    }
    Ok(())
}

fn ensure_plugin_config_available(config_path: &Path, name: &str) -> anyhow::Result<()> {
    if !config_path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(config_path)?;
    let config: toml::Value = toml::from_str(&raw)?;
    if config
        .get("plugins")
        .and_then(toml::Value::as_table)
        .is_some_and(|plugins| plugins.contains_key(name))
    {
        anyhow::bail!(
            "plugin {name} is already configured in {}",
            config_path.display()
        );
    }
    Ok(())
}

fn register_plugin_config(
    config_path: &Path,
    name: &str,
    language: &str,
    kind: &str,
    runtime: &str,
) -> anyhow::Result<()> {
    let mut config = if config_path.exists() {
        std::fs::read_to_string(config_path)?
    } else {
        "[app]\nname = \"my-app\"\nversion = \"0.1.0\"\nidentifier = \"dev.example.myapp\"\n\n[host]\naddr = \"127.0.0.1:8787\"\n\n[dev]\nwatch = true\nhot_reload = true\n".to_string()
    };
    if !config.ends_with('\n') {
        config.push('\n');
    }
    config.push_str(&format!(
        "\n[plugins.\"{name}\"]\npath = \"plugins/{name}\"\nlanguage = \"{language}\"\ntype = \"{kind}\"\nruntime = \"{runtime}\"\n"
    ));
    if kind == "ui" {
        config.push_str("ui_entry = \"index.html\"\n");
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, config)?;
    Ok(())
}

/// App root derived from the config path. Relative plugin paths, manifests and
/// UI assets are always resolved against this directory, so `--config
/// examples/app/wasm-app.toml` works from any working directory.
fn app_root(config_path: &Path) -> PathBuf {
    config::config_dir(config_path)
}

fn app_path(root: &Path, path: impl AsRef<Path>) -> PathBuf {
    // Resolve relative to an already-derived root. `root` is the config
    // directory, so delegate to the shared helper with a synthetic config path
    // is not needed; keep the historical semantics.
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn display_app_path(root: &Path, path: &Path) -> String {
    pathdiff::diff_paths(path, root)
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

#[allow(clippy::too_many_arguments)]
fn generate_manifest(
    component: &Path,
    name: &str,
    version: &str,
    language: &str,
    kind: &str,
    out: &Path,
    fs: &[String],
    kv: Option<&str>,
    http_outbound: &[String],
    network_outbound: &[String],
    network_inbound: &[String],
    persistent: bool,
    build_id: &str,
) -> anyhow::Result<()> {
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;

    // Run wasm-tools component wit --json
    let out_json = std::process::Command::new("wasm-tools")
        .args(["component", "wit", "--json"])
        .arg(component)
        .output()?;
    if !out_json.status.success() {
        anyhow::bail!("wasm-tools component wit --json failed");
    }
    let doc: serde_json::Value = serde_json::from_slice(&out_json.stdout)?;

    let interfaces = doc
        .get("interfaces")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let packages = doc
        .get("packages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let worlds = doc
        .get("worlds")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let world = worlds
        .first()
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    fn package_parts(pkg_name: &str) -> (String, Option<String>) {
        if let Some((base, ver)) = pkg_name.rsplit_once('@') {
            (base.to_string(), Some(ver.to_string()))
        } else {
            (pkg_name.to_string(), None)
        }
    }

    let mut wasi_set = BTreeSet::new();
    let mut host_set = BTreeSet::new();
    if let Some(imports) = world.get("imports").and_then(|v| v.as_object()) {
        for (_key, entry) in imports {
            let iface_id = entry
                .get("interface")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_u64());
            let Some(id) = iface_id else {
                continue;
            };
            let iface = interfaces.get(id as usize);
            let Some(iface) = iface else {
                continue;
            };
            let pkg_idx = iface.get("package").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let Some(pkg) = packages.get(pkg_idx) else {
                continue;
            };
            let pkg_name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let (base, version) = package_parts(pkg_name);
            let iface_name = iface.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let mut full = format!("{}/{}", base, iface_name);
            if let Some(v) = version {
                full = format!("{}@{}", full, v);
            }
            if base.starts_with("wasi:") {
                wasi_set.insert(full);
            } else {
                host_set.insert(full);
            }
        }
    }
    let wasi: Vec<String> = wasi_set.into_iter().collect();
    let host: Vec<String> = host_set.into_iter().collect();
    let imports_host_kv = host.iter().any(|name| name.starts_with("poc:host/host-kv"));
    if imports_host_kv != kv.is_some() {
        anyhow::bail!(
            "plugin {name}: host-kv WIT import and configured kv permission must both be present"
        );
    }
    for host in http_outbound {
        if host == "*"
            || host.is_empty()
            || host.contains('/')
            || (host.contains(':') && host.parse::<std::net::IpAddr>().is_err())
        {
            anyhow::bail!("invalid outbound HTTP host {host:?} for {name}");
        }
    }
    for host in network_outbound {
        if host == "*"
            || host.contains('/')
            || (host.contains(':') && host.parse::<std::net::IpAddr>().is_err())
            || host.is_empty()
        {
            anyhow::bail!("invalid outbound network host {host:?} for {name}");
        }
    }
    for address in network_inbound {
        if address.parse::<std::net::IpAddr>().is_err() {
            anyhow::bail!("inbound network permission must be an IP address: {address}");
        }
    }

    let exports = wit_types::extract_exports(&doc);

    // CT-02: validate structured type grammar for every export param/result.
    for (i, ex) in exports.iter().enumerate() {
        if let Some(params) = ex.get("params").and_then(|v| v.as_array()) {
            for (j, p) in params.iter().enumerate() {
                let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if let Err(e) =
                    type_parser::validate_type(&format!("exports[{}].params[{}].type", i, j), ty)
                {
                    anyhow::bail!("plugin {}: {}", name, e);
                }
            }
        }
        if let Some(results) = ex.get("results").and_then(|v| v.as_array()) {
            for (j, r) in results.iter().enumerate() {
                let ty = r.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if let Err(e) =
                    type_parser::validate_type(&format!("exports[{}].results[{}].type", i, j), ty)
                {
                    anyhow::bail!("plugin {}: {}", name, e);
                }
            }
        }
    }

    // abi
    let abi = if wasi.iter().any(|w| w.contains("@0.3.")) {
        "wasi-0.3"
    } else {
        "wasi-0.2"
    };

    // fs
    let mut fs_perms = Vec::new();
    for v in fs {
        let (path, access) = if let Some((p, a)) = v.rsplit_once(':') {
            (p.to_string(), a.to_string())
        } else {
            (v.clone(), "read-write".to_string())
        };
        if path.is_empty() || !matches!(access.as_str(), "read" | "read-write") {
            anyhow::bail!("invalid filesystem permission {v:?}");
        }
        fs_perms.push(json!({"path": path, "access": access, "preopen": true}));
    }

    // kv
    let kv = if let Some(k) = kv {
        let (ns, access) = if let Some((n, a)) = k.rsplit_once(':') {
            (n.to_string(), a.to_string())
        } else {
            (k.to_string(), "read-write".to_string())
        };
        if ns.is_empty()
            || !ns
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            || !matches!(access.as_str(), "read" | "read-write")
        {
            anyhow::bail!("invalid KV permission {k:?}");
        }
        Some(json!({"namespace": ns, "access": access}))
    } else {
        None
    };

    let storage = if persistent {
        if !fs.is_empty() {
            "wasi-filesystem"
        } else if kv.is_some() {
            "host-kv"
        } else {
            "none"
        }
    } else {
        "none"
    };

    // sha256
    let bytes = std::fs::read(component)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha = format!("{:x}", hasher.finalize());

    let manifest = json!({
        "name": name,
        "version": version,
        "language": language,
        "abi": abi,
        "runtime": "wasi-component",
        "type": kind,
        "component": std::path::Path::new(component).file_name().unwrap_or_default().to_string_lossy(),
        "exports": exports,
        "imports": {"wasi": wasi, "host": host, "plugins": []},
        "permissions": {
            "filesystem": fs_perms,
            "http": {"outbound": http_outbound, "inbound": kind == "executable" || kind == "hybrid"},
            "network": {"outbound": network_outbound, "inbound": network_inbound},
            "kv": kv,
            "resources": {"max_memory_mb": 64, "max_fuel": 1000000000, "timeout_ms": 30000, "max_instances": 1}
        },
        "state": {"persistent": persistent, "storage": storage,
                  "root": if storage == "wasi-filesystem" { format!("./data/{}", name) } else { String::new() }},
        "composition": {"strategy": "build-time", "entry": kind == "executable"},
        "metadata": {"author": "", "license": "MIT", "description": "", "sha256": sha, "build_id": build_id}
    });

    let manifest_dir = out.parent().unwrap_or_else(|| std::path::Path::new("."));
    let rel =
        pathdiff::diff_paths(component, manifest_dir).unwrap_or_else(|| component.to_path_buf());
    let mut manifest = manifest;
    manifest["component"] = json!(rel.to_string_lossy().to_string());

    std::fs::create_dir_all(manifest_dir)?;
    std::fs::write(out, serde_json::to_string_pretty(&manifest)? + "\n")?;
    println!(
        "wrote {} ({} wasi imports, {} host imports, {} exports)",
        out.display(),
        wasi.len(),
        host.len(),
        exports.len()
    );
    Ok(())
}

#[allow(clippy::regex_creation_in_loops, clippy::too_many_lines)]
fn cmd_build(config: &Path, manifest_dir: &Path, sign: bool) -> anyhow::Result<()> {
    let root = app_root(config);
    let raw = std::fs::read_to_string(config)?;
    let cfg: AppConfig = toml::from_str(&raw)?;
    cfg.validate()?;
    let plugins = cfg.plugins.clone().unwrap_or_default();
    if plugins.is_empty() {
        println!("no plugins declared in {}", config.display());
        return Ok(());
    }
    let manifest_dir = app_path(&root, manifest_dir);
    std::fs::create_dir_all(&manifest_dir)?;
    let ui_dir = root.join("ui");
    if ui_dir.join("package.json").is_file() {
        ensure_npm_dependencies(&ui_dir)?;
    }

    let app_version = cfg
        .app
        .as_ref()
        .and_then(|a| a.version.clone())
        .unwrap_or_else(|| "0.1.0".to_string());

    let build_id = build_id::compute_build_id();
    std::env::set_var("OWASIDEA_BUILD_ID", &build_id);

    // Build ordinary plugins first so composed clients can resolve their
    // dependency components in the same invocation.
    let mut ordered: Vec<(&String, &PluginSection)> = plugins.iter().collect();
    ordered.sort_by_key(|(name, plugin)| (!plugin.compose_with.is_empty(), (*name).clone()));

    let mut ui_index = Vec::new();
    for (name, p) in ordered {
        println!("==> Building {} ({})", name, p.language);
        if p.runtime.as_deref() == Some("js-wasm") {
            let manifest = build_ui_plugin(&root, name, p, &app_version, &manifest_dir, &build_id)?;
            ui_index.push(ui_index_entry(name, &manifest));
            continue;
        }
        let plugin_dir = app_path(&root, &p.path);
        let lang = p.language.as_str();
        match lang {
            "rust" => {
                // Generate WIT from Rust attributes if present.
                let _ = generate_wit(&plugin_dir);
                let status = std::process::Command::new("cargo")
                    .args(["component", "build", "--release"])
                    .current_dir(&plugin_dir)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("rust build failed for {}", name);
                }
                let out = std::process::Command::new("cargo")
                    .args(["metadata", "--format-version", "1", "--no-deps"])
                    .current_dir(&plugin_dir)
                    .output()?;
                let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
                let td = meta["target_directory"].as_str().unwrap_or("");
                let artifact_name = name.replace('-', "_");
                let src = std::path::Path::new(td)
                    .join("wasm32-wasip1/release")
                    .join(format!("{}.wasm", artifact_name));
                let dst = plugin_dir.join(format!("{}.component.wasm", name));
                std::fs::copy(&src, &dst)?;
            }
            "go" => {
                let _ = generate_wit(&plugin_dir);
                let wit_dir = plugin_dir.join("wit");
                let status = if wit_dir.exists() {
                    // derive world arg from generated WIT
                    let mut world_arg: Option<String> = None;
                    let world_re = regex::Regex::new(r"world\s+([A-Za-z0-9_-]+)").unwrap();
                    let pkg_re =
                        regex::Regex::new(r"package\s+([A-Za-z0-9_-]+:[A-Za-z0-9_-]+)").unwrap();
                    if let Ok(entries) = std::fs::read_dir(&wit_dir) {
                        for e in entries.flatten() {
                            let pp = e.path();
                            if pp.extension().and_then(|x| x.to_str()) == Some("wit") {
                                if let Ok(content) = std::fs::read_to_string(&pp) {
                                    let world =
                                        world_re.captures(&content).map(|c| c[1].to_string());
                                    let pkg = pkg_re.captures(&content).map(|c| c[1].to_string());
                                    if let (Some(w), Some(pk)) = (world, pkg) {
                                        world_arg = Some(format!("{}/{}", pk, w));
                                    }
                                }
                                break;
                            }
                        }
                    }
                    let mut cmd = std::process::Command::new("go");
                    cmd.current_dir(&plugin_dir);
                    if let Some(wa) = &world_arg {
                        cmd.args([
                            "tool",
                            "componentize-go",
                            "-d",
                            "wit",
                            "-w",
                            wa,
                            "build",
                            "-o",
                            "main.wasm",
                        ]);
                    } else {
                        cmd.args(["tool", "componentize-go", "build"]);
                    }
                    cmd.status()?
                } else {
                    std::process::Command::new("go")
                        .args(["tool", "componentize-go", "build"])
                        .current_dir(&plugin_dir)
                        .status()?
                };
                if !status.success() {
                    anyhow::bail!("go build failed for {}", name);
                }
                let src = plugin_dir.join("main.wasm");
                let dst = plugin_dir.join(format!("{}.component.wasm", name));
                std::fs::copy(&src, &dst)?;
            }
            "typescript" | "javascript" => {
                let _ = generate_wit(&plugin_dir);
                ensure_npm_dependencies(&plugin_dir)?;
                let status = external_command("npm")
                    .args(["run", "build"])
                    .current_dir(&plugin_dir)
                    .status()?;
                if !status.success() {
                    anyhow::bail!(
                        "npm build failed for {} (run `npm install` in {})",
                        name,
                        plugin_dir.display()
                    );
                }
                let produced = std::fs::read_dir(&plugin_dir)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .find(|pp| {
                        pp.file_name()
                            .and_then(|s| s.to_str())
                            .map(|s| s.ends_with(".component.wasm"))
                            .unwrap_or(false)
                    });
                if let Some(src) = produced {
                    let dst = plugin_dir.join(format!("{}.component.wasm", name));
                    if src != dst {
                        std::fs::copy(&src, &dst)?;
                    }
                }
            }
            "c" | "cpp" | "c++" => {
                let _ = generate_wit(&plugin_dir);
                let (wasi_sdk, adapter) = wasi_toolchain();
                let (Some(wasi_sdk), Some(adapter)) = (wasi_sdk, adapter) else {
                    anyhow::bail!(
                        "C/C++ build for {name} needs wasi-sdk and the reactor adapter; run `owasidea doctor --install-wasi-sdk` or set WASI_SDK_PATH and WASI_PREVIEW1_ADAPTER"
                    );
                };
                let mut make = if let Some((program, args)) = p.build.split_first() {
                    let mut command = external_command(program);
                    command.args(args);
                    command
                } else {
                    external_command("make")
                };
                let sdk_env = wasi_sdk.display().to_string().replace('\\', "/");
                let adapter_env = adapter.display().to_string().replace('\\', "/");
                let status = make
                    .env("WASI_SDK_PATH", sdk_env)
                    .env("WASI_PREVIEW1_ADAPTER", adapter_env)
                    .current_dir(&plugin_dir)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("make failed for {}", name);
                }
            }
            "csharp" => {
                let _ = generate_wit(&plugin_dir);
                let dotnet =
                    tool_path("dotnet").unwrap_or_else(|| std::path::PathBuf::from("dotnet"));
                let mut cmd = std::process::Command::new(&dotnet);
                cmd.args(["build", "-c", "Release", "--source", ""])
                    .current_dir(&plugin_dir);
                if let Some(wasi_sdk) = std::env::var_os("WASI_SDK_PATH") {
                    cmd.env("WASI_SDK_PATH", wasi_sdk);
                }
                let status = cmd.status()?;
                if !status.success() {
                    anyhow::bail!("dotnet build failed for {}", name);
                }
                // componentize-dotnet produces <name>.wasm in publish/.
                let mut found = None;
                for candidate in [
                    plugin_dir.join(format!(
                        "bin/Release/net10.0/wasi-wasm/publish/{}.wasm",
                        name
                    )),
                    plugin_dir.join(format!("bin/Release/net10.0/wasi-wasm/{}.wasm", name)),
                ] {
                    if candidate.exists() {
                        found = Some(candidate);
                        break;
                    }
                }
                if found.is_none() {
                    for entry in walkdir(plugin_dir.join("bin")) {
                        if entry.extension().and_then(|s| s.to_str()) == Some("wasm")
                            && !entry.to_string_lossy().contains("component")
                        {
                            found = Some(entry);
                            break;
                        }
                    }
                }
                if let Some(src) = found {
                    let dst = plugin_dir.join(format!("{}.component.wasm", name));
                    std::fs::copy(&src, &dst)?;
                } else {
                    anyhow::bail!("dotnet.wasm not found after build");
                }
            }
            other => anyhow::bail!("unsupported language: {}", other),
        }

        let component = plugin_dir.join(format!("{}.component.wasm", name));
        if !component.exists() {
            anyhow::bail!("component not found after build: {}", component.display());
        }

        // Build-time composition: turn this plugin's output into a client and
        // link the configured provider components into a self-contained one.
        if !p.compose_with.is_empty() {
            let client = plugin_dir.join(format!("{}.client.wasm", name));
            std::fs::rename(&component, &client)?;
            // wasm-tools derives a package name from each definition path and
            // rejects multi-dot file names such as `core-rust.component.wasm`.
            // Stage definitions under simple single-dot names.
            let compose_stage =
                std::env::temp_dir().join(format!("owasidea-compose-{}", std::process::id()));
            std::fs::create_dir_all(&compose_stage)?;
            let mut cmd = std::process::Command::new("wasm-tools");
            cmd.arg("compose").arg(&client);
            for dependency in &p.compose_with {
                let Some(dep) = plugins.get(dependency) else {
                    anyhow::bail!("plugin {name} composes with unknown plugin {dependency:?}");
                };
                let dep_dir = app_path(&root, &dep.path);
                let dep_component = dep_dir.join(format!("{}.component.wasm", dependency));
                if !dep_component.exists() {
                    anyhow::bail!(
                        "compose dependency {dependency} is not built at {}",
                        dep_component.display()
                    );
                }
                let staged = compose_stage.join(format!("{dependency}.wasm"));
                std::fs::copy(&dep_component, &staged)?;
                cmd.arg("-d").arg(&staged);
            }
            cmd.arg("-o").arg(&component);
            let status = cmd.status()?;
            let _ = std::fs::remove_dir_all(&compose_stage);
            if !status.success() {
                anyhow::bail!("wasm-tools compose failed for {name}");
            }
            println!("    composed with: {}", p.compose_with.join(", "));
        }

        let manifest = manifest_dir.join(format!("{}.manifest.json", name));
        generate_manifest(
            &component,
            name,
            &p.version.clone().unwrap_or_else(|| app_version.clone()),
            lang,
            &p.kind,
            &manifest,
            &p.fs,
            p.kv.as_deref(),
            &p.http_outbound,
            &p.network_outbound,
            &p.network_inbound,
            p.persistent,
            &build_id,
        )?;
        println!("    manifest: {}", manifest.display());
    }
    if ui_dir.join("public").is_dir() {
        // Digests must travel with the assets they describe. The dashboard
        // verifies the bundle it actually loaded, even when the running
        // desktop binary and the manifests on disk were built at different
        // times.
        let index_path = ui_dir.join("public/ui-plugins.json");
        std::fs::write(&index_path, serde_json::to_string_pretty(&ui_index)? + "\n")?;
        println!("    UI index: {}", index_path.display());
    }
    if sign {
        sign_all(&root, &manifest_dir, cfg.signing.as_ref())?;
    }
    let _ = config_render::render_targets(
        config,
        config_render::RenderScope::Single(config_render::RenderTarget::Runtime),
        false,
    );
    println!("Build complete.");
    Ok(())
}

fn build_ui_plugin(
    root: &Path,
    name: &str,
    plugin: &PluginSection,
    app_version: &str,
    manifest_dir: &Path,
    build_id: &str,
) -> anyhow::Result<serde_json::Value> {
    validate_plugin_name(name)?;
    if plugin.kind != "ui" {
        anyhow::bail!("js-wasm plugin {name} must declare type = \"ui\"");
    }
    let plugin_dir = app_path(root, &plugin.path);
    if !plugin.ui_build.is_empty() {
        ensure_npm_dependencies(&plugin_dir)?;
        let (program, args) = plugin
            .ui_build
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("empty UI build command"))?;
        let status = external_command(program)
            .args(args)
            .current_dir(&plugin_dir)
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "UI build command failed for {name} (run `npm install` in {} first)",
                plugin_dir.display()
            );
        }
    }

    let source = plugin_dir.join("dist");
    if !source.is_dir() {
        anyhow::bail!("UI plugin build output not found: {}", source.display());
    }
    let entry = plugin.ui_entry.as_deref().unwrap_or("index.html");
    validate_ui_entry(entry)?;
    if !source.join(entry).is_file() {
        anyhow::bail!("UI entry file not found: {}", source.join(entry).display());
    }

    let destination = root.join("ui/public/plugins").join(name);
    let marker = root.join("ui/.owasidea/ui-plugins").join(name);
    let destination = sync_static_tree(root, &source, &destination, &marker)?;
    let (digest, files) = hash_static_tree(&destination)?;
    let manifest = serde_json::json!({
        "name": name,
        "version": plugin.version.as_deref().unwrap_or(app_version),
        "language": plugin.language,
        "abi": "webview-v1",
        "runtime": "js-wasm",
        "type": "ui",
        "component": "",
        "exports": [],
        "imports": {"wasi": [], "host": [], "plugins": []},
        "permissions": {
            "filesystem": [],
            "http": {"outbound": [], "inbound": false},
            "network": {"outbound": [], "inbound": []},
            "kv": null,
            "resources": {"max_memory_mb": null, "max_fuel": null, "timeout_ms": null, "max_instances": 1}
        },
        "state": {"persistent": false, "storage": "none", "root": null},
        "composition": {"strategy": "webview", "entry": false},
        "metadata": {
            "build_id": build_id,
            "ui": {"entry": entry, "sha256": digest, "files": files},
            "description": "Webview-only UI plugin"
        }
    });
    std::fs::create_dir_all(manifest_dir)?;
    std::fs::write(
        manifest_dir.join(format!("{name}.manifest.json")),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    println!("    UI assets: {}", destination.display());
    Ok(manifest)
}

fn ui_index_entry(name: &str, manifest: &serde_json::Value) -> serde_json::Value {
    let ui = &manifest["metadata"]["ui"];
    serde_json::json!({
        "name": manifest["name"],
        "version": manifest["version"],
        "language": manifest["language"],
        "entry": format!(
            "plugins/{name}/{}",
            ui["entry"].as_str().unwrap_or("index.html")
        ),
        "sha256": ui["sha256"],
        "files": ui["files"],
    })
}

fn validate_plugin_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        anyhow::bail!("invalid plugin name {name:?}");
    }
    Ok(())
}

fn validate_ui_entry(entry: &str) -> anyhow::Result<()> {
    let path = Path::new(entry);
    if entry.is_empty()
        || path.is_absolute()
        || !entry
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        anyhow::bail!("UI entry must be a safe relative path: {entry:?}");
    }
    Ok(())
}

fn copy_static_tree(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = destination.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!(
                "symlinks are not allowed in UI bundles: {}",
                source_path.display()
            );
        } else if metadata.is_dir() {
            copy_static_tree(&source_path, &target_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn sync_static_tree(
    project_root: &Path,
    source: &Path,
    destination: &Path,
    marker: &Path,
) -> anyhow::Result<PathBuf> {
    let project_root = std::fs::canonicalize(project_root)?;
    let destination_parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UI destination has no parent directory"))?;
    std::fs::create_dir_all(destination_parent)?;
    let destination_parent = std::fs::canonicalize(destination_parent)?;
    if !destination_parent.starts_with(&project_root) {
        anyhow::bail!("UI output parent resolves outside the project directory");
    }
    let destination = destination_parent.join(
        destination
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("UI destination has no filename"))?,
    );
    let marker_parent = marker
        .parent()
        .ok_or_else(|| anyhow::anyhow!("UI ownership marker has no parent"))?;
    std::fs::create_dir_all(marker_parent)?;
    let marker_parent = std::fs::canonicalize(marker_parent)?;
    if !marker_parent.starts_with(&project_root) {
        anyhow::bail!("UI marker parent resolves outside the project directory");
    }
    let marker = marker_parent.join(
        marker
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("UI ownership marker has no filename"))?,
    );
    if std::fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("UI output ownership marker must not be a symlink");
    }
    if std::fs::symlink_metadata(&destination)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        anyhow::bail!("UI output directory must not be a symlink");
    }
    if destination.exists() && !marker.is_file() {
        anyhow::bail!(
            "refusing to overwrite unowned UI output {}; move it or remove it manually",
            destination.display()
        );
    }
    if destination.exists() {
        clear_static_tree(&destination)?;
    } else {
        std::fs::create_dir_all(&destination)?;
    }
    copy_static_tree(source, &destination)?;
    std::fs::write(marker, b"managed by owasidea build\n")?;
    Ok(destination)
}

fn clear_static_tree(root: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!(
                "refusing to clear symlink from generated UI output: {}",
                path.display()
            );
        }
        if metadata.is_dir() {
            clear_static_tree(&path)?;
            std::fs::remove_dir(&path)?;
        } else if metadata.is_file() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn hash_static_tree(
    root: &Path,
) -> anyhow::Result<(String, std::collections::BTreeMap<String, String>)> {
    use sha2::{Digest, Sha256};
    let mut files = walkdir(root.to_path_buf());
    files.sort();
    let mut file_hashes = std::collections::BTreeMap::new();
    for file in files {
        let relative = file.strip_prefix(root)?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        validate_ui_entry(&relative)?;
        let digest = format!("{:x}", Sha256::digest(std::fs::read(file)?));
        file_hashes.insert(relative, digest);
    }
    Ok((digest_ui_file_list(&file_hashes), file_hashes))
}

fn walkdir(dir: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let pth = e.path();
            if pth.is_dir() {
                out.extend(walkdir(pth));
            } else {
                out.push(pth);
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn cmd_dev(
    config_path: &Path,
    manifest_dir: &Path,
    http_addr: &str,
    build: bool,
    desktop: bool,
    open: bool,
    no_ui: bool,
    no_host: bool,
    no_watch: bool,
) -> anyhow::Result<()> {
    let _ = read_app_config(config_path)?;
    if desktop && no_ui {
        anyhow::bail!("--desktop requires a UI; --no-ui is incompatible with --desktop");
    }
    if no_host && !desktop {
        eprintln!("warning: --no-host without --desktop: UI will have no host backend");
    }
    let root = app_root(config_path);
    if build {
        cmd_build(config_path, manifest_dir, true)?;
    }
    let manifest_dir = app_path(&root, manifest_dir);
    build_id::ensure_env_from_manifests(&manifest_dir);
    // RF-08: detect UI/host build skew.
    let mut stale_bundle = false;
    if root.join("ui").join("package.json").is_file() {
        let manifest_bid = build_id::resolve_from_manifests(&manifest_dir);
        let runtime_bid = build_id::read_runtime_build_id(&root);
        match (runtime_bid, manifest_bid) {
            (Some(rb), Some(mb)) if rb != mb => {
                stale_bundle = true;
                eprintln!(
                    "warning: UI bundle build id ({}) differs from host build id ({}); rebuild the UI bundle",
                    rb, mb
                );
            }
            (None, _) => {
                stale_bundle = true;
                eprintln!(
                    "warning: UI runtime bundle missing or stale (ui/.owasidea/runtime.json); run `owasidea build`"
                );
            }
            _ => {}
        }
    }
    if !build {
        if let Ok(cfg) = read_app_config(config_path) {
            sign_all(&root, &manifest_dir, cfg.signing.as_ref())?;
        }
    }
    apply_signing_env(config_path);
    let dev = std::fs::read_to_string(config_path)
        .ok()
        .and_then(|raw| toml::from_str::<AppConfig>(&raw).ok())
        .and_then(|config| config.dev)
        .unwrap_or_default();
    let http_addr = reserve_http_addr(http_addr)?;
    let host_url = format!("http://{http_addr}");
    let ui_dir = root.join("ui");
    let ui_present = ui_dir.join("package.json").is_file();
    if ui_present {
        ensure_npm_dependencies(&ui_dir)?;
    }
    let ui_port = if ui_present {
        Some(reserve_ui_addr(dev.ui_addr.as_deref())?)
    } else {
        None
    };
    let mut tauri_dev_config_path: Option<PathBuf> = None;
    if let Some(port) = ui_port {
        let ui_url = format!("http://127.0.0.1:{port}");
        match crate::config_render::write_tauri_dev_config(config_path, &ui_url) {
            Ok(path) => {
                println!("Tauri dev config: {} (devUrl {})", path.display(), ui_url);
                tauri_dev_config_path = Some(path);
            }
            Err(error) => eprintln!("warning: could not write tauri.dev.conf.json: {error}"),
        }
    }
    if let Some(port) = ui_port {
        let mut origins = std::env::var("OWASIDEA_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "tauri://localhost,https://tauri.localhost".to_string());
        for origin in [
            format!("http://localhost:{port}"),
            format!("http://127.0.0.1:{port}"),
        ] {
            if !origins.split(',').any(|item| item.trim() == origin) {
                if !origins.is_empty() {
                    origins.push(',');
                }
                origins.push_str(&origin);
            }
        }
        std::env::set_var("OWASIDEA_ALLOWED_ORIGINS", origins);
    }
    let plugins_dir = root.join("plugins");
    let mut watch_paths = vec![plugins_dir.clone(), manifest_dir.clone()];
    watch_paths.retain(|path| path.exists());
    let mut displayed_watch_paths: Vec<String> = watch_paths
        .iter()
        .map(|path| display_app_path(&root, path))
        .collect();
    if ui_present {
        displayed_watch_paths.push("ui (Vite HMR)".to_string());
    }
    let effective_watch = dev.watch && !no_watch;
    let reported_watch_paths = if effective_watch {
        displayed_watch_paths.clone()
    } else {
        Vec::new()
    };
    std::env::set_var("OWASIDEA_DEV_MODE", "true");
    std::env::set_var("OWASIDEA_DEV_HOT_RELOAD", dev.hot_reload.to_string());
    std::env::set_var(
        "OWASIDEA_DEV_WATCH_PATHS",
        serde_json::to_string(&reported_watch_paths)?,
    );
    println!(
        "UI URL:         {}",
        ui_port
            .map(|port| format!("http://127.0.0.1:{port}"))
            .unwrap_or_else(|| "disabled (no ui/package.json)".to_string())
    );
    println!("Host URL:       {host_url}");
    println!(
        "Loaded plugins: {}",
        loaded_plugin_names(&manifest_dir).join(", ")
    );
    println!(
        "Watch paths:    {}",
        if effective_watch {
            displayed_watch_paths.join(", ")
        } else {
            "disabled by [dev].watch".to_string()
        }
    );

    // DL-07: next steps block.
    let ui_url_str = ui_port.map(|p| format!("http://127.0.0.1:{p}"));
    let plugins = loaded_plugin_names(&manifest_dir);
    print!(
        "{}",
        crate::dev_next_steps::dev_next_steps(
            ui_url_str.as_deref(),
            &host_url,
            &plugins,
            stale_bundle,
        )
    );

    let mut children: Vec<std::process::Child> = Vec::new();
    if ui_present && !desktop && !no_ui {
        let vite = ui_dir.join("node_modules/vite/bin/vite.js");
        if !vite.is_file() {
            anyhow::bail!(
                "Vite is not installed; run `npm install` in {}",
                ui_dir.display()
            );
        }
        let ui_port = ui_port.unwrap().to_string();
        let child = std::process::Command::new("node")
            .args([
                "node_modules/vite/bin/vite.js",
                "--host",
                "127.0.0.1",
                "--port",
                ui_port.as_str(),
                "--strictPort",
            ])
            .env("OWASIDEA_HOST", host_url.clone())
            .current_dir(&ui_dir)
            .spawn()?;
        children.push(child);
        println!(
            "[dev] UI ready at http://127.0.0.1:{ui_port} — press Ctrl+C to stop Vite and the host"
        );
    }
    if effective_watch {
        start_dev_watcher(
            watch_paths,
            plugins_dir,
            manifest_dir.to_path_buf(),
            host_url.clone(),
            dev.hot_reload,
        );
    }

    if desktop {
        if !ui_present {
            anyhow::bail!("--desktop requires a UI (ui/package.json)");
        }
        let tauri_config = tauri_dev_config_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("tauri.dev.conf.json was not generated"))?;
        if !crate::dev_desktop::cargo_tauri_available() {
            anyhow::bail!("`cargo tauri` not found; install it with `cargo install tauri-cli`");
        }
        let cfg = read_app_config(config_path)?;
        let desktop_path = cfg.desktop.as_ref().and_then(|d| d.path.clone());
        let desktop_dir = match desktop_path {
            Some(path) => app_path(&root, path),
            None if root.join("desktop/Cargo.toml").is_file() => root.join("desktop"),
            None => root.join("crates/desktop"),
        };
        if !desktop_dir.join("tauri.conf.json").is_file() {
            anyhow::bail!(
                "Tauri config not found in {}; add [desktop] path",
                desktop_dir.display()
            );
        }

        // Host as a subprocess so its logs can be prefixed and managed.
        let mut host_child: Option<_> = if no_host {
            None
        } else {
            let exe = std::env::current_exe()?;
            let hc = std::process::Command::new(&exe)
                .arg("--config")
                .arg(config_path)
                .arg("--manifest-dir")
                .arg(&manifest_dir)
                .arg("--http-addr")
                .arg(http_addr.clone())
                .arg("run")
                .current_dir(&root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("failed to spawn host")?;
            Some(crate::dev_desktop::spawn_prefixed(hc, "host"))
        };

        let ui_port_str = ui_port.unwrap().to_string();
        let vite_child = std::process::Command::new("node")
            .args([
                "node_modules/vite/bin/vite.js",
                "--host",
                "127.0.0.1",
                "--port",
                ui_port_str.as_str(),
                "--strictPort",
            ])
            .env("OWASIDEA_HOST", host_url.clone())
            .current_dir(&ui_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to spawn vite")?;
        let mut vite_child = crate::dev_desktop::spawn_prefixed(vite_child, "ui");

        let host_url_for_tauri = host_url.clone();
        let http_addr_for_tauri = http_addr.to_string();
        let runtime_path = Some(root.join("ui").join(".owasidea").join("runtime.json"));
        let inv = crate::dev_desktop::tauri_dev_invocation(
            &desktop_dir,
            &tauri_config,
            &host_url_for_tauri,
            &http_addr_for_tauri,
            &manifest_dir,
            runtime_path.as_deref(),
            no_host,
        );
        let mut tauri_cmd = std::process::Command::new(&inv.program);
        tauri_cmd.args(&inv.args).current_dir(&inv.cwd);
        for (k, v) in &inv.env {
            tauri_cmd.env(k, v);
        }
        let tauri_child = match tauri_cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                if let Some(hc) = host_child.as_mut() {
                    let _ = hc.kill();
                    let _ = hc.wait();
                }
                let _ = vite_child.kill();
                let _ = vite_child.wait();
                return Err(e.into());
            }
        };
        let tauri_child = crate::dev_desktop::spawn_prefixed(tauri_child, "tauri");

        if open {
            let url = ui_port
                .map(|p| format!("http://127.0.0.1:{p}"))
                .unwrap_or_else(|| host_url.clone());
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).status();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).status();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", &url])
                .status();
        }
        // Wait for any process to exit or Ctrl+C.
        let tauri_slot = std::sync::Arc::new(std::sync::Mutex::new(Some(tauri_child)));
        let tauri_slot_wait = tauri_slot.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || loop {
            let mut guard = match tauri_slot_wait.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if let Some(child) = guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        guard.take();
                        let _ = tx.send(Ok(status));
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        guard.take();
                        let _ = tx.send(Err(error));
                        return;
                    }
                }
            } else {
                return;
            }
            drop(guard);
            std::thread::sleep(std::time::Duration::from_millis(50));
        });

        tokio::select! {
            _ = wait_for_shutdown() => {
                println!("[dev] shutting down; stopping tauri, UI and host");
            }
            res = tokio::task::spawn_blocking(move || rx.recv()) => {
                match res {
                    Ok(Ok(Ok(status))) => println!("[tauri] exited with {status}"),
                    _ => println!("[tauri] exited"),
                }
            }
        }
        if let Ok(mut guard) = tauri_slot.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Some(hc) = host_child.as_mut() {
            let _ = hc.kill();
            let _ = hc.wait();
        }
        let _ = vite_child.kill();
        let _ = vite_child.wait();
        return Ok(());
    }

    let host_task = if !desktop && !no_host {
        Some(tokio::spawn(async move {
            owasidea_host::run(manifest_dir.clone(), http_addr.clone()).await
        }))
    } else {
        None
    };
    let result = if let Some(mut host_task) = host_task {
        tokio::select! {
            joined = &mut host_task => match joined {
                Ok(result) => result,
                Err(error) => Err(anyhow::anyhow!("host task failed: {error}")),
            },
            _ = wait_for_shutdown() => {
                println!("[dev] shutting down; stopping UI and host");
                host_task.abort();
                let _ = host_task.await;
                Ok(())
            }
        }
    } else {
        Ok(())
    };
    for mut child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn reserve_http_addr(addr: &str) -> anyhow::Result<String> {
    let requested: std::net::SocketAddr = addr.parse()?;
    if requested.port() != 0 {
        return Ok(requested.to_string());
    }
    let listener = std::net::TcpListener::bind(requested)?;
    Ok(listener.local_addr()?.to_string())
}

fn reserve_ui_addr(addr: Option<&str>) -> anyhow::Result<u16> {
    if let Some(addr) = addr {
        let requested: std::net::SocketAddr = addr.parse()?;
        let ip = requested.ip();
        let start = requested.port();
        if start == 0 {
            let listener = std::net::TcpListener::bind((ip, 0))?;
            return Ok(listener.local_addr()?.port());
        }
        for port in start..=u16::MAX {
            if let Ok(listener) = std::net::TcpListener::bind((ip, port)) {
                return Ok(listener.local_addr()?.port());
            }
        }
        anyhow::bail!("no free UI dev port found starting at {start}")
    }
    reserve_ui_port()
}

fn reserve_ui_port() -> anyhow::Result<u16> {
    for port in 5173..=5273 {
        if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            return Ok(listener.local_addr()?.port());
        }
    }
    anyhow::bail!("no free UI dev port found in 5173-5273")
}

fn loaded_plugin_names(manifest_dir: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(manifest_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path
                .file_name()?
                .to_string_lossy()
                .ends_with(".manifest.json")
            {
                return None;
            }
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            value["name"].as_str().map(str::to_string)
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn start_dev_watcher(
    paths: Vec<PathBuf>,
    plugins_dir: PathBuf,
    manifest_dir: PathBuf,
    host_url: String,
    hot_reload: bool,
) {
    std::thread::spawn(move || {
        let root = plugins_dir.parent().map(PathBuf::from).unwrap_or_default();
        let mut previous = snapshot_watch_paths(&paths);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            let current = snapshot_watch_paths(&paths);
            let mut reload_plugins = std::collections::BTreeSet::new();
            let mut ui_plugins_to_rebuild = std::collections::BTreeSet::new();
            for (changed, state) in &current {
                if previous.get(changed) == Some(state) {
                    continue;
                }
                if let Some(plugin) = plugin_changed_by_path(changed, &manifest_dir) {
                    println!(
                        "[dev] changed {}; validating/reloading {plugin}",
                        changed.display()
                    );
                    if hot_reload {
                        reload_plugins.insert(plugin.to_string());
                    }
                } else if let Some(name) = ui_plugin_name_for_path(changed, &plugins_dir) {
                    ui_plugins_to_rebuild.insert(name);
                } else if changed.starts_with(&plugins_dir)
                    && matches!(
                        changed.extension().and_then(|extension| extension.to_str()),
                        Some(
                            "rs" | "go"
                                | "wit"
                                | "toml"
                                | "cs"
                                | "ts"
                                | "tsx"
                                | "js"
                                | "jsx"
                                | "svelte"
                                | "vue"
                                | "c"
                                | "cpp"
                                | "h"
                                | "hpp"
                                | "html"
                                | "css"
                        )
                    )
                {
                    println!(
                        "[dev] source changed: {}; run `owasidea build` to compile",
                        changed.display()
                    );
                }
            }
            if !ui_plugins_to_rebuild.is_empty() {
                // Debounce: wait for the directory copy to settle.
                std::thread::sleep(std::time::Duration::from_millis(300));
                for name in ui_plugins_to_rebuild {
                    println!("[dev] UI plugin dist changed: {name}; rebuilding index");
                    rebuild_ui_plugin_from_dist(&root, &name, &manifest_dir);
                }
            }
            if !reload_plugins.is_empty() {
                for plugin in reload_plugins {
                    let url = format!("{host_url}/plugin/{plugin}/reload");
                    match reqwest::blocking::Client::new().post(url).send() {
                        Ok(response) if response.status().is_success() => {
                            println!("[dev] reloaded {plugin}");
                        }
                        Ok(response) => {
                            eprintln!("[dev] reload {plugin} failed: {}", response.status())
                        }
                        Err(error) => eprintln!("[dev] reload {plugin} failed: {error}"),
                    }
                }
            }
            previous = current;
        }
    });
}

fn snapshot_watch_paths(paths: &[PathBuf]) -> HashMap<PathBuf, (std::time::SystemTime, u64)> {
    fn visit(root: &Path, files: &mut HashMap<PathBuf, (std::time::SystemTime, u64)>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("target" | "node_modules" | "vendor" | ".git" | ".owasidea")
                ) {
                    visit(&path, files);
                }
            } else if metadata.is_file() {
                let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
                files.insert(path, (modified, metadata.len()));
            }
        }
    }

    let mut files = HashMap::new();
    for path in paths {
        if path.is_dir() {
            visit(path, &mut files);
        }
    }
    files
}

fn plugin_changed_by_path(path: &Path, manifest_dir: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    if let Some(name) = file_name.strip_suffix(".component.wasm") {
        let manifest_path = manifest_dir.join(format!("{name}.manifest.json"));
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(manifest_path).ok()?).ok()?;
        if manifest["name"].as_str() != Some(name)
            || manifest["runtime"].as_str() == Some("js-wasm")
        {
            return None;
        }
        return Some(name.to_string());
    }
    if let Some(name) = file_name.strip_suffix(".manifest.json") {
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if manifest["name"].as_str() != Some(name)
            || manifest["runtime"].as_str() == Some("js-wasm")
        {
            return None;
        }
        return Some(name.to_string());
    }
    None
}

fn ui_plugin_name_for_path(path: &Path, plugins_dir: &Path) -> Option<String> {
    let rel = path.strip_prefix(plugins_dir).ok()?;
    let mut comps = rel.components();
    let name = comps.next()?.as_os_str().to_str()?.to_string();
    let rest: PathBuf = comps.collect();
    if rest == Path::new("index.html") || rest.starts_with("dist") {
        Some(name)
    } else {
        None
    }
}

fn rebuild_ui_plugin_from_dist(root: &Path, plugin_name: &str, manifest_dir: &Path) {
    let config_path = root.join("wasm-app.toml");
    let Ok(raw) = std::fs::read_to_string(&config_path) else {
        return;
    };
    let Ok(cfg) = toml::from_str::<AppConfig>(&raw) else {
        return;
    };
    let plugins = cfg.plugins.clone().unwrap_or_default();
    let Some(plugin) = plugins.get(plugin_name) else {
        return;
    };
    if plugin.runtime.as_deref() != Some("js-wasm") {
        return;
    }
    let app_version = cfg
        .app
        .as_ref()
        .and_then(|a| a.version.clone())
        .unwrap_or_else(|| "0.1.0".to_string());
    let build_id =
        build_id::resolve_from_manifests(manifest_dir).unwrap_or_else(build_id::compute_build_id);
    if std::env::var("OWASIDEA_BUILD_ID").is_err() {
        std::env::set_var("OWASIDEA_BUILD_ID", &build_id);
    }
    match build_ui_plugin(
        root,
        plugin_name,
        plugin,
        &app_version,
        manifest_dir,
        &build_id,
    ) {
        Ok(_) => {
            let mut ui_index = Vec::new();
            let mut ordered: Vec<(&String, &PluginSection)> = plugins.iter().collect();
            ordered.sort_by_key(|(name, _)| (*name).clone());
            for (name, p) in ordered {
                if p.runtime.as_deref() != Some("js-wasm") {
                    continue;
                }
                let mp = manifest_dir.join(format!("{name}.manifest.json"));
                if let Ok(raw) = std::fs::read_to_string(&mp) {
                    if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) {
                        ui_index.push(ui_index_entry(name, &manifest));
                    }
                }
            }
            let ui_dir = root.join("ui");
            let index_path = ui_dir.join("public/ui-plugins.json");
            if ui_dir.join("public").is_dir() {
                let _ = std::fs::write(
                    &index_path,
                    serde_json::to_string_pretty(&ui_index).unwrap_or_default() + "\n",
                );
                println!("[dev] regenerated UI index: {}", index_path.display());
            }
        }
        Err(error) => eprintln!("[dev] UI plugin rebuild {plugin_name} failed: {error}"),
    }
}

async fn cmd_run(manifest_dir: &Path, http_addr: &str) -> anyhow::Result<()> {
    build_id::ensure_env_from_manifests(manifest_dir);
    owasidea_host::run(manifest_dir.to_path_buf(), http_addr.to_string()).await
}

fn cmd_inspect(manifest_dir: &Path, plugin: &str, json: bool) -> anyhow::Result<()> {
    let candidates = [
        manifest_dir.join(format!("{}.manifest.json", plugin)),
        manifest_dir.join(format!("{}.json", plugin)),
    ];
    let found = candidates.iter().find(|c| c.exists()).cloned();
    let Some(found) = found else {
        anyhow::bail!("manifest not found for {}", plugin);
    };
    let raw = std::fs::read_to_string(&found)?;
    if json {
        println!("{}", raw);
    } else {
        let v: serde_json::Value = serde_json::from_str(&raw)?;
        println!("{}", serde_json::to_string_pretty(&v)?);
        // Show WIT if component exists
        let comp = found
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(v["component"].as_str().unwrap_or(""));
        if comp.exists() {
            println!("\n--- WIT ---");
            let status = std::process::Command::new("wasm-tools")
                .args(["component", "wit"])
                .arg(&comp)
                .status();
            if status.is_ok() {
                let out = std::process::Command::new("wasm-tools")
                    .args(["component", "wit"])
                    .arg(&comp)
                    .output()?;
                print!("{}", String::from_utf8_lossy(&out.stdout));
            }
        }
    }
    Ok(())
}

fn cmd_validate(config: &Path, manifest_dir: &Path) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let root = app_root(config);
    let raw = std::fs::read_to_string(config)?;
    let cfg: AppConfig = toml::from_str(&raw)?;
    cfg.validate()?;
    let plugins = cfg.plugins.clone().unwrap_or_default();
    let manifest_dir = app_path(&root, manifest_dir);
    let mut errors = 0;
    for (name, p) in &plugins {
        let manifest_path = manifest_dir.join(format!("{}.manifest.json", name));
        if !manifest_path.exists() {
            println!("MISSING manifest: {}", manifest_path.display());
            errors += 1;
            continue;
        }
        let mraw = std::fs::read_to_string(&manifest_path)?;
        let manifest: serde_json::Value = serde_json::from_str(&mraw)?;
        if p.runtime.as_deref() == Some("js-wasm") {
            if manifest["runtime"].as_str() != Some("js-wasm")
                || manifest["type"].as_str() != Some("ui")
            {
                println!("{}: UI manifest runtime/type mismatch", name);
                errors += 1;
            } else {
                println!("{}: UI manifest ok", name);
            }
            continue;
        }
        // checksum
        let comp = app_path(&root, &p.path).join(format!("{}.component.wasm", name));
        if comp.exists() {
            let bytes = std::fs::read(&comp)?;
            let mut h = Sha256::new();
            h.update(&bytes);
            let actual = format!("{:x}", h.finalize());
            let expected = manifest["metadata"]["sha256"].as_str().unwrap_or("");
            if expected.is_empty() {
                println!("{}: no checksum in manifest", name);
            } else if expected != actual {
                println!("{}: CHECKSUM MISMATCH", name);
                errors += 1;
            } else {
                println!("{}: checksum ok", name);
            }
        } else {
            println!("{}: component not found", name);
            errors += 1;
        }
        // capabilities: fs count
        let mfs = manifest["permissions"]["filesystem"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        if mfs != p.fs.len() {
            println!(
                "{}: fs permission mismatch (manifest {}, config {})",
                name,
                mfs,
                p.fs.len()
            );
            errors += 1;
        }
        // kv
        let mkv = manifest["permissions"]["kv"].is_null();
        if p.kv.is_some() != !mkv {
            println!("{}: kv permission mismatch", name);
            errors += 1;
        }
        // http outbound
        let mout = manifest["permissions"]["http"]["outbound"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        if mout != p.http_outbound.len() {
            println!("{}: http outbound mismatch", name);
            errors += 1;
        }
    }
    if errors > 0 {
        anyhow::bail!("validation failed with {} error(s)", errors);
    }
    println!("Validation OK.");
    Ok(())
}

#[derive(serde::Deserialize)]
struct RegistryIndex {
    version: u32,
    plugins: Vec<RegistryPlugin>,
}

#[derive(serde::Deserialize)]
struct RegistryPlugin {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    manifest: String,
    #[serde(default)]
    artifact: String,
    #[serde(default)]
    assets: Vec<RegistryAsset>,
}

#[derive(serde::Deserialize)]
struct RegistryAsset {
    path: String,
    url: String,
}

fn registry_url(raw: &str) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)?;
    let local_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !local_http {
        anyhow::bail!("registry must use HTTPS (HTTP is allowed only for localhost tests)");
    }
    if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
        anyhow::bail!("registry URL must not contain credentials or a fragment");
    }
    Ok(url)
}

async fn fetch_registry_index(url: &str) -> anyhow::Result<(reqwest::Url, RegistryIndex)> {
    let index_url = registry_url(url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let response = client
        .get(index_url.clone())
        .send()
        .await?
        .error_for_status()?;
    let bytes = read_limited(response, 4 * 1024 * 1024).await?;
    let index: RegistryIndex = serde_json::from_slice(&bytes)?;
    if index.version != 1 {
        anyhow::bail!("unsupported registry index version {}", index.version);
    }
    Ok((index_url, index))
}

async fn cmd_registry_search(query: &str, url: &str, json: bool) -> anyhow::Result<()> {
    let (_, index) = fetch_registry_index(url).await?;
    let query = query.to_ascii_lowercase();
    let found: Vec<_> = index
        .plugins
        .into_iter()
        .filter(|plugin| {
            plugin.name.to_ascii_lowercase().contains(&query)
                || plugin.description.to_ascii_lowercase().contains(&query)
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&found.iter().map(|plugin| {
            serde_json::json!({"name": plugin.name, "version": plugin.version, "description": plugin.description})
        }).collect::<Vec<_>>())?);
    } else if found.is_empty() {
        println!("No plugins found.");
    } else {
        for plugin in found {
            println!("{}@{}  {}", plugin.name, plugin.version, plugin.description);
        }
    }
    Ok(())
}

async fn cmd_registry_install(
    name: &str,
    version: &str,
    registry: &str,
    public_key: &Path,
    manifest_dir: &Path,
    force: bool,
    json: bool,
) -> anyhow::Result<()> {
    validate_plugin_name(name)?;
    let (index_url, index) = fetch_registry_index(registry).await?;
    let entry = index
        .plugins
        .into_iter()
        .find(|plugin| plugin.name == name && plugin.version == version)
        .ok_or_else(|| anyhow::anyhow!("registry does not contain {name}@{version}"))?;
    let manifest_url = index_url.join(&entry.manifest)?;
    let artifact_url = index_url.join(&entry.artifact)?;
    if manifest_url.origin() != index_url.origin() || artifact_url.origin() != index_url.origin() {
        anyhow::bail!("registry manifest and artifact must stay on registry origin");
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let manifest_response = client.get(manifest_url).send().await?.error_for_status()?;
    let manifest_bytes = read_limited(manifest_response, 1024 * 1024).await?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    if manifest["name"].as_str() != Some(name) || manifest["version"].as_str() != Some(version) {
        anyhow::bail!("registry index and plugin manifest identity do not match");
    }
    let is_ui = manifest["runtime"].as_str() == Some("js-wasm");
    let mut ui_assets = std::collections::BTreeMap::new();
    let artifact = if is_ui {
        if !entry.artifact.is_empty() {
            anyhow::bail!("UI registry entries must use the assets list, not artifact");
        }
        let expected_files = manifest["metadata"]["ui"]["files"]
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("UI manifest has no asset digest map"))?;
        if expected_files.len() != entry.assets.len() {
            anyhow::bail!("registry file list does not match signed UI manifest");
        }
        for asset in &entry.assets {
            validate_registry_path(&asset.path)?;
            let expected = expected_files
                .get(&asset.path)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    anyhow::anyhow!("asset {} is not declared by manifest", asset.path)
                })?;
            let asset_url = index_url.join(&asset.url)?;
            if asset_url.origin() != index_url.origin() {
                anyhow::bail!("registry assets must stay on registry origin");
            }
            let response = client.get(asset_url).send().await?.error_for_status()?;
            let bytes = read_limited(response, 64 * 1024 * 1024).await?;
            use sha2::{Digest, Sha256};
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if actual != expected {
                anyhow::bail!("UI asset checksum mismatch: {}", asset.path);
            }
            ui_assets.insert(asset.path.clone(), bytes);
        }
        if ui_assets.len() != expected_files.len() {
            anyhow::bail!("registry file list contains duplicate or missing UI assets");
        }
        let digests = expected_files
            .iter()
            .map(|(path, value)| {
                Ok((
                    path.clone(),
                    value
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("invalid digest for {path}"))?
                        .to_string(),
                ))
            })
            .collect::<anyhow::Result<std::collections::BTreeMap<_, _>>>()?;
        let bundle_digest = digest_ui_file_list(&digests);
        if manifest["metadata"]["ui"]["sha256"].as_str() != Some(bundle_digest.as_str()) {
            anyhow::bail!("UI bundle digest does not match signed manifest");
        }
        Vec::new()
    } else {
        if !entry.assets.is_empty() {
            anyhow::bail!("WASI component registry entries cannot contain UI assets");
        }
        let component_rel = manifest["component"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("manifest has no component path"))?;
        validate_registry_path(component_rel)?;
        if artifact_url.origin() != index_url.origin() {
            anyhow::bail!("registry artifact must stay on registry origin");
        }
        let response = client.get(artifact_url).send().await?.error_for_status()?;
        let artifact = read_limited(response, 512 * 1024 * 1024).await?;
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(&artifact));
        if manifest["metadata"]["sha256"].as_str() != Some(digest.as_str()) {
            anyhow::bail!("plugin artifact checksum mismatch");
        }
        artifact.to_vec()
    };
    let signature = manifest["metadata"]["signature"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("registry plugin is unsigned"))?;
    if manifest["metadata"]["signature_format"].as_str() != Some("owasidea-manifest-v1") {
        anyhow::bail!("unsupported plugin signature format");
    }
    let key_b64 = std::fs::read_to_string(public_key)?;
    let key_bytes = base64_decode(key_b64.trim())?;
    let signature_bytes = base64_decode(signature)?;
    let signed_manifest = manifest.clone();
    let payload = cli_signature_payload(signed_manifest, &artifact)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key_bytes)
        .verify(&payload, &signature_bytes)
        .map_err(|_| anyhow::anyhow!("plugin signature verification failed"))?;

    std::fs::create_dir_all(manifest_dir)?;
    let manifest_root = std::fs::canonicalize(manifest_dir)?;
    let manifest_path = manifest_root.join(format!("{name}.manifest.json"));
    let component_path = if is_ui {
        None
    } else {
        let component_rel = manifest["component"].as_str().unwrap();
        let path = manifest_root.join(component_rel);
        if path == manifest_path {
            anyhow::bail!("component and manifest paths collide");
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let parent = std::fs::canonicalize(path.parent().unwrap())?;
        if !parent.starts_with(&manifest_root) {
            anyhow::bail!("component path resolves outside manifest directory");
        }
        Some(path)
    };
    let ui_root = if is_ui {
        let project_root = std::fs::canonicalize(".")?;
        let base = Path::new("ui/public/plugins");
        std::fs::create_dir_all(base)?;
        let base = std::fs::canonicalize(base)?;
        if !base.starts_with(&project_root) {
            anyhow::bail!("UI plugin root resolves outside the project directory");
        }
        let root = base.join(name);
        if std::fs::symlink_metadata(&root).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            anyhow::bail!("UI plugin asset directory must not be a symlink");
        }
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(root)?;
        if !root.starts_with(&base) {
            anyhow::bail!("UI plugin directory resolves outside static asset root");
        }
        Some(root)
    } else {
        None
    };
    if !force
        && (manifest_path.exists()
            || component_path.as_ref().is_some_and(|path| path.exists())
            || ui_root
                .as_ref()
                .is_some_and(|root| root.read_dir().is_ok_and(|mut d| d.next().is_some())))
    {
        anyhow::bail!("plugin files already exist; pass --force to replace them");
    }
    let previous_component = if force {
        component_path
            .as_ref()
            .filter(|path| path.is_file())
            .map(std::fs::read)
            .transpose()?
    } else {
        None
    };
    let suffix = format!("tmp-{}", std::process::id());
    let manifest_tmp = manifest_path.with_extension(suffix);
    let component_tmp = component_path
        .as_ref()
        .map(|path| path.with_extension(format!("tmp-{}", std::process::id())));
    if let Some(tmp) = &component_tmp {
        std::fs::write(tmp, &artifact)?;
    }
    std::fs::write(&manifest_tmp, serde_json::to_vec_pretty(&manifest)?)?;
    if let (Some(path), Some(tmp)) = (&component_path, &component_tmp) {
        if let Err(error) = std::fs::rename(tmp, path) {
            let _ = std::fs::remove_file(tmp);
            let _ = std::fs::remove_file(&manifest_tmp);
            return Err(error.into());
        }
    }
    if let Some(root) = &ui_root {
        for (relative, bytes) in &ui_assets {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let parent = std::fs::canonicalize(path.parent().unwrap())?;
            if !parent.starts_with(root) {
                anyhow::bail!("UI asset path resolves outside plugin asset directory");
            }
            if std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                anyhow::bail!("refusing to replace symlink UI asset: {}", path.display());
            }
            std::fs::write(path, bytes)?;
        }
    }
    if let Err(error) = std::fs::rename(&manifest_tmp, &manifest_path) {
        if let (Some(path), Some(tmp)) = (&component_path, &component_tmp) {
            if let Some(previous) = previous_component {
                let _ = std::fs::write(path, previous);
            } else {
                let _ = std::fs::remove_file(path);
            }
            let _ = std::fs::remove_file(tmp);
        }
        let _ = std::fs::remove_file(&manifest_tmp);
        return Err(error.into());
    }
    if json {
        println!(
            "{}",
            serde_json::json!({"installed": name, "version": version})
        );
    } else {
        println!("installed {name}@{version} into {}", manifest_dir.display());
    }
    Ok(())
}

async fn read_limited(mut response: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        anyhow::bail!("remote response exceeds {} bytes", limit);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("remote response exceeds {} bytes", limit);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_registry_path(value: &str) -> anyhow::Result<()> {
    validate_ui_entry(value)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
    {
        anyhow::bail!("unsupported character in registry artifact path {value:?}");
    }
    Ok(())
}

fn digest_ui_file_list(files: &std::collections::BTreeMap<String, String>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (path, digest) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn cli_signature_payload(
    mut manifest: serde_json::Value,
    component: &[u8],
) -> anyhow::Result<Vec<u8>> {
    manifest["metadata"]
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("manifest metadata must be an object"))?
        .remove("signature");
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let mut payload = b"owasidea-plugin-signature-v1\0".to_vec();
    payload.extend_from_slice(&manifest_bytes);
    payload.push(0);
    payload.extend_from_slice(component);
    Ok(payload)
}

fn isolated_bundle_command(
    workdir: &Path,
    extra_masks: &[PathBuf],
    program: &str,
    args: &[std::ffi::OsString],
) -> anyhow::Result<std::process::Command> {
    if !cfg!(target_os = "linux") {
        println!("note: --isolate-host-libs only applies to Linux bundlers; ignoring it");
        let mut command = external_command(program);
        command.args(args);
        return Ok(command);
    }
    let mut masks = Vec::new();
    for candidate in ["/usr/lib/vmware", "/usr/lib64/vmware"] {
        let path = PathBuf::from(candidate);
        if path.is_dir() {
            // /usr/lib64 is a symlink to /usr/lib on several distributions;
            // canonicalize so the same directory is masked only once.
            let path = path.canonicalize().unwrap_or(path);
            if !masks.contains(&path) {
                masks.push(path);
            }
        }
    }
    for path in extra_masks {
        if !path.is_dir() {
            anyhow::bail!("--mask-path is not a directory: {}", path.display());
        }
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !masks.contains(&path) {
            masks.push(path);
        }
    }
    let bwrap = tool_path("bwrap").ok_or_else(|| {
        anyhow::anyhow!(
            "--isolate-host-libs requires bubblewrap (bwrap); install it (for example `pacman -S bubblewrap` or `apt install bubblewrap`) or use the chroot recipe in docs/CLEAN_BUILD.md"
        )
    })?;
    if masks.is_empty() {
        println!("note: no vendor library directory found to hide; running the bundler normally");
        let mut command = external_command(program);
        command.args(args);
        return Ok(command);
    }
    let mut command = std::process::Command::new(bwrap);
    // Full host access, like running without bwrap; only the vendor library
    // directories are replaced with empty tmpfs mounts.
    command.args(["--dev-bind", "/", "/"]);
    for mask in &masks {
        println!("Hiding {} from the bundler", mask.display());
        command.arg("--tmpfs").arg(mask);
    }
    command.arg("--chdir").arg(workdir.canonicalize()?);
    command.arg("--").arg(program).args(args);
    Ok(command)
}

fn cmd_bundle(
    config: &Path,
    manifest_dir: &Path,
    build: bool,
    bundles: &[String],
    isolate_host_libs: bool,
    mask_paths: &[PathBuf],
) -> anyhow::Result<()> {
    let root = app_root(config);
    if build {
        cmd_build(config, manifest_dir, true)?;
    }
    let cfg = read_app_config(config)?;
    cfg.validate_for_bundle()?;
    if !build {
        sign_all(&root, &app_path(&root, manifest_dir), cfg.signing.as_ref())?;
    }
    let desktop = cfg.desktop.unwrap_or_default();
    let ui_dir = if let Some(ui) = desktop.ui.as_deref() {
        crate::config_render::safe_resolve(&root, ui)?
    } else {
        root.join("ui")
    };

    let desktop_dir = match desktop.path {
        Some(path) => crate::config_render::safe_resolve(&root, &path)?,
        None if root.join("desktop/Cargo.toml").is_file() => root.join("desktop"),
        None => root.join("crates/desktop"),
    };
    if !ui_dir.join("package.json").is_file() {
        anyhow::bail!("UI package.json not found in {}", ui_dir.display());
    }
    if !desktop_dir.join("tauri.conf.json").is_file() {
        anyhow::bail!(
            "Tauri config not found in {}; add [desktop] path to {} or scaffold a desktop shell",
            desktop_dir.display(),
            config.display()
        );
    }
    if !root.join("manifests").is_dir() {
        println!(
            "warning: manifests directory is missing under {}; run `owasidea build` first",
            root.display()
        );
    }

    ensure_npm_dependencies(&ui_dir)?;
    // The dashboard embeds the host origin at build time (Vite `define`).
    // Use the same source of truth as `dev`/`run` instead of the framework
    // default, otherwise changing [host].addr silently breaks the bundle.
    let host_url = format!(
        "http://{}",
        read_host_addr(config).unwrap_or_else(|| "127.0.0.1:8787".to_string())
    );
    // Render the generated Tauri config from [host].addr so the bundle uses
    // the generated CSP (RF-05). Fail loudly if it cannot be created.
    let generated_config = crate::config_render::render_targets(
        config,
        crate::config_render::RenderScope::Single(crate::config_render::RenderTarget::Tauri),
        false,
    )
    .map_err(|e| anyhow::anyhow!("failed to render tauri.generated.conf.json: {e}"))?
    .into_iter()
    .next()
    .ok_or_else(|| anyhow::anyhow!("config render did not produce tauri.generated.conf.json"))?;
    if !generated_config.is_file() {
        anyhow::bail!(
            "generated Tauri config not found at {}",
            generated_config.display()
        );
    }
    let generated_raw = std::fs::read_to_string(&generated_config)?;
    let generated_value: serde_json::Value = serde_json::from_str(&generated_raw)?;
    let generated_csp = generated_value
        .get("app")
        .and_then(|a| a.get("security"))
        .and_then(|s| s.get("csp"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    if !crate::config_render::csp_allows_origin(generated_csp, &host_url) {
        anyhow::bail!(
            "generated Tauri config CSP does not allow host origin {}; refusing to bundle",
            host_url
        );
    }
    if !crate::config_render::csp_is_strict(generated_csp) {
        anyhow::bail!(
            "generated Tauri config CSP is not strict (contains * or null or wildcard port)"
        );
    }
    if crate::config_render::csp_connect_src_count(generated_csp) != 1 {
        anyhow::bail!("generated Tauri config CSP must contain exactly one connect-src directive");
    }
    println!("Building UI in {}...", ui_dir.display());
    let status = external_command("npm")
        .args(["run", "build"])
        .env("OWASIDEA_HOST", &host_url)
        .current_dir(&ui_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("UI build failed");
    }
    println!("Building Tauri bundle in {}...", desktop_dir.display());
    let requested = bundles
        .iter()
        .map(|bundle| bundle.trim())
        .filter(|bundle| !bundle.is_empty() && *bundle != "all")
        .collect::<Vec<_>>();
    if !requested.is_empty() {
        println!("Bundle targets: {}", requested.join(","));
    } else {
        println!("Bundle targets: all (from config)");
    }
    let appimage =
        cfg!(target_os = "linux") && (requested.is_empty() || requested.contains(&"appimage"));
    let mut tauri_args = vec![
        std::ffi::OsString::from("tauri"),
        std::ffi::OsString::from("build"),
        std::ffi::OsString::from("--config"),
        generated_config.as_os_str().to_os_string(),
    ];
    if !requested.is_empty() {
        tauri_args.push(std::ffi::OsString::from("--bundles"));
        tauri_args.push(std::ffi::OsString::from(requested.join(",")));
    }
    let mut tauri = if isolate_host_libs {
        isolated_bundle_command(&desktop_dir, mask_paths, "cargo", &tauri_args)?
    } else {
        let mut command = external_command("cargo");
        command.args(&tauri_args);
        command
    };
    if appimage {
        // linuxdeploy is itself an AppImage; extraction avoids a FUSE
        // dependency. Disabling strip avoids failures on hosts with a
        // partially broken ELF toolchain.
        tauri
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .env("NO_STRIP", "1");
    }
    let status = tauri.current_dir(&desktop_dir).status()?;
    let tauri_ok = status.success();
    if !tauri_ok {
        eprintln!(
            "Tauri reported a bundle failure; collecting artifacts that were generated before the failure..."
        );
        if !isolate_host_libs && cfg!(target_os = "linux") && Path::new("/usr/lib/vmware").is_dir()
        {
            eprintln!(
                "hint: /usr/lib/vmware exists on this host; if linuxdeploy failed, rerun with --isolate-host-libs"
            );
        }
    }
    let metadata = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&desktop_dir)
        .output()?;
    let meta: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    let target = PathBuf::from(meta["target_directory"].as_str().unwrap_or(""));
    let bundle_dir = target.join("release/bundle");
    let direct_resources = target.join("release/resources");
    copy_runtime_resources(&root, &direct_resources, cfg.signing.as_ref())?;
    println!("Direct-run resources: {}", direct_resources.display());
    if bundle_dir.is_dir() {
        if let Some(output) = desktop.output {
            let output_dir = app_path(&root, output);
            clear_bundle_artifacts(&output_dir)?;
            copy_selected_bundles(&bundle_dir, &output_dir, &requested)?;
            println!("Artifacts copied to {}", output_dir.display());
        } else {
            println!("Artifacts are under {}", bundle_dir.display());
        }
    } else {
        println!(
            "No bundle artifacts were produced under {}",
            bundle_dir.display()
        );
    }
    if tauri_ok {
        println!("Bundle complete.");
        Ok(())
    } else {
        anyhow::bail!(
            "bundle completed with failed target(s); successful artifacts are still available under dist/"
        )
    }
}

fn copy_runtime_resources(
    root: &Path,
    destination: &Path,
    signing: Option<&SigningSection>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    let manifests = root.join("manifests");
    if manifests.is_dir() {
        copy_dir_all(&manifests, &destination.join("manifests"))?;
    }
    let runtime_src = root.join("ui").join(".owasidea").join("runtime.json");
    if runtime_src.is_file() {
        std::fs::copy(&runtime_src, destination.join("owasidea.runtime.json"))?;
    }
    let plugins = root.join("plugins");
    if plugins.is_dir() {
        for entry in std::fs::read_dir(&plugins)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let component = path.join(format!("{name}.component.wasm"));
            if component.is_file() {
                let target = destination.join("plugins").join(name);
                std::fs::create_dir_all(&target)?;
                std::fs::copy(&component, target.join(component.file_name().unwrap()))?;
            }
        }
    }
    let public_key = signing
        .and_then(|signing| signing.public_key.as_deref())
        .map(|path| app_path(root, path))
        .unwrap_or_else(|| root.join("keys/owasidea.pub"));
    if public_key.is_file() {
        let trust = destination.join("trust");
        std::fs::create_dir_all(&trust)?;
        std::fs::copy(&public_key, trust.join("owasidea.pub"))?;
    }
    Ok(())
}

fn copy_selected_bundles(
    bundle_dir: &Path,
    destination: &Path,
    targets: &[&str],
) -> anyhow::Result<()> {
    if targets.is_empty() {
        for entry in std::fs::read_dir(bundle_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                copy_tree_files(&entry.path(), destination)?;
            }
        }
        return Ok(());
    }
    for target in targets {
        let folder = match *target {
            "app" => "macos",
            other => other,
        };
        let source = bundle_dir.join(folder);
        if source.is_dir() {
            copy_tree_files(&source, destination)?;
        } else {
            println!("  (no {} artifact at {})", target, source.display());
        }
    }
    Ok(())
}

fn clear_bundle_artifacts(destination: &Path) -> anyhow::Result<()> {
    if !destination.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(destination)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            extension.as_str(),
            "deb" | "rpm" | "appimage" | "msi" | "dmg" | "zip" | "exe"
        ) {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn copy_tree_files(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            copy_tree_files(&path, destination)?;
        } else if path.is_file() {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(
                extension.as_str(),
                "deb" | "rpm" | "appimage" | "msi" | "dmg" | "zip" | "exe"
            ) {
                continue;
            }
            let target = destination.join(entry.file_name());
            std::fs::copy(&path, &target)?;
            println!("  {}", target.display());
        }
    }
    Ok(())
}

struct TestHost {
    child: std::process::Child,
    data_dir: PathBuf,
}

impl Drop for TestHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

async fn wait_for_host(client: &reqwest::Client, base: &str) -> anyhow::Result<()> {
    for _ in 0..120 {
        if let Ok(response) = client.get(format!("{base}/health")).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    anyhow::bail!("host did not become ready at {base}")
}

fn check(ok: bool, label: impl std::fmt::Display) -> bool {
    if ok {
        println!("  ok   {label}");
    } else {
        println!("  FAIL {label}");
    }
    ok
}

async fn run_api_smoke(
    config_path: &Path,
    manifest_dir: &Path,
    cfg: &AppConfig,
    json: bool,
) -> anyhow::Result<usize> {
    let root = app_root(config_path);
    let manifest_abs = std::fs::canonicalize(manifest_dir)
        .map_err(|_| anyhow::anyhow!("manifests not found at {}", manifest_dir.display()))?;
    let config_abs = std::fs::canonicalize(config_path)?;
    if std::fs::read_dir(&manifest_abs)?.next().is_none() {
        anyhow::bail!(
            "no manifests found at {}; run `owasidea build` first",
            manifest_abs.display()
        );
    }

    let port = reserve_http_addr("127.0.0.1:0")?;
    let base = format!("http://{port}");
    let data_dir = std::env::temp_dir().join(format!("owasidea-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let log_path = data_dir.join("host.log");
    let log = std::fs::File::create(&log_path)?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("--config")
        .arg(&config_abs)
        .arg("--manifest-dir")
        .arg(&manifest_abs)
        .arg("--http-addr")
        .arg(&port)
        .arg("run")
        .current_dir(&root)
        .env("OWASIDEA_DATA_DIR", &data_dir)
        .stdout(std::process::Stdio::from(log.try_clone()?))
        .stderr(std::process::Stdio::from(log));
    apply_signing_env_to(&mut command, cfg, &root);
    let child = command.spawn()?;
    let host = TestHost { child, data_dir };
    let client = reqwest::Client::new();
    if let Err(error) = wait_for_host(&client, &base).await {
        eprintln!(
            "host log:\n{}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );
        return Err(error);
    }

    let mut failures = 0;
    println!("smoke: host ready at {base}");

    let health = client.get(format!("{base}/health")).send().await?;
    let health_ok = health.status().is_success() && health.text().await? == "ok";
    failures += usize::from(!check(health_ok, "GET /health"));

    let plugins = client
        .get(format!("{base}/plugins"))
        .send()
        .await?
        .json::<Vec<serde_json::Value>>()
        .await?;
    let plugin_names = plugins
        .iter()
        .filter_map(|plugin| plugin["name"].as_str())
        .collect::<std::collections::HashSet<_>>();
    let configured = cfg.plugins.clone().unwrap_or_default();
    for (name, plugin) in &configured {
        if plugin.runtime.as_deref() == Some("js-wasm") {
            continue;
        }
        failures += usize::from(!check(
            plugin_names.contains(name.as_str()),
            format!("plugin {name} loaded"),
        ));
    }

    let ui_plugins = client
        .get(format!("{base}/ui-plugins"))
        .send()
        .await?
        .json::<Vec<serde_json::Value>>()
        .await?;
    let expected_ui = configured
        .values()
        .filter(|plugin| plugin.runtime.as_deref() == Some("js-wasm"))
        .count();
    failures += usize::from(!check(
        ui_plugins.len() == expected_ui,
        format!("UI plugins: {} (expected {expected_ui})", ui_plugins.len()),
    ));

    let smoke = cfg.smoke.clone().unwrap_or_default();
    for call in &smoke.calls {
        let response = client
            .post(format!("{base}/plugin/{}/call", call.plugin))
            .json(&serde_json::json!({"function": call.function, "args": call.args}))
            .send()
            .await;
        let label = format!("{}.{}", call.plugin, call.function);
        let Ok(response) = response else {
            failures += usize::from(!check(false, label));
            continue;
        };
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let value: serde_json::Value =
            serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body.clone()));
        let result = value
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let is_error = result.get("err").is_some();
        let rendered = match &result {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let mut ok = status.is_success() && !is_error;
        if let Some(expected) = &call.expect_contains {
            ok &= rendered.contains(expected);
        }
        if let Some(expected) = &call.expect_equals {
            ok &= &result == expected;
        }
        if let Some(min) = call.min {
            ok &= result.as_f64().is_some_and(|value| value >= min);
        }
        if let Some(max) = call.max {
            ok &= result.as_f64().is_some_and(|value| value <= max);
        }
        failures += usize::from(!check(ok, format!("{label} -> {rendered}")));
    }

    for http in &smoke.http {
        let method = http.method.as_deref().unwrap_or("GET");
        let mut request = client.request(
            reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET),
            format!("{base}{}", http.path),
        );
        if let Some(body) = &http.body {
            request = request.json(body);
        }
        let response = request.send().await;
        let label = format!("{method} {}", http.path);
        let Ok(response) = response else {
            failures += usize::from(!check(false, label));
            continue;
        };
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let mut ok = http.expect_status.map_or_else(
            || status.is_success(),
            |expected| status.as_u16() == expected,
        );
        if let Some(expected) = &http.expect_contains {
            ok &= body.contains(expected);
        }
        failures += usize::from(!check(ok, format!("{label} -> {} {body}", status.as_u16())));
    }

    for service in &smoke.services {
        let plugin = &service.plugin;
        let started = client
            .post(format!("{base}/plugin/{plugin}/service/start"))
            .json(&serde_json::json!({"region_json": ""}))
            .send()
            .await;
        let start_ok = started
            .as_ref()
            .is_ok_and(|response| response.status().is_success());
        failures += usize::from(!check(start_ok, format!("{plugin} service start")));
        let status = client
            .get(format!("{base}/plugin/{plugin}/service/status"))
            .send()
            .await;
        let running = status
            .as_ref()
            .ok()
            .is_some_and(|response| response.status().is_success());
        let running = if running {
            status
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .ok()
                .is_some_and(|value| value["running"] == true)
        } else {
            false
        };
        failures += usize::from(!check(running, format!("{plugin} service status")));
        let stopped = client
            .post(format!("{base}/plugin/{plugin}/service/stop"))
            .send()
            .await;
        failures += usize::from(!check(
            stopped
                .as_ref()
                .is_ok_and(|response| response.status().is_success()),
            format!("{plugin} service stop"),
        ));
    }

    drop(host);
    if json {
        println!(
            "{}",
            serde_json::json!({"smoke": failures == 0, "failures": failures})
        );
    }
    Ok(failures)
}

fn apply_signing_env_to(command: &mut std::process::Command, cfg: &AppConfig, root: &Path) {
    let Some(signing) = &cfg.signing else {
        return;
    };
    let public_key = app_path(
        root,
        signing.public_key.as_deref().unwrap_or("keys/owasidea.pub"),
    );
    if public_key.is_file() {
        command.env("OWASIDEA_PUBLIC_KEY", public_key);
    }
    command.env(
        "OWASIDEA_REQUIRE_SIGNATURES",
        if signing.require.unwrap_or(true) {
            "true"
        } else {
            "false"
        },
    );
}

async fn cmd_test(
    config_path: &Path,
    manifest_dir: &Path,
    build: bool,
    smoke: bool,
    ui: bool,
    json: bool,
) -> anyhow::Result<()> {
    let root = app_root(config_path);
    if build {
        cmd_build(config_path, manifest_dir, true)?;
    }
    let cfg = read_app_config(config_path)?;
    let manifest_dir = app_path(&root, manifest_dir);
    if !build {
        sign_all(&root, &manifest_dir, cfg.signing.as_ref())?;
    }
    let mut failures = 0usize;
    if smoke {
        failures += run_api_smoke(config_path, &manifest_dir, &cfg, json).await?;
    }
    if ui {
        let command = cfg
            .test
            .as_ref()
            .map(|test| test.ui.clone())
            .unwrap_or_default();
        if command.is_empty() {
            println!("ui tests: no [test].ui command configured; skipped");
        } else {
            println!("ui tests: {}", command.join(" "));
            let mut child = external_command(&command[0]);
            child.args(&command[1..]).current_dir(&root);
            if let Ok(exe) = std::env::current_exe() {
                child.env("OWASIDEA_BIN", exe);
            }
            apply_signing_env_to(&mut child, &cfg, &root);
            let status = child.status()?;
            if !status.success() {
                failures += 1;
                println!("  FAIL ui test command");
            }
        }
    }
    if failures > 0 {
        anyhow::bail!("{failures} test failure(s)");
    }
    println!("tests passed");
    Ok(())
}

#[derive(Clone)]
struct ExportSig {
    name: String,
    params: String,
    ret: String,
    is_async: bool,
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_rs_files(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
}

fn collect_cs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_cs_files(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("cs") {
                out.push(p);
            }
        }
    }
}

fn csharp_type_to_wit(ty: &str) -> String {
    let mut t = ty.trim();
    t = t.trim_end_matches('?');
    if t.ends_with("[]") {
        let inner = t.trim_end_matches("[]");
        return format!("list<{}>", csharp_type_to_wit(inner));
    }
    match t {
        "string" => "string".to_string(),
        "int" => "s32".to_string(),
        "long" => "s64".to_string(),
        "short" => "s16".to_string(),
        "byte" => "u8".to_string(),
        "sbyte" => "s8".to_string(),
        "uint" => "u32".to_string(),
        "ulong" => "u64".to_string(),
        "ushort" => "u16".to_string(),
        "float" => "f32".to_string(),
        "double" => "f64".to_string(),
        "bool" => "bool".to_string(),
        "char" => "char".to_string(),
        "void" => String::new(),
        "Task" => String::new(),
        _ => {
            if let Some(inner) = t.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
                return format!("list<{}>", csharp_type_to_wit(inner.trim()));
            }
            if let Some(inner) = t
                .strip_prefix("IEnumerable<")
                .and_then(|s| s.strip_suffix('>'))
            {
                return format!("list<{}>", csharp_type_to_wit(inner.trim()));
            }
            if let Some(inner) = t.strip_prefix("Task<").and_then(|s| s.strip_suffix('>')) {
                return csharp_type_to_wit(inner.trim());
            }
            t.to_lowercase()
        }
    }
}

fn parse_csharp_params(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for part in raw.split(',') {
        let mut part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((left, _)) = part.split_once('=') {
            part = left.trim();
        }
        let tokens: Vec<&str> = part.split_whitespace().collect();
        if tokens.len() < 2 {
            continue;
        }
        let pname = tokens[tokens.len() - 1];
        let ty = tokens[..tokens.len() - 1].join(" ");
        let wit_ty = csharp_type_to_wit(&ty);
        out.push(format!("{}: {}", pname, wit_ty));
    }
    out.join(", ")
}

fn collect_ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_ts_files(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("ts") {
                out.push(p);
            }
        }
    }
}

fn collect_go_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_go_files(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("go") {
                out.push(p);
            }
        }
    }
}

fn collect_c_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_c_files(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("c") {
                out.push(p);
            }
        }
    }
}

fn collect_cpp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_cpp_files(&p, out);
            } else {
                match p.extension().and_then(|s| s.to_str()) {
                    Some("cpp") | Some("cc") | Some("cxx") | Some("C") => out.push(p),
                    _ => {}
                }
            }
        }
    }
}

fn attr_last(attr: &syn::Attribute) -> Option<String> {
    attr.path().segments.last().map(|s| s.ident.to_string())
}

fn has_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|a| attr_last(a).as_deref() == Some(name))
}

fn parse_world_attr(attr: &syn::Attribute) -> Option<String> {
    attr.parse_args::<syn::LitStr>().ok().map(|l| l.value())
}

fn rust_type_to_wit(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(tp) => {
            let seg = match tp.path.segments.last() {
                Some(s) => s,
                None => return "string".to_string(),
            };
            let ident = seg.ident.to_string();
            match ident.as_str() {
                "String" | "str" => "string".to_string(),
                "i8" => "s8".to_string(),
                "i16" => "s16".to_string(),
                "i32" => "s32".to_string(),
                "i64" => "s64".to_string(),
                "u8" => "u8".to_string(),
                "u16" => "u16".to_string(),
                "u32" => "u32".to_string(),
                "u64" => "u64".to_string(),
                "f32" => "f32".to_string(),
                "f64" => "f64".to_string(),
                "bool" => "bool".to_string(),
                "char" => "char".to_string(),
                "Option" => {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return format!("option<{}>", rust_type_to_wit(inner));
                        }
                    }
                    "option<string>".to_string()
                }
                "Result" => {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        let mut it = args.args.iter();
                        let ok = it
                            .next()
                            .map(type_arg_to_wit)
                            .unwrap_or_else(|| "string".to_string());
                        let err = it
                            .next()
                            .map(type_arg_to_wit)
                            .unwrap_or_else(|| "string".to_string());
                        return format!("result<{}, {}>", ok, err);
                    }
                    "result<string, string>".to_string()
                }
                "Vec" => {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return format!("list<{}>", rust_type_to_wit(inner));
                        }
                    }
                    "list<string>".to_string()
                }
                other => other.to_lowercase(),
            }
        }
        syn::Type::Reference(r) => rust_type_to_wit(&r.elem),
        _ => "string".to_string(),
    }
}

fn type_arg_to_wit(arg: &syn::GenericArgument) -> String {
    match arg {
        syn::GenericArgument::Type(ty) => rust_type_to_wit(ty),
        _ => "string".to_string(),
    }
}

fn parse_fn_sig(sig: &syn::Signature) -> ExportSig {
    let params = sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(t) = arg {
                let pname = match &*t.pat {
                    syn::Pat::Ident(pi) => pi.ident.to_string(),
                    _ => "arg".to_string(),
                };
                let ty = rust_type_to_wit(&t.ty);
                Some(format!("{}: {}", pname, ty))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = match &sig.output {
        syn::ReturnType::Default => String::new(),
        syn::ReturnType::Type(_, ty) => format!(" -> {}", rust_type_to_wit(ty)),
    };
    ExportSig {
        name: sig.ident.to_string(),
        params,
        ret,
        is_async: sig.asyncness.is_some(),
    }
}

fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            '>' | ')' | ']' | '}' => {
                depth = (depth - 1).max(0);
                cur.push(c);
            }
            _ if c == sep && depth == 0 => {
                parts.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

fn unwrap_generic(ty: &str, name: &str) -> Option<String> {
    let ty = ty.trim();
    let prefix = format!("{}<", name);
    if ty.starts_with(&prefix) && ty.ends_with('>') {
        Some(ty[prefix.len()..ty.len() - 1].to_string())
    } else {
        None
    }
}

fn ts_type_to_wit(ty: &str) -> String {
    let ty = ty.trim();
    if ty.is_empty() {
        return String::new();
    }
    if let Some(inner) = unwrap_generic(ty, "Promise") {
        return ts_type_to_wit(&inner);
    }
    if let Some(inner) = unwrap_generic(ty, "Array") {
        return format!("list<{}>", ts_type_to_wit(&inner));
    }
    if ty.ends_with("[]") {
        return format!(
            "list<{}>",
            ts_type_to_wit(ty.strip_suffix("[]").unwrap_or(ty))
        );
    }
    // handle union by taking first alternative
    if ty.contains('|') {
        let first = ty.split('|').next().unwrap_or(ty).trim();
        return ts_type_to_wit(first);
    }
    match ty {
        "string" => "string".to_string(),
        "number" => "f64".to_string(),
        "boolean" => "bool".to_string(),
        "bigint" => "s64".to_string(),
        "void" | "undefined" | "null" => String::new(),
        "any" | "unknown" => "string".to_string(),
        _ => {
            // generic list<T>?
            if ty.starts_with("list<") {
                ty.to_string()
            } else {
                ty.to_lowercase()
            }
        }
    }
}

fn parse_ts_params(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let parts = split_top_level(raw, ',');
    let mut out = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon) = part.find(':') {
            let name = part[..colon].trim().trim_end_matches('?').to_string();
            let ty = part[colon + 1..].trim();
            let wit = ts_type_to_wit(ty);
            if wit.is_empty() {
                out.push(name);
            } else {
                out.push(format!("{}: {}", name, wit));
            }
        }
    }
    out.join(", ")
}

pub fn generate_wit(plugin_dir: &Path) -> anyhow::Result<()> {
    let src_dir = plugin_dir.join("src");
    let mut rs_files = Vec::new();
    collect_rs_files(&src_dir, &mut rs_files);
    if rs_files.is_empty() {
        collect_rs_files(plugin_dir, &mut rs_files);
    }
    let mut ts_files = Vec::new();
    collect_ts_files(&src_dir, &mut ts_files);
    if ts_files.is_empty() {
        collect_ts_files(plugin_dir, &mut ts_files);
    }
    let mut go_files = Vec::new();
    collect_go_files(&src_dir, &mut go_files);
    if go_files.is_empty() {
        collect_go_files(plugin_dir, &mut go_files);
    }
    let mut cs_files = Vec::new();
    collect_cs_files(&src_dir, &mut cs_files);
    if cs_files.is_empty() {
        collect_cs_files(plugin_dir, &mut cs_files);
    }

    let mut c_files = Vec::new();
    collect_c_files(&src_dir, &mut c_files);
    if c_files.is_empty() {
        collect_c_files(plugin_dir, &mut c_files);
    }
    let mut cpp_files = Vec::new();
    collect_cpp_files(&src_dir, &mut cpp_files);
    if cpp_files.is_empty() {
        collect_cpp_files(plugin_dir, &mut cpp_files);
    }

    let mut world_name: Option<String> = None;
    let mut host_log = false;
    let mut host_kv = false;
    let mut exports: Vec<ExportSig> = Vec::new();

    if !ts_files.is_empty() {
        let world_re = regex::Regex::new(r"@owasidea-world\s+([A-Za-z0-9_-]+)").unwrap();
        let func_re = regex::Regex::new(
            r"(?s)/\*\*(.*?)\*/\s*export\s+(async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*(?::\s*([^\{]+))?",
        )
        .unwrap();
        for file in &ts_files {
            let content = std::fs::read_to_string(file)?;
            // world name from any @owasidea-world tag
            for cap in world_re.captures_iter(&content) {
                world_name = Some(cap[1].to_string());
            }
            if content.contains("@owasidea-host-log") {
                host_log = true;
            }
            if content.contains("@owasidea-host-kv") {
                host_kv = true;
            }

            // exported functions preceded by a JSDoc block containing @owasidea-export
            for cap in func_re.captures_iter(&content) {
                let jsdoc = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if !jsdoc.contains("@owasidea-export") {
                    continue;
                }
                let is_async = cap.get(2).is_some();
                let name = cap.get(3).unwrap().as_str().to_string();
                let params_raw = cap.get(4).map(|m| m.as_str()).unwrap_or("");
                let ret_raw = cap.get(5).map(|m| m.as_str()).unwrap_or("");
                let params = parse_ts_params(params_raw);
                let mut ret = ts_type_to_wit(ret_raw.trim());
                let async_ret = if ret_raw.trim().starts_with("Promise") {
                    // unwrap Promise<T>
                    if let Some(inner) = unwrap_generic(ret_raw.trim(), "Promise") {
                        ret = ts_type_to_wit(&inner);
                    }
                    true
                } else {
                    is_async
                };
                let ret_str = if ret_raw.trim().is_empty() || ret_raw.trim() == "void" {
                    String::new()
                } else {
                    format!(" -> {}", ret)
                };
                exports.push(ExportSig {
                    name,
                    params,
                    ret: ret_str,
                    is_async: async_ret || is_async,
                });
            }
        }
    } else if !go_files.is_empty() {
        let world_re = regex::Regex::new(r"(?m)//\s*owasidea:world\s+([A-Za-z0-9_-]+)").unwrap();
        let host_log_re = regex::Regex::new(r"(?m)//\s*owasidea:host_log\b").unwrap();
        let host_kv_re = regex::Regex::new(r"(?m)//\s*owasidea:host_kv\b").unwrap();
        let export_re = regex::Regex::new(
            r"(?ms)//[^\n]*owasidea:export[^\n]*\n\s*func\s+(\w+)\s*\(([^)]*)\)\s*([^\n{]*)",
        )
        .unwrap();

        for file in &go_files {
            let content = std::fs::read_to_string(file)?;
            if let Some(c) = world_re.captures(&content) {
                world_name = Some(c[1].to_string());
            }
            if host_log_re.is_match(&content) {
                host_log = true;
            }
            if host_kv_re.is_match(&content) {
                host_kv = true;
            }
            for cap in export_re.captures_iter(&content) {
                let name = cap[1].to_string();
                let params_raw = cap[2].to_string();
                let ret_raw = cap[3].to_string();
                let marker = &cap[0];
                let is_async = marker.contains("async");
                exports.push(parse_go_fn_sig(&name, &params_raw, &ret_raw, is_async));
            }
        }
    } else if !cs_files.is_empty() {
        let world_re =
            regex::Regex::new(r#"(?i)\[\s*owasideaworld\s*\(\s*"([^"]+)"\s*\)\s*\]"#).unwrap();
        let host_log_re = regex::Regex::new(r#"(?i)\[\s*owasideahostlog\s*\]"#).unwrap();
        let host_kv_re = regex::Regex::new(r#"(?i)\[\s*owasideahostkv\s*\]"#).unwrap();
        let export_re = regex::Regex::new(r#"(?is)\[\s*owasideaexport\s*\]\s*(?:\[[^\]]*\]\s*)*((?:(?:public|private|protected|internal|static|async|virtual|override|new|unsafe|extern|partial)\s+)+)([\w<>,\.\?\[\]\s]+?)\s+(\w+)\s*\(([^)]*)\)"#).unwrap();
        for file in &cs_files {
            let content = std::fs::read_to_string(file)?;
            if let Some(c) = world_re.captures(&content) {
                world_name = Some(c[1].to_string());
            }
            if host_log_re.is_match(&content) {
                host_log = true;
            }
            if host_kv_re.is_match(&content) {
                host_kv = true;
            }
            for cap in export_re.captures_iter(&content) {
                let modifiers = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let ret_raw = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                let raw_name = cap.get(3).unwrap().as_str().to_string();
                let name = if let Some(first) = raw_name.chars().next() {
                    first.to_lowercase().to_string() + &raw_name[first.len_utf8()..]
                } else {
                    raw_name
                };
                let params_raw = cap.get(4).map(|m| m.as_str()).unwrap_or("");
                let is_async = modifiers.contains("async") || ret_raw.starts_with("Task");
                let ret_str = if ret_raw.is_empty() || ret_raw == "void" || ret_raw == "Task" {
                    String::new()
                } else if let Some(inner) = ret_raw
                    .strip_prefix("Task<")
                    .and_then(|s| s.strip_suffix('>'))
                {
                    let m = csharp_type_to_wit(inner.trim());
                    if m.is_empty() {
                        String::new()
                    } else {
                        format!(" -> {}", m)
                    }
                } else {
                    let m = csharp_type_to_wit(ret_raw);
                    if m.is_empty() {
                        String::new()
                    } else {
                        format!(" -> {}", m)
                    }
                };
                let params = parse_csharp_params(params_raw);
                exports.push(ExportSig {
                    name,
                    params,
                    ret: ret_str,
                    is_async,
                });
            }
        }
    } else if !c_files.is_empty() || !cpp_files.is_empty() {
        let is_cpp = !cpp_files.is_empty();
        let files = if is_cpp { &cpp_files } else { &c_files };
        let world_re =
            regex::Regex::new(r"(?im)//\s*owasidea[: _-]?world\s+([A-Za-z0-9_-]+)").unwrap();
        let host_log_re = regex::Regex::new(r"(?i)owasidea[: _-]?host[: _-]?log").unwrap();
        let host_kv_re = regex::Regex::new(r"(?i)owasidea[: _-]?host[: _-]?kv").unwrap();
        for file in files {
            let content = std::fs::read_to_string(file)?;
            for cap in world_re.captures_iter(&content) {
                world_name = Some(cap[1].to_string());
            }
            if host_log_re.is_match(&content) {
                host_log = true;
            }
            if host_kv_re.is_match(&content) {
                host_kv = true;
            }

            let lines: Vec<&str> = content.lines().collect();
            let mut i = 0usize;
            while i < lines.len() {
                let l = lines[i].trim();
                let low = l.to_lowercase();
                if low.contains("owasidea") && low.contains("export") {
                    let is_async = low.contains("async");
                    // find next non-empty, non-comment declaration
                    let mut j = i + 1;
                    while j < lines.len() {
                        let t = lines[j].trim();
                        if t.is_empty()
                            || t.starts_with("//")
                            || t.starts_with("/*")
                            || t.starts_with("*")
                        {
                            j += 1;
                            continue;
                        }
                        break;
                    }
                    if j < lines.len() {
                        let mut decl = lines[j].trim().to_string();
                        decl = decl.trim_end_matches(';').trim().to_string();
                        decl = decl.trim_end_matches('{').trim().to_string();
                        if let Some(paren) = decl.find('(') {
                            // matching close paren
                            let mut depth = 0i32;
                            let mut close = None;
                            for (idx, ch) in decl.char_indices().skip(paren) {
                                match ch {
                                    '(' => depth += 1,
                                    ')' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            close = Some(idx);
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            let before = decl[..paren].trim();
                            let mut toks: Vec<&str> = before.split_whitespace().collect();
                            let popped = toks.pop().unwrap_or("").to_string();
                            let had_ptr = popped.starts_with('*');
                            let fname = popped.trim_start_matches('*').to_string();
                            let mut ret_raw = toks.join(" ");
                            if had_ptr {
                                ret_raw = format!("{} *", ret_raw).trim().to_string();
                            }
                            let params_raw = if let Some(c) = close {
                                decl[paren + 1..c].to_string()
                            } else {
                                String::new()
                            };
                            let name = if fname.is_empty() {
                                String::new()
                            } else {
                                fname
                            };
                            if !name.is_empty() {
                                let params = if is_cpp {
                                    parse_cpp_params(&params_raw)
                                } else {
                                    parse_c_params(&params_raw)
                                };
                                let ret_mapped = if is_cpp {
                                    cpp_type_to_wit(&ret_raw)
                                } else {
                                    c_type_to_wit(&ret_raw)
                                };
                                let ret_str = if ret_mapped.is_empty() {
                                    String::new()
                                } else {
                                    format!(" -> {}", ret_mapped)
                                };
                                exports.push(ExportSig {
                                    name,
                                    params,
                                    ret: ret_str,
                                    is_async,
                                });
                            }
                        }
                    }
                }
                i += 1;
            }
        }
    } else {
        for file in &rs_files {
            let content = std::fs::read_to_string(file)?;
            let ast = match syn::parse_file(&content) {
                Ok(a) => a,
                Err(_) => continue,
            };
            for item in ast.items {
                match item {
                    syn::Item::Struct(s) => {
                        for attr in &s.attrs {
                            match attr_last(attr).as_deref() {
                                Some("world") => {
                                    if let Some(n) = parse_world_attr(attr) {
                                        world_name = Some(n);
                                    }
                                }
                                Some("host_log") => host_log = true,
                                Some("host_kv") => host_kv = true,
                                _ => {}
                            }
                        }
                    }
                    syn::Item::Fn(f) => {
                        if has_attr(&f.attrs, "export") {
                            exports.push(parse_fn_sig(&f.sig));
                        }
                    }
                    syn::Item::Impl(i) => {
                        for ii in &i.items {
                            if let syn::ImplItem::Fn(m) = ii {
                                if has_attr(&m.attrs, "export") {
                                    exports.push(parse_fn_sig(&m.sig));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let world = match world_name {
        Some(w) => w,
        None => return Ok(()),
    };

    // dedup exports by name
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::new();
    for e in exports {
        if seen.insert(e.name.clone()) {
            deduped.push(e);
        }
    }

    let wit_dir = plugin_dir.join("wit");
    std::fs::create_dir_all(&wit_dir)?;
    let wit_path = wit_dir.join(format!("{}.wit", world));

    let mut out = String::new();
    out.push_str(&format!("package poc:{}@0.1.0;\n\n", world));
    out.push_str(&format!("world {} {{\n", world));
    if host_log {
        out.push_str("    import poc:host/host-log@0.1.0;\n");
    }
    if host_kv {
        out.push_str("    import poc:host/host-kv@0.1.0;\n");
    }
    if (host_log || host_kv) && !deduped.is_empty() {
        out.push('\n');
    }
    for e in &deduped {
        let async_kw = if e.is_async { "async " } else { "" };
        out.push_str(&format!(
            "    export {}: {}func({}){};\n",
            e.name, async_kw, e.params, e.ret
        ));
    }
    out.push_str("}\n");
    std::fs::write(&wit_path, out)?;

    if host_log || host_kv {
        let deps_dir = wit_dir.join("deps").join("host");
        std::fs::create_dir_all(&deps_dir)?;
        let host_wit = deps_dir.join("host.wit");
        if !host_wit.exists() {
            std::fs::write(
                &host_wit,
                "package poc:host@0.1.0;\n\ninterface host-kv {\n    get: func(key: string) -> option<string>;\n    set: func(key: string, value: string);\n}\n\ninterface host-log {\n    log: func(msg: string);\n}\n",
            )?;
        }
    }

    println!("generated {}", wit_path.display());
    Ok(())
}

fn go_type_to_wit(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Some(inner) = t.strip_prefix("[]") {
        return format!("list<{}>", go_type_to_wit(inner));
    }
    if let Some(inner) = t.strip_prefix("*") {
        return go_type_to_wit(inner);
    }
    match t {
        "string" => "string".to_string(),
        "bool" => "bool".to_string(),
        "int" | "int32" => "s32".to_string(),
        "int8" => "s8".to_string(),
        "int16" => "s16".to_string(),
        "int64" => "s64".to_string(),
        "uint" | "uint32" => "u32".to_string(),
        "uint8" | "byte" => "u8".to_string(),
        "uint16" => "u16".to_string(),
        "uint64" => "u64".to_string(),
        "float32" => "f32".to_string(),
        "float64" => "f64".to_string(),
        "rune" => "char".to_string(),
        "error" => "string".to_string(),
        _ => t.to_lowercase(),
    }
}

fn parse_go_params(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    // Split by commas, tracking grouping: names without types accumulate
    // until a typed segment appears.
    let parts: Vec<&str> = raw.split(',').collect();
    let mut pending: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let fields: Vec<&str> = part.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        if fields.len() == 1 {
            pending.push(fields[0].to_string());
        } else {
            // typed: all but last are names, last is type
            for f in &fields[..fields.len() - 1] {
                pending.push(f.to_string());
            }
            let ty_raw = fields[fields.len() - 1];
            let wit = go_type_to_wit(ty_raw);
            for n in pending.drain(..) {
                if wit.is_empty() {
                    out.push(n);
                } else {
                    out.push(format!("{}: {}", n, wit));
                }
            }
        }
    }
    // trailing names without type -> default string
    for n in pending.drain(..) {
        out.push(format!("{}: string", n));
    }
    out.join(", ")
}

fn camel_to_kebab(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i != 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_go_fn_sig(name: &str, params_raw: &str, ret_raw: &str, is_async: bool) -> ExportSig {
    let params = parse_go_params(params_raw);
    let ret_trim = ret_raw.trim();
    let ret_str = if ret_trim.is_empty() {
        String::new()
    } else if ret_trim.starts_with('(') && ret_trim.ends_with(')') {
        let inner = &ret_trim[1..ret_trim.len() - 1];
        let ret_parts: Vec<&str> = inner
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if ret_parts.len() >= 2 && ret_parts[1] == "error" {
            format!(" -> result<{}, string>", go_type_to_wit(ret_parts[0]))
        } else if ret_parts.len() == 1 {
            let w = go_type_to_wit(ret_parts[0]);
            if w.is_empty() {
                String::new()
            } else {
                format!(" -> {}", w)
            }
        } else {
            // unsupported multi-return; take first
            let w = go_type_to_wit(ret_parts[0]);
            if w.is_empty() {
                String::new()
            } else {
                format!(" -> {}", w)
            }
        }
    } else {
        let w = go_type_to_wit(ret_trim);
        if w.is_empty() {
            String::new()
        } else {
            format!(" -> {}", w)
        }
    };
    ExportSig {
        name: camel_to_kebab(name),
        params,
        ret: ret_str,
        is_async,
    }
}

fn c_type_to_wit(ty: &str) -> String {
    let t = ty.trim();
    if t.is_empty() {
        return String::new();
    }
    // strip qualifiers/pointers for primitive mapping, but track pointer-ness for strings
    let lower = t.to_lowercase();
    if lower.contains("char") && (lower.contains('*') || lower.contains("char*")) {
        return "string".to_string();
    }
    if lower.contains("char *") || lower.contains("char*") {
        return "string".to_string();
    }
    let t = t
        .trim_start_matches("const")
        .trim_start_matches("volatile")
        .trim();
    let t = t.trim_end_matches('*').trim();
    match t {
        "void" => String::new(),
        "bool" | "_Bool" => "bool".to_string(),
        "char" | "signed char" => "s8".to_string(),
        "unsigned char" | "uint8_t" => "u8".to_string(),
        "short" | "short int" | "int16_t" => "s16".to_string(),
        "unsigned short" | "uint16_t" => "u16".to_string(),
        "int" | "signed int" | "int32_t" => "s32".to_string(),
        "unsigned int" | "uint32_t" => "u32".to_string(),
        "long" | "long int" | "int64_t" => "s64".to_string(),
        "unsigned long" | "uint64_t" => "u64".to_string(),
        "long long" => "s64".to_string(),
        "float" => "f32".to_string(),
        "double" => "f64".to_string(),
        "size_t" => "u64".to_string(),
        _ => {
            if t.starts_with("list<") || t.starts_with("option<") || t.starts_with("result<") {
                t.to_string()
            } else {
                t.to_lowercase()
            }
        }
    }
}

fn cpp_type_to_wit(ty: &str) -> String {
    let t = ty.trim();
    if t.is_empty() {
        return String::new();
    }
    let lower = t.to_lowercase();
    if lower == "void" {
        return String::new();
    }
    if lower == "std::string" || lower == "string" {
        return "string".to_string();
    }
    if lower == "bool" {
        return "bool".to_string();
    }
    if lower == "char" {
        return "s8".to_string();
    }
    if lower == "int" || lower == "int32_t" {
        return "s32".to_string();
    }
    if lower == "unsigned int" || lower == "uint32_t" {
        return "u32".to_string();
    }
    if lower == "long" || lower == "int64_t" {
        return "s64".to_string();
    }
    if lower == "unsigned long" || lower == "uint64_t" {
        return "u64".to_string();
    }
    if lower == "float" {
        return "f32".to_string();
    }
    if lower == "double" {
        return "f64".to_string();
    }
    if let Some(inner) = t
        .strip_prefix("std::vector<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return format!("list<{}>", cpp_type_to_wit(inner.trim()));
    }
    if let Some(inner) = t
        .strip_prefix("std::optional<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return format!("option<{}>", cpp_type_to_wit(inner.trim()));
    }
    if let Some(inner) = t
        .strip_prefix("std::list<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return format!("list<{}>", cpp_type_to_wit(inner.trim()));
    }
    t.to_lowercase()
}

fn parse_c_params(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() || raw == "void" {
        return String::new();
    }
    let parts = split_top_level(raw, ',');
    let mut out = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // param form: "type name" (name last token)
        let toks: Vec<&str> = part.split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        if toks.len() == 1 {
            // name only -> default string
            out.push(format!("{}: string", toks[0]));
            continue;
        }
        let pname = toks[toks.len() - 1].trim_matches('*');
        let ty = toks[..toks.len() - 1].join(" ");
        let wit = if ty.to_lowercase().contains("char") && part.contains('*') {
            "string".to_string()
        } else {
            c_type_to_wit(&ty)
        };
        if wit.is_empty() {
            out.push(pname.to_string());
        } else {
            out.push(format!("{}: {}", pname, wit));
        }
    }
    out.join(", ")
}

fn parse_cpp_params(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() || raw == "void" {
        return String::new();
    }
    let parts = split_top_level(raw, ',');
    let mut out = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // support "type name" or "name: type"
        if part.contains(':') && !part.contains("::") {
            if let Some(colon) = part.find(':') {
                let name = part[..colon].trim().to_string();
                let ty = part[colon + 1..].trim();
                let wit = cpp_type_to_wit(ty);
                if wit.is_empty() {
                    out.push(name);
                } else {
                    out.push(format!("{}: {}", name, wit));
                }
                continue;
            }
        }
        let toks: Vec<&str> = part.split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        if toks.len() == 1 {
            out.push(format!("{}: string", toks[0]));
            continue;
        }
        let pname = toks[toks.len() - 1].trim_matches('&').trim_matches('*');
        let ty = toks[..toks.len() - 1].join(" ");
        let wit = cpp_type_to_wit(&ty);
        if wit.is_empty() {
            out.push(pname.to_string());
        } else {
            out.push(format!("{}: {}", pname, wit));
        }
    }
    out.join(", ")
}

fn cmd_wit_generate(path: &Path, _json: bool) -> anyhow::Result<()> {
    generate_wit(path)
}

fn generate_keypair(private_key_path: &Path, public_key_path: &Path) -> anyhow::Result<()> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    if let Some(parent) = private_key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = public_key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .map_err(|e| anyhow::anyhow!("keygen failed: {e}"))?;
    let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|e| anyhow::anyhow!("parse key failed: {e}"))?;
    let pub_bytes = keypair.public_key().as_ref();
    std::fs::write(private_key_path, base64_encode(pkcs8.as_ref()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(private_key_path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::write(public_key_path, base64_encode(pub_bytes))?;
    println!(
        "generated {} and {}",
        private_key_path.display(),
        public_key_path.display()
    );
    Ok(())
}

fn cmd_keygen(out: &Path) -> anyhow::Result<()> {
    generate_keypair(&out.join("owasidea.key"), &out.join("owasidea.pub"))
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn sign_manifest(manifest_dir: &Path, plugin: &str, key_path: &Path) -> anyhow::Result<()> {
    use ring::signature::Ed25519KeyPair;
    let manifest_path = manifest_dir.join(format!("{}.manifest.json", plugin));
    let raw = std::fs::read_to_string(&manifest_path)?;
    let mut manifest: serde_json::Value = serde_json::from_str(&raw)?;
    let bytes = if manifest["runtime"].as_str() == Some("js-wasm") {
        Vec::new()
    } else {
        let comp_rel = manifest["component"].as_str().unwrap_or("").to_string();
        let comp_path = manifest_path.parent().unwrap().join(&comp_rel);
        std::fs::read(&comp_path)?
    };
    let key_b64 = std::fs::read_to_string(key_path)?;
    let key_bytes = base64_decode(key_b64.trim())?;
    let keypair = Ed25519KeyPair::from_pkcs8(&key_bytes)
        .map_err(|e| anyhow::anyhow!("invalid private key: {e}"))?;
    manifest["permissions"]
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("manifest permissions must be an object"))?
        .entry("network")
        .or_insert_with(|| serde_json::json!({"outbound": [], "inbound": []}));
    manifest["metadata"]["signature_format"] = serde_json::json!("owasidea-manifest-v1");
    manifest["metadata"]
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("manifest metadata must be an object"))?
        .remove("signature");
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let mut payload = b"owasidea-plugin-signature-v1\0".to_vec();
    payload.extend_from_slice(&manifest_bytes);
    payload.push(0);
    payload.extend_from_slice(&bytes);
    let sig = keypair.sign(&payload);
    manifest["metadata"]["signature"] = serde_json::json!(base64_encode(sig.as_ref()));
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    Ok(())
}

fn sign_all(
    root: &Path,
    manifest_dir: &Path,
    signing: Option<&SigningSection>,
) -> anyhow::Result<()> {
    let Some(signing) = signing else {
        return Ok(());
    };
    let key_path = app_path(root, signing.key.as_deref().unwrap_or("keys/owasidea.key"));
    if !key_path.is_file() {
        if !signing.generate {
            anyhow::bail!(
                "signing key not found at {}; run `owasidea keygen --out keys` or set [signing].generate = true",
                key_path.display()
            );
        }
        let public_key_path = app_path(
            root,
            signing.public_key.as_deref().unwrap_or("keys/owasidea.pub"),
        );
        generate_keypair(&key_path, &public_key_path)?;
    }
    let mut names = std::fs::read_dir(manifest_dir)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_suffix(".manifest.json").map(str::to_string)
        })
        .collect::<Vec<_>>();
    names.sort();
    for name in &names {
        sign_manifest(manifest_dir, name, &key_path)?;
    }
    if !names.is_empty() {
        println!("signed {} manifests", names.len());
    }
    Ok(())
}

fn apply_signing_env(config_path: &Path) {
    let root = app_root(config_path);
    let Ok(config) = read_app_config(config_path) else {
        return;
    };
    let Some(signing) = config.signing else {
        return;
    };
    let public_key = app_path(
        &root,
        signing.public_key.as_deref().unwrap_or("keys/owasidea.pub"),
    );
    if public_key.is_file() {
        std::env::set_var("OWASIDEA_PUBLIC_KEY", &public_key);
    }
    std::env::set_var(
        "OWASIDEA_REQUIRE_SIGNATURES",
        if signing.require.unwrap_or(true) {
            "true"
        } else {
            "false"
        },
    );
}

fn ensure_npm_dependencies(plugin_dir: &Path) -> anyhow::Result<()> {
    if !plugin_dir.join("package.json").is_file() || plugin_dir.join("node_modules").is_dir() {
        return Ok(());
    }
    println!(
        "    installing npm dependencies in {}",
        plugin_dir.display()
    );
    let status = external_command("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(plugin_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("npm install failed in {}", plugin_dir.display());
    }
    Ok(())
}

fn cmd_sign(manifest_dir: &Path, plugin: &str, key_path: &Path) -> anyhow::Result<()> {
    sign_manifest(manifest_dir, plugin, key_path)?;
    println!(
        "signed {} -> {}",
        plugin,
        manifest_dir
            .join(format!("{}.manifest.json", plugin))
            .display()
    );
    Ok(())
}

fn base64_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))
}

#[cfg(test)]
mod csp_tests {
    use super::*;

    #[test]
    fn connect_src_matches_configured_host() {
        let csp = "default-src 'self'; connect-src 'self' ipc: http://127.0.0.1:8790";
        assert!(csp_allows_origin(csp, "http://127.0.0.1:8790"));
        assert!(!csp_allows_origin(csp, "http://127.0.0.1:9000"));
    }

    #[test]
    fn wildcard_port_and_default_src_are_supported() {
        assert!(csp_allows_origin(
            "default-src 'self' http://127.0.0.1:*",
            "http://127.0.0.1:9000"
        ));
        assert!(!csp_allows_origin(
            "default-src 'self'",
            "http://127.0.0.1:9000"
        ));
    }
}

#[cfg(test)]
mod dev_tests {
    use super::*;

    #[test]
    fn dev_watcher_extracts_wasi_plugin_and_skips_webview_plugins() {
        let root = std::env::temp_dir().join(format!("owasidea-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let component = root.join("demo.component.wasm");
        std::fs::write(&component, b"artifact").unwrap();
        assert_eq!(plugin_changed_by_path(&component, &root), None);
        std::fs::write(
            root.join("demo.manifest.json"),
            r#"{"name":"demo","runtime":"wasi-component"}"#,
        )
        .unwrap();
        assert_eq!(
            plugin_changed_by_path(&component, &root),
            Some("demo".to_string())
        );

        let ui_manifest = root.join("dashboard.manifest.json");
        std::fs::write(&ui_manifest, r#"{"name":"dashboard","runtime":"js-wasm"}"#).unwrap();
        assert_eq!(plugin_changed_by_path(&ui_manifest, &root), None);

        let wasi_manifest = root.join("notes.manifest.json");
        std::fs::write(
            &wasi_manifest,
            r#"{"name":"notes","runtime":"wasi-component"}"#,
        )
        .unwrap();
        assert_eq!(
            plugin_changed_by_path(&wasi_manifest, &root),
            Some("notes".to_string())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dev_reserves_concrete_host_and_ui_ports() {
        assert_ne!(reserve_http_addr("127.0.0.1:0").unwrap(), "127.0.0.1:0");
        assert!(reserve_ui_port().unwrap() >= 5173);
    }

    #[test]
    fn reserve_ui_addr_falls_back_when_occupied() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let occupied = listener.local_addr().unwrap().port();
        let reserved = reserve_ui_addr(Some(&format!("127.0.0.1:{occupied}"))).unwrap();
        assert_ne!(reserved, occupied);
        assert!(reserved > occupied);
    }

    #[test]
    fn tauri_dev_config_preserves_base_and_sets_devurl() {
        let root = std::env::temp_dir().join("owasidea-rf06-tauri-dev");
        let _ = std::fs::remove_dir_all(&root);
        let desktop = root.join("desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::write(
            desktop.join("tauri.conf.json"),
            r#"{"productName":"Demo","build":{"frontendDist":"../ui/dist","devUrl":"http://localhost:5173"},"app":{"security":{"csp":"default-src 'self'"}}}"#,
        )
        .unwrap();
        let config_path = root.join("wasm-app.toml");
        std::fs::write(
            &config_path,
            format!(
                "[app]\nname = \"Demo\"\n\n[desktop]\npath = \"{}\"\n",
                desktop.display()
            ),
        )
        .unwrap();
        let path =
            crate::config_render::write_tauri_dev_config(&config_path, "http://127.0.0.1:5199")
                .unwrap();
        assert_eq!(path, desktop.join("tauri.dev.conf.json"));
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["build"]["devUrl"], "http://127.0.0.1:5199");
        assert_eq!(value["productName"], "Demo");
        assert_eq!(
            value["app"]["security"]["csp"],
            "default-src 'self'; connect-src http://127.0.0.1:8787 http://127.0.0.1:*"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

fn cmd_client(manifest_dir: &Path, lang: &str, output: &Path, check: bool) -> anyhow::Result<()> {
    if lang != "ts" {
        anyhow::bail!("unsupported language: {} (only ts)", lang);
    }
    let expected = client_gen::generate_ts(manifest_dir)?;
    if check {
        let actual = std::fs::read_to_string(output).ok();
        match actual {
            Some(a) if a == expected => Ok(()),
            Some(_) => anyhow::bail!("generated client is out of date: {}", output.display()),
            None => anyhow::bail!("generated client missing: {}", output.display()),
        }
    } else {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output, expected)?;
        Ok(())
    }
}

#[cfg(test)]
mod dl08_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ui_plugin_name_for_dist() {
        let plugins = PathBuf::from("/tmp/app/plugins");
        assert_eq!(
            ui_plugin_name_for_path(&plugins.join("foo/dist/index.html"), &plugins),
            Some("foo".to_string())
        );
        assert_eq!(
            ui_plugin_name_for_path(&plugins.join("foo/dist/assets/x.js"), &plugins),
            Some("foo".to_string())
        );
        assert_eq!(
            ui_plugin_name_for_path(&plugins.join("foo/index.html"), &plugins),
            Some("foo".to_string())
        );
        assert_eq!(
            ui_plugin_name_for_path(&plugins.join("foo/src/main.ts"), &plugins),
            None
        );
        assert_eq!(
            ui_plugin_name_for_path(&plugins.join("foo/node_modules/x"), &plugins),
            None
        );
        assert_eq!(
            ui_plugin_name_for_path(&plugins.join("foo/target/debug/x"), &plugins),
            None
        );
    }
}
