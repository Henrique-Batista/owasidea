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

fn tool_path(name: &str) -> Option<std::path::PathBuf> {
    // PATH
    if let Ok(path) = std::process::Command::new("which").arg(name).output() {
        if path.status.success() {
            let p = String::from_utf8_lossy(&path.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(std::path::PathBuf::from(p));
            }
        }
    }
    let candidates = match name {
        "dotnet" => vec![
            "/usr/share/dotnet/dotnet",
            "/root/.dotnet/dotnet",
            "/home/pablitux/.dotnet/dotnet",
        ],
        "clang" => vec!["/usr/bin/clang", "/opt/wasi-sdk/bin/clang"],
        "jco" => vec!["/home/pablitux/.npm-global/bin/jco"],
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
        ("componentize-dotnet (C#)", "componentize-dotnet --version"),
        ("python3 (optional)", "python3 --version"),
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
        let tool = cmd.split_whitespace().next().unwrap_or("");
        let resolved = tool_path(tool).unwrap_or_else(|| std::path::PathBuf::from(tool));
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let ok = Command::new(&resolved)
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
    persistent: bool,
) -> anyhow::Result<()> {
    use regex::Regex;
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

    // exports via text
    let text_out = std::process::Command::new("wasm-tools")
        .args(["component", "wit"])
        .arg(component)
        .output()?;
    let text = String::from_utf8_lossy(&text_out.stdout);
    let export_re = Regex::new(
        r"(?m)^\s*export\s+([A-Za-z0-9_-]+)\s*:\s*(?:async\s+)?(func\(.*?\)(?:\s*->\s*[^;]+)?);",
    )
    .unwrap();
    let export_iface_re =
        Regex::new(r"(?m)^\s*export\s+([A-Za-z0-9:_/-]+(?:@[0-9.]+)?)\s*;").unwrap();
    let mut exports = Vec::new();
    for cap in export_re.captures_iter(&text) {
        exports.push(json!({"name": &cap[1], "signature": cap[2].trim()}));
    }
    for cap in export_iface_re.captures_iter(&text) {
        exports.push(json!({"name": &cap[1], "signature": "interface"}));
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
        fs_perms.push(json!({"path": path, "access": access, "preopen": true}));
    }

    // kv
    let kv = kv.map(|k| {
        let (ns, access) = if let Some((n, a)) = k.rsplit_once(':') {
            (n.to_string(), a.to_string())
        } else {
            (k.to_string(), "read-write".to_string())
        };
        json!({"namespace": ns, "access": access})
    });

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
            "kv": kv,
            "resources": {"max_memory_mb": 64, "max_fuel": 1000000000, "timeout_ms": 30000, "max_instances": 1}
        },
        "state": {"persistent": persistent, "storage": storage,
                  "root": if storage == "wasi-filesystem" { format!("./data/{}", name) } else { String::new() }},
        "composition": {"strategy": "build-time", "entry": kind == "executable"},
        "metadata": {"author": "", "license": "MIT", "description": "", "sha256": sha}
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

fn cmd_build(config: &Path, manifest_dir: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(config)?;
    let cfg: AppConfig = toml::from_str(&raw)?;
    let plugins = cfg.plugins.clone().unwrap_or_default();
    if plugins.is_empty() {
        println!("no plugins declared in {}", config.display());
        return Ok(());
    }
    std::fs::create_dir_all(manifest_dir)?;

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
                let dotnet =
                    tool_path("dotnet").unwrap_or_else(|| std::path::PathBuf::from("dotnet"));
                let status = std::process::Command::new(&dotnet)
                    .args(["build", "-c", "Release", "--source", ""])
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
            p.persistent,
        )?;
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
