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
        Commands::Keygen { out } => cmd_keygen(&out),
        Commands::Sign { plugin, key } => cmd_sign(&cli.manifest_dir, &plugin, &key),
        Commands::Wit { cmd } => match cmd {
            WitCmd::Generate { path } => cmd_wit_generate(&path, cli.json),
        },
        Commands::Validate => cmd_validate(&cli.config, &cli.manifest_dir),
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

#[allow(clippy::useless_format)]
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
                    "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"scripts\": {{\n    \"build\": \"jco componentize src/index.ts --wit wit --world-name {name} -o {name}.component.wasm\"\n  }},\n  \"devDependencies\": {{\n    \"@bytecodealliance/jco\": \"latest\",\n    \"@bytecodealliance/componentize-js\": \"latest\"\n  }}\n}}\n"
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
    <PackageReference Include="runtime.linux-x64.microsoft.dotnet.ilcompiler.llvm" Version="10.0.0-rc.1.26306.1" />
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
                    "WASI_SDK ?= /home/pablitux/wasi-sdk/wasi-sdk-25.0-x86_64-linux\nADAPTER ?= /home/pablitux/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wasi-preview1-component-adapter-provider-48.0.7/artefacts/wasi_snapshot_preview1.reactor.wasm\nCC = $(WASI_SDK)/bin/{compiler}\n\nall:\n\twit-bindgen {lang} wit/{name}.wit --out-dir src\n\t$(CC) --target=wasm32-wasip1 {stdflag} -O2 -c src/{name}.c -o src/{name}.o 2>/dev/null || $(CC) --target=wasm32-wasip1 {stdflag} -O2 -c src/{name}.cpp -o src/{name}.o\n\t$(CC) --target=wasm32-wasip1 {stdflag} -O2 -c src/impl.{ext} -o src/impl.o\n\t$(CC) --target=wasm32-wasip1 {stdflag} -O2 src/{name}_component_type.o src/{name}.o src/impl.o -o {name}.wasm -mexec-model=reactor\n\twasm-tools component new {name}.wasm --adapt $(ADAPTER) -o {name}.component.wasm\n",
                    lang = if lang == "c" { "c" } else { "cpp" },
                    compiler = compiler,
                    stdflag = stdflag,
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

