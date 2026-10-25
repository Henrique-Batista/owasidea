use clap::{Parser, Subcommand};
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
    Doctor,
    /// Manage plugins (create new plugins).
    #[command(about = "Manage plugins (create new plugins).")]
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// Build all plugins, regenerate manifests and copy .wasm artifacts.
    #[command(about = "Build all plugins, regenerate manifests and copy .wasm artifacts.")]
    Build,
    /// Run the app in development mode (host + UI dev server).
    #[command(about = "Run the app in development mode (host + UI dev server).")]
    Dev,
    /// Run the bundled app without file watching.
    #[command(about = "Run the bundled app without file watching.")]
    Run,
    /// Inspect a plugin manifest (WIT imports/exports, permissions).
    #[command(about = "Inspect a plugin manifest (WIT imports/exports, permissions).")]
    Inspect {
        /// Plugin name.
        #[arg(help = "Plugin name.")]
        plugin: String,
    },
    /// Build the desktop bundle (Tauri).
    #[command(about = "Build the desktop bundle (Tauri).")]
    Bundle,
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
    },
}

fn read_host_addr(config: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config).ok()?;
    let val: toml::Value = toml::from_str(&raw).ok()?;
    val.get("host")?
        .get("addr")?
        .as_str()
        .map(|s| s.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Init { dir } => cmd_init(dir, cli.json),
        Commands::Doctor => cmd_doctor(cli.json),
        Commands::Plugin { cmd } => match cmd {
            PluginCmd::New { name, lang, r#type } => cmd_plugin_new(name, lang, r#type, cli.json),
        },
        Commands::Build => cmd_build(&cli.config, &cli.manifest_dir),
        Commands::Dev => {
            let addr = cli
                .http_addr
                .clone()
                .or_else(|| read_host_addr(&cli.config))
                .unwrap_or_else(|| "127.0.0.1:0".to_string());
            cmd_dev(&cli.manifest_dir, &addr).await
        }
        Commands::Run => {
            let addr = cli
                .http_addr
                .clone()
                .or_else(|| read_host_addr(&cli.config))
                .unwrap_or_else(|| "127.0.0.1:0".to_string());
            cmd_run(&cli.manifest_dir, &addr).await
        }
        Commands::Inspect { plugin } => cmd_inspect(&cli.manifest_dir, &plugin, cli.json),
        Commands::Bundle => cmd_bundle(),
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
addr = "127.0.0.1:0"

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
    println!("scaffolded {}", target_dir.display());
    Ok(())
}

fn cmd_doctor(json: bool) -> anyhow::Result<()> {
    use std::process::Command;
    let checks = [
        ("rustc", "rustc --version"),
        ("cargo", "cargo --version"),
        ("cargo-component", "cargo component --version"),
        ("wasm-tools", "wasm-tools --version"),
        ("go", "go version"),
        ("node", "node --version"),
        ("pnpm", "pnpm --version"),
    ];
    let optional = [
        ("clang (C/C++)", "clang --version"),
        ("dotnet (C#)", "dotnet --version"),
        ("jco (JS/TS)", "jco --version"),
    ];
    let mut results: Vec<(&str, bool, bool)> = Vec::new();
    for (name, cmd) in checks {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let ok = Command::new(parts[0])
            .args(&parts[1..])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        results.push((name, ok, false));
    }
    for (name, cmd) in optional {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let ok = Command::new(parts[0])
            .args(&parts[1..])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        results.push((name, ok, true));
    }
    if json {
        let v: Vec<_> = results
            .iter()
            .map(|(n, ok, opt)| serde_json::json!({"tool": n, "ok": ok, "optional": opt}))
            .collect();
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
    }
    if results.iter().any(|(_, ok, opt)| !ok && !*opt) {
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_plugin_new(
    name: Option<String>,
    lang: Option<String>,
    kind: Option<String>,
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
                let items = ["rust", "go", "typescript", "csharp", "c", "cpp"];
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
                let items = ["library", "executable"];
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

    let dir = std::path::Path::new("plugins").join(&name);
    std::fs::create_dir_all(dir.join("wit"))?;
    std::fs::create_dir_all(dir.join("src"))?;
    let wit = format!(
        "package poc:{name}@0.1.0;\n\nworld {name} {{\n    export greet: func(name: string) -> string;\n}}\n"
    );
    std::fs::write(dir.join("wit").join(format!("{name}.wit")), wit)?;
    match lang.as_str() {
        "rust" => {
            std::fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[workspace]\n\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nwit-bindgen = \"0.41\"\n\n[package.metadata.component]\npackage = \"poc:{name}\"\n[package.metadata.component.target]\npath = \"wit\"\nworld = \"{name}\"\n"
                ),
            )?;
            std::fs::write(
                dir.join("src").join("lib.rs"),
                format!(
                    "wit_bindgen::generate!({{ world: \"{name}\", path: \"wit\", generate_all }});\n\nstruct Component;\nimpl Guest for Component {{\n    fn greet(name: String) -> String {{\n        format!(\"Hello, {{}}! (from Rust)\", name)\n    }}\n}}\nexport!(Component);\n"
                ),
            )?;
        }
        "go" => {
            std::fs::write(
                dir.join("go.mod"),
                format!("module poc/{name}\n\ngo 1.27.1\n"),
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
                    "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"scripts\": {{\n    \"build\": \"jco componentize src/index.ts --wit wit --world {name} -o {name}.component.wasm\"\n  }}\n}}\n"
                ),
            )?;
            std::fs::write(
                dir.join("src").join("index.ts"),
                "export function greet(name: string): string {\n  return `Hello, ${name}! (from TypeScript)`;\n}\n",
            )?;
        }
        "csharp" => {
            std::fs::write(
                dir.join(format!("{name}.csproj")),
                "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n",
            )?;
            std::fs::write(
                dir.join("Program.cs"),
                "using System;\nclass Program {\n    static void Main() { Console.WriteLine(\"Hello from C#\"); }\n}\n",
            )?;
        }
        "c" | "cpp" | "c++" => {
            let ext = if lang == "c" { "c" } else { "cpp" };
            let compiler = if lang == "c" { "clang" } else { "clang++" };
            std::fs::write(
                dir.join("Makefile"),
                format!(
                    "WASI_SDK ?= /opt/wasi-sdk\nCC = $(WASI_SDK)/bin/{compiler}\n\nall:\n\t$(CC) --target=wasm32-wasip1 -o {name}.wasm src/main.{ext}\n\twasm-tools component new {name}.wasm -o {name}.component.wasm\n"
                ),
            )?;
            std::fs::write(
                dir.join("src").join(format!("main.{ext}")),
                if lang == "c" {
                    "#include <stdio.h>\nint main(void) {\n    printf(\"Hello from C!\\n\");\n    return 0;\n}\n".to_string()
                } else {
                    "#include <iostream>\nint main() {\n    std::cout << \"Hello from C++!\" << std::endl;\n    return 0;\n}\n".to_string()
                },
            )?;
        }
        _ => {}
    }
    std::fs::write(
        dir.join("README.md"),
        format!("# {name}\n\nLanguage: {lang}\nType: {kind}\n"),
    )?;
    println!(
        "created plugin scaffold at {} (lang={}, type={})",
        dir.display(),
        lang,
        kind
    );
    Ok(())
}

