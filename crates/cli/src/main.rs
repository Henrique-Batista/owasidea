use clap::{Parser, Subcommand};
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
    #[arg(
        long,
        global = true,
        default_value = "127.0.0.1:8787",
        env = "HOST_ADDR"
    )]
    http_addr: String,
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
        /// Target directory.
        #[arg(help = "Target directory.")]
        dir: PathBuf,
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
        name: String,
        #[arg(long, default_value = "rust")]
        lang: String,
        #[arg(long, default_value = "library")]
        r#type: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Init { dir } => cmd_init(&dir),
        Commands::Doctor => cmd_doctor(cli.json),
        Commands::Plugin { cmd } => match cmd {
            PluginCmd::New { name, lang, r#type } => cmd_plugin_new(&name, &lang, &r#type),
        },
        Commands::Build => cmd_build(&cli.config, &cli.manifest_dir),
        Commands::Dev => cmd_dev(&cli.manifest_dir, &cli.http_addr).await,
        Commands::Run => cmd_run(&cli.manifest_dir, &cli.http_addr).await,
        Commands::Inspect { plugin } => cmd_inspect(&cli.manifest_dir, &plugin, cli.json),
        Commands::Bundle => cmd_bundle(),
    }
}

fn cmd_init(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let cfg = dir.join("wasm-app.toml");
    if !cfg.exists() {
        std::fs::write(
            &cfg,
            r#"[app]
name = "my-app"
version = "0.1.0"
identifier = "dev.example.myapp"

[host]
addr = "127.0.0.1:8787"

[dev]
watch = true
hot_reload = true
"#,
        )?;
    }
    for sub in ["plugins", "manifests", "ui", "docs"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    println!("scaffolded {}", dir.display());
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

fn cmd_plugin_new(name: &str, lang: &str, kind: &str) -> anyhow::Result<()> {
    let dir = std::path::Path::new("plugins").join(name);
    std::fs::create_dir_all(dir.join("wit"))?;
    std::fs::create_dir_all(dir.join("src"))?;
    let wit = format!(
        "package poc:{name}@0.1.0;\n\nworld {name} {{\n    export greet: func(name: string) -> string;\n}}\n"
    );
    std::fs::write(dir.join("wit").join(format!("{name}.wit")), wit)?;
    match lang {
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
                    "WASI_SDK ?= /opt/wasi-sdk\\nCC = $(WASI_SDK)/bin/{compiler}\\n\\nall:\\n\\t$(CC) --target=wasm32-wasip1 -o {name}.wasm src/main.{ext}\\n\\twasm-tools component new {name}.wasm -o {name}.component.wasm\\n"
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

fn cmd_build(_config: &Path, manifest_dir: &Path) -> anyhow::Result<()> {
    // Invoke build-all.sh if present
    let script = std::path::Path::new("scripts/build-all.sh");
    if script.exists() {
        let status = std::process::Command::new("bash").arg(script).status()?;
        if !status.success() {
            anyhow::bail!("build failed");
        }
        Ok(())
    } else {
        println!(
            "no build script found; manifests in {}",
            manifest_dir.display()
        );
        Ok(())
    }
}

async fn cmd_dev(manifest_dir: &Path, http_addr: &str) -> anyhow::Result<()> {
    println!("UI URL:    http://localhost:5173");
    println!("Host URL:  http://{}", http_addr);
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
