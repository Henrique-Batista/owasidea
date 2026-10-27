pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

pub const INIT_WASM_APP_TOML: &str = include_str!("../templates/init/wasm-app.toml");

pub const UI_INDEX_HTML: &str = include_str!("../templates/plugin/ui/index.html");
pub const UI_README: &str = include_str!("../templates/plugin/ui/README.md");

pub const WIT_SERVICE: &str = include_str!("../templates/plugin/wit/service.wit");
pub const WIT_LIBRARY: &str = include_str!("../templates/plugin/wit/library.wit");

pub const RUST_CARGO: &str = include_str!("../templates/plugin/rust/Cargo.toml");
pub const RUST_LIB_SERVICE: &str = include_str!("../templates/plugin/rust/lib_service.rs");
pub const RUST_LIB: &str = include_str!("../templates/plugin/rust/lib.rs");

pub const GO_MOD: &str = include_str!("../templates/plugin/go/go.mod");
pub const GO_MAIN: &str = include_str!("../templates/plugin/go/main.go");

pub const TS_PACKAGE_JSON: &str = include_str!("../templates/plugin/ts/package.json");
pub const TS_INDEX: &str = include_str!("../templates/plugin/ts/index.ts");

pub const CSHARP_EXECUTABLE_CSPROJ: &str =
    include_str!("../templates/plugin/csharp/executable.csproj");
pub const CSHARP_PROGRAM_CS: &str = include_str!("../templates/plugin/csharp/Program.cs");
pub const CSHARP_LIBRARY_CSPROJ: &str = include_str!("../templates/plugin/csharp/library.csproj");
pub const CSHARP_WIT: &str = include_str!("../templates/plugin/csharp/wit.wit");
pub const CSHARP_HOST_WIT: &str = include_str!("../templates/plugin/csharp/host.wit");
pub const CSHARP_IMPL: &str = include_str!("../templates/plugin/csharp/Impl.cs");
pub const CSHARP_NUGET_CONFIG: &str = include_str!("../templates/plugin/csharp/nuget.config");

pub const C_WIT: &str = include_str!("../templates/plugin/c/wit.wit");
pub const C_IMPL: &str = include_str!("../templates/plugin/c/impl.c");
pub const C_MAKEFILE: &str = include_str!("../templates/plugin/c/Makefile");

pub const CPP_IMPL: &str = include_str!("../templates/plugin/cpp/impl.cpp");

pub const PLUGIN_README: &str = include_str!("../templates/plugin/README.md");