#[derive(serde::Deserialize, Default, Clone)]
struct PluginSection {
    path: String,
    language: String,
    #[serde(rename = "type", default = "default_plugin_type")]
    kind: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    fs: Vec<String>,
    #[serde(default)]
    kv: Option<String>,
    #[serde(default)]
    http_outbound: Vec<String>,
    #[serde(default)]
    persistent: bool,
}

fn default_plugin_type() -> String {
    "library".to_string()
}

#[derive(serde::Deserialize, Default)]
struct AppConfig {
    app: Option<AppSection>,
    plugins: Option<std::collections::BTreeMap<String, PluginSection>>,
}

#[derive(serde::Deserialize, Default)]
struct AppSection {
    version: Option<String>,
}

fn cmd_build(config: &Path, manifest_dir: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(config)?;
    let cfg: AppConfig = toml::from_str(&raw)?;
    let plugins = cfg.plugins.clone().unwrap_or_default();
    if plugins.is_empty() {
        println!("no plugins declared in {}", config.display());
        return Ok(());
    }
    std::fs::create_dir_all(manifest_dir)?;
    let script_path = std::env::temp_dir().join("owasidea-gen-manifest.py");
    std::fs::write(
        &script_path,
        include_str!("../../../scripts/gen-manifest.py"),
    )?;

    let app_version = cfg
        .app
        .as_ref()
        .and_then(|a| a.version.clone())
        .unwrap_or_else(|| "0.1.0".to_string());

    for (name, p) in &plugins {
        println!("==> Building {} ({})", name, p.language);
        let lang = p.language.as_str();
        match lang {
            "rust" => {
                let status = std::process::Command::new("cargo")
                    .args(["component", "build", "--release"])
                    .current_dir(&p.path)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("rust build failed for {}", name);
                }
                let out = std::process::Command::new("cargo")
                    .args(["metadata", "--format-version", "1", "--no-deps"])
                    .current_dir(&p.path)
                    .output()?;
                let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
                let td = meta["target_directory"].as_str().unwrap_or("");
                let artifact_name = name.replace('-', "_");
                let src = std::path::Path::new(td)
                    .join("wasm32-wasip1/release")
                    .join(format!("{}.wasm", artifact_name));
                let dst = std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
                std::fs::copy(&src, &dst)?;
            }
            "go" => {
                let status = std::process::Command::new("go")
                    .args(["tool", "componentize-go", "build"])
                    .current_dir(&p.path)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("go build failed for {}", name);
                }
                let src = std::path::Path::new(&p.path).join("main.wasm");
                let dst = std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
                std::fs::copy(&src, &dst)?;
            }
            "typescript" | "javascript" => {
                let status = std::process::Command::new("npm")
                    .args(["run", "build"])
                    .current_dir(&p.path)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("npm build failed for {}", name);
                }
                let produced = std::fs::read_dir(&p.path)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .find(|pp| {
                        pp.file_name()
                            .and_then(|s| s.to_str())
                            .map(|s| s.ends_with(".component.wasm"))
                            .unwrap_or(false)
                    });
                if let Some(src) = produced {
                    let dst =
                        std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
                    if src != dst {
                        std::fs::copy(&src, &dst)?;
                    }
                }
            }
            "c" | "cpp" | "c++" => {
                let status = std::process::Command::new("make")
                    .current_dir(&p.path)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("make failed for {}", name);
                }
            }
            "csharp" => {
                let status = std::process::Command::new("dotnet")
                    .args(["build", "-c", "Release"])
                    .current_dir(&p.path)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("dotnet build failed for {}", name);
                }
                let mut module = None;
                for entry in walkdir(std::path::Path::new(&p.path).join("bin")) {
                    if entry.extension().and_then(|e| e.to_str()) == Some("wasm")
                        && !entry.to_string_lossy().contains("component")
                    {
                        module = Some(entry);
                        break;
                    }
                }
                if let Some(m) = module {
                    let dst =
                        std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
                    let status = std::process::Command::new("wasm-tools")
                        .args([
                            "component",
                            "new",
                            m.to_str().unwrap(),
                            "-o",
                            dst.to_str().unwrap(),
                        ])
                        .status()?;
                    if !status.success() {
                        anyhow::bail!("wasm-tools component new failed");
                    }
                }
            }
            other => anyhow::bail!("unsupported language: {}", other),
        }

        let component = std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
        if !component.exists() {
            anyhow::bail!("component not found after build: {}", component.display());
        }
        let manifest = manifest_dir.join(format!("{}.manifest.json", name));
        let mut cmd = std::process::Command::new("python3");
        cmd.arg(&script_path)
            .arg("--component")
            .arg(&component)
            .arg("--name")
            .arg(name)
            .arg("--version")
            .arg(p.version.clone().unwrap_or_else(|| app_version.clone()))
            .arg("--language")
            .arg(lang)
            .arg("--type")
            .arg(&p.kind)
            .arg("--out")
            .arg(&manifest);
        for fs in &p.fs {
            cmd.arg("--fs").arg(fs);
        }
        if let Some(kv) = &p.kv {
            cmd.arg("--kv").arg(kv);
        }
        for h in &p.http_outbound {
            cmd.arg("--http-outbound").arg(h);
        }
        if p.persistent {
            cmd.arg("--persistent");
            if !p.fs.is_empty() {
                cmd.arg("--storage").arg("wasi-filesystem");
            } else if p.kv.is_some() {
                cmd.arg("--storage").arg("host-kv");
            }
        }
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("manifest generation failed for {}", name);
        }
        println!("    manifest: {}", manifest.display());
    }
    println!("Build complete.");
    Ok(())
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