#[allow(clippy::regex_creation_in_loops)]
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
                // Generate WIT from Rust attributes if present.
                let _ = generate_wit(std::path::Path::new(&p.path));
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
                let _ = generate_wit(std::path::Path::new(&p.path));
                let wit_dir = std::path::Path::new(&p.path).join("wit");
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
                    cmd.current_dir(&p.path);
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
                        .current_dir(&p.path)
                        .status()?
                };
                if !status.success() {
                    anyhow::bail!("go build failed for {}", name);
                }
                let src = std::path::Path::new(&p.path).join("main.wasm");
                let dst = std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
                std::fs::copy(&src, &dst)?;
            }
            "typescript" | "javascript" => {
                let _ = generate_wit(std::path::Path::new(&p.path));
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
                let _ = generate_wit(std::path::Path::new(&p.path));
                let status = std::process::Command::new("make")
                    .current_dir(&p.path)
                    .status()?;
                if !status.success() {
                    anyhow::bail!("make failed for {}", name);
                }
            }
            "csharp" => {
                let _ = generate_wit(std::path::Path::new(&p.path));
                let dotnet =
                    tool_path("dotnet").unwrap_or_else(|| std::path::PathBuf::from("dotnet"));
                let mut cmd = std::process::Command::new(&dotnet);
                cmd.args(["build", "-c", "Release", "--source", ""])
                    .current_dir(&p.path);
                let wasi =
                    std::path::PathBuf::from("/home/pablitux/wasi-sdk/wasi-sdk-25.0-x86_64-linux");
                if wasi.exists() {
                    cmd.env("WASI_SDK_PATH", wasi);
                }
                let status = cmd.status()?;
                if !status.success() {
                    anyhow::bail!("dotnet build failed for {}", name);
                }
                // componentize-dotnet produces <name>.wasm in publish/.
                let mut found = None;
                for candidate in [
                    std::path::Path::new(&p.path).join(format!(
                        "bin/Release/net10.0/wasi-wasm/publish/{}.wasm",
                        name
                    )),
                    std::path::Path::new(&p.path)
                        .join(format!("bin/Release/net10.0/wasi-wasm/{}.wasm", name)),
                ] {
                    if candidate.exists() {
                        found = Some(candidate);
                        break;
                    }
                }
                if found.is_none() {
                    for entry in walkdir(std::path::Path::new(&p.path).join("bin")) {
                        if entry.extension().and_then(|s| s.to_str()) == Some("wasm")
                            && !entry.to_string_lossy().contains("component")
                        {
                            found = Some(entry);
                            break;
                        }
                    }
                }
                if let Some(src) = found {
                    let dst =
                        std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
                    std::fs::copy(&src, &dst)?;
                } else {
                    anyhow::bail!("dotnet.wasm not found after build");
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

    // File watcher for hot reload (best-effort)
    let base = format!("http://{}", http_addr);
    std::thread::spawn(move || {
        use inotify::{Inotify, WatchMask};
        let mut inotify = match Inotify::init() {
            Ok(i) => i,
            Err(_) => return,
        };
        // watch plugins dir
        let plugins_dir = std::path::Path::new("plugins");
        if plugins_dir.exists() {
            let _ = inotify.watches().add(
                plugins_dir,
                WatchMask::MODIFY | WatchMask::CLOSE_WRITE | WatchMask::CREATE,
            );
        }
        let mut buffer = [0u8; 4096];
        loop {
            if let Ok(events) = inotify.read_events_blocking(&mut buffer) {
                for ev in events {
                    if let Some(name) = ev.name {
                        let n = name.to_string_lossy();
                        if n.ends_with(".component.wasm") {
                            // derive plugin name from parent dir
                            // plugins/<plugin>/<name>.component.wasm
                            let plugin = n.trim_end_matches(".component.wasm").to_string();
                            println!("[dev] detected change: {} -> reloading {}", n, plugin);
                            let url = format!("{}/plugin/{}/reload", base, plugin);
                            let _ = reqwest::blocking::Client::new().post(&url).send();
                        }
                    }
                }
            }
        }
    });

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
    let raw = std::fs::read_to_string(config)?;
    let cfg: AppConfig = toml::from_str(&raw)?;
    let plugins = cfg.plugins.clone().unwrap_or_default();
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
        // checksum
        let comp = std::path::Path::new(&p.path).join(format!("{}.component.wasm", name));
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
    // Tauri may fail only on AppImage (linuxdeploy icon/strip); try manual fallback.
    if !status.success() {
        println!("Tauri build reported failure; attempting AppImage fallback...");
    }
    // Manual AppImage creation if AppDir exists.
    let appdir =
        std::path::Path::new("crates/desktop/target/release/bundle/appimage/Owasidea.AppDir");
    // cargo uses build dir /mnt/scratch; check there too
    let appdir_path = if appdir.exists() {
        Some(appdir.to_path_buf())
    } else {
        let alt = std::path::PathBuf::from(
            "/mnt/scratch/build-artifacts/cargo/release/bundle/appimage/Owasidea.AppDir",
        );
        if alt.exists() {
            Some(alt)
        } else {
            None
        }
    };
    if let Some(dir) = appdir_path {
        let icon = dir.join("icon.png");
        let src_icon = dir.join("Owasidea.png");
        if !icon.exists() && src_icon.exists() {
            std::fs::copy(&src_icon, &icon).ok();
        }
        let linuxdeploy =
            std::path::Path::new("/home/pablitux/.cache/tauri/linuxdeploy-x86_64.AppImage");
        if linuxdeploy.exists() {
            let mut cmd = std::process::Command::new(linuxdeploy);
            cmd.arg("--appdir")
                .arg(&dir)
                .arg("--output")
                .arg("appimage");
            cmd.env("APPIMAGE_EXTRACT_AND_RUN", "1");
            cmd.env("NO_STRIP", "1");
            let st = cmd.status()?;
            if st.success() {
                println!("AppImage created via linuxdeploy fallback.");
            }
        }
    }
    println!("Bundle complete.");
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

fn cmd_keygen(out: &Path) -> anyhow::Result<()> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    std::fs::create_dir_all(out)?;
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .map_err(|e| anyhow::anyhow!("keygen failed: {e}"))?;
    let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|e| anyhow::anyhow!("parse key failed: {e}"))?;
    let pub_bytes = keypair.public_key().as_ref();
    std::fs::write(out.join("owasidea.key"), base64_encode(pkcs8.as_ref()))?;
    std::fs::write(out.join("owasidea.pub"), base64_encode(pub_bytes))?;
    println!("generated {}/owasidea.key and owasidea.pub", out.display());
    Ok(())
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn cmd_sign(manifest_dir: &Path, plugin: &str, key_path: &Path) -> anyhow::Result<()> {
    use ring::signature::Ed25519KeyPair;
    let manifest_path = manifest_dir.join(format!("{}.manifest.json", plugin));
    let raw = std::fs::read_to_string(&manifest_path)?;
    let mut manifest: serde_json::Value = serde_json::from_str(&raw)?;
    let comp_rel = manifest["component"].as_str().unwrap_or("").to_string();
    let comp_path = manifest_path.parent().unwrap().join(&comp_rel);
    let bytes = std::fs::read(&comp_path)?;
    let key_b64 = std::fs::read_to_string(key_path)?;
    let key_bytes = base64_decode(key_b64.trim())?;
    let keypair = Ed25519KeyPair::from_pkcs8(&key_bytes)
        .map_err(|e| anyhow::anyhow!("invalid private key: {e}"))?;
    let sig = keypair.sign(&bytes);
    manifest["metadata"]["signature"] = serde_json::json!(base64_encode(sig.as_ref()));
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    println!("signed {} -> {}", plugin, manifest_path.display());
    Ok(())
}

fn base64_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))
}
