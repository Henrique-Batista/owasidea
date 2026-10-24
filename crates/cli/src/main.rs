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
    Init {
        dir: PathBuf,
    },
    Doctor,
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    Build,
    Dev,
    Run,
    Inspect {
        plugin: String,
    },
    Bundle,
}

#[derive(Subcommand)]
enum PluginCmd {
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
    let mut results = Vec::new();
    for (name, cmd) in checks {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let ok = Command::new(parts[0])
            .args(&parts[1..])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        results.push((name, ok));
    }
    if json {
        let v: Vec<_> = results
            .iter()
            .map(|(n, ok)| serde_json::json!({"tool": n, "ok": ok}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        for (name, ok) in &results {
            println!("{:20} {}", name, if *ok { "ok" } else { "MISSING" });
        }
    }
    if results.iter().any(|(_, ok)| !ok) {
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_plugin_new(name: &str, lang: &str, kind: &str) -> anyhow::Result<()> {
    let dir = std::path::Path::new("plugins").join(name);
    std::fs::create_dir_all(&dir)?;
    println!(
        "created plugin scaffold at {} (lang={}, type={})",
        dir.display(),
        lang,
        kind
    );
    // Minimal template note
    std::fs::write(
        dir.join("README.md"),
        format!("# {name}\n\nLanguage: {lang}\nType: {kind}\n"),
    )?;
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
    // Run host
    let manifests = manifest_dir.to_path_buf();
    let addr = http_addr.to_string();
    owasidea_host::run(manifests, addr).await
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
    println!("bundle not implemented in v0");
    Ok(())
}