async fn cmd_dev(manifest_dir: &Path, http_addr: &str) -> anyhow::Result<()> {
    println!("UI URL:    http://localhost:5173");
    println!("Host URL:  (assigned by host, see below)");
    println!("Manifests: {}", manifest_dir.display());

    // Start Vite dev server if a UI project is present.
    let mut children: Vec<std::process::Child> = Vec::new();
    if std::path::Path::new("ui/package.json").exists() {
        let child = std::process::Command::new("npm")
            .args(["run", "dev"])
            .current_dir("ui")
            .spawn();
        match child {
            Ok(c) => children.push(c),
            Err(e) => eprintln!("failed to start UI dev server: {e}"),
        }
    }

    let manifests = manifest_dir.to_path_buf();
    let addr = http_addr.to_string();
    let res = owasidea_host::run(manifests, addr).await;

    for mut c in children {
        let _ = c.kill();
        let _ = c.wait();
    }
    res
}

async fn cmd_run(manifest_dir: &Path, http_addr: &str) -> anyhow::Result<()> {
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
    }
    Ok(())
}

fn cmd_bundle() -> anyhow::Result<()> {
    println!("Building UI...");
    let status = std::process::Command::new("npm")
        .args(["run", "build"])
        .current_dir("ui")
        .status()?;
    if !status.success() {
        anyhow::bail!("ui build failed");
    }
    println!("Building Tauri bundle...");
    let status = std::process::Command::new("cargo")
        .args(["tauri", "build"])
        .current_dir("crates/desktop")
        .status()?;
    if !status.success() {
        anyhow::bail!("tauri build failed");
    }
    println!("Bundle complete.");
    Ok(())
}
