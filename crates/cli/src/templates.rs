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

pub const WIT_LIBRARY: &str = include_str!("../templates/plugin/wit/library.wit");

pub const RUST_CARGO: &str = include_str!("../templates/plugin/rust/Cargo.toml");
pub const RUST_LIB_SERVICE: &str = include_str!("../templates/plugin/rust/lib_service.rs");
pub const RUST_LIB: &str = include_str!("../templates/plugin/rust/lib.rs");

pub const GO_MOD: &str = include_str!("../templates/plugin/go/go.mod");
pub const GO_EXPORT_IMPL: &str = include_str!("../templates/plugin/go/wit_bindings.go");

pub const TS_PACKAGE_JSON: &str = include_str!("../templates/plugin/ts/package.json");
pub const TS_BUILD_SCRIPT: &str = include_str!("../templates/plugin/ts/build.mjs");
pub const TS_INDEX: &str = include_str!("../templates/plugin/ts/index.ts");

pub const CSHARP_EXECUTABLE_CSPROJ: &str =
    include_str!("../templates/plugin/csharp/executable.csproj");
pub const CSHARP_PROGRAM_CS: &str = include_str!("../templates/plugin/csharp/Program.cs");
pub const CSHARP_LIBRARY_CSPROJ: &str = include_str!("../templates/plugin/csharp/library.csproj");
pub const CSHARP_DIRECTORY_BUILD_PROPS: &str =
    include_str!("../templates/plugin/csharp/Directory.Build.props");
pub const CSHARP_IMPL: &str = include_str!("../templates/plugin/csharp/Impl.cs");
pub const CSHARP_NUGET_CONFIG: &str = include_str!("../templates/plugin/csharp/nuget.config");
pub const CSHARP_ATTRIBUTES: &str = include_str!("../templates/plugin/csharp/Attributes.cs");

pub const C_IMPL: &str = include_str!("../templates/plugin/c/impl.c");
pub const C_MAKEFILE: &str = include_str!("../templates/plugin/c/Makefile");

pub const CPP_IMPL: &str = include_str!("../templates/plugin/cpp/impl.cpp");

pub const PLUGIN_README: &str = include_str!("../templates/plugin/README.md");

pub const SVELTE_PACKAGE_JSON: &str = include_str!("../templates/plugin/ui/svelte/package.json");
pub const SVELTE_VITE_CONFIG: &str = include_str!("../templates/plugin/ui/svelte/vite.config.ts");
pub const SVELTE_TSCONFIG: &str = include_str!("../templates/plugin/ui/svelte/tsconfig.json");
pub const SVELTE_INDEX_HTML: &str = include_str!("../templates/plugin/ui/svelte/index.html");
pub const SVELTE_MAIN_TS: &str = include_str!("../templates/plugin/ui/svelte/src/main.ts");
pub const SVELTE_APP_SVELTE: &str = include_str!("../templates/plugin/ui/svelte/src/App.svelte");

pub const REACT_PACKAGE_JSON: &str = include_str!("../templates/plugin/ui/react/package.json");
pub const REACT_VITE_CONFIG: &str = include_str!("../templates/plugin/ui/react/vite.config.ts");
pub const REACT_TSCONFIG: &str = include_str!("../templates/plugin/ui/react/tsconfig.json");
pub const REACT_INDEX_HTML: &str = include_str!("../templates/plugin/ui/react/index.html");
pub const REACT_MAIN_TSX: &str = include_str!("../templates/plugin/ui/react/src/main.tsx");
pub const REACT_APP_TSX: &str = include_str!("../templates/plugin/ui/react/src/App.tsx");

pub const VUE_PACKAGE_JSON: &str = include_str!("../templates/plugin/ui/vue/package.json");
pub const VUE_VITE_CONFIG: &str = include_str!("../templates/plugin/ui/vue/vite.config.ts");
pub const VUE_TSCONFIG: &str = include_str!("../templates/plugin/ui/vue/tsconfig.json");
pub const VUE_INDEX_HTML: &str = include_str!("../templates/plugin/ui/vue/index.html");
pub const VUE_MAIN_TS: &str = include_str!("../templates/plugin/ui/vue/src/main.ts");
pub const VUE_APP_VUE: &str = include_str!("../templates/plugin/ui/vue/src/App.vue");

pub const YEW_CARGO_TOML: &str = include_str!("../templates/plugin/ui/yew/Cargo.toml");
pub const YEW_INDEX_HTML: &str = include_str!("../templates/plugin/ui/yew/index.html");
pub const YEW_MAIN_RS: &str = include_str!("../templates/plugin/ui/yew/src/main.rs");
pub const YEW_TRUNK_TOML: &str = include_str!("../templates/plugin/ui/yew/Trunk.toml");

pub const BLAZOR_CSPROJ: &str = include_str!("../templates/plugin/ui/blazor/app.csproj");
pub const BLAZOR_PROGRAM_CS: &str = include_str!("../templates/plugin/ui/blazor/Program.cs");
pub const BLAZOR_APP_RAZOR: &str = include_str!("../templates/plugin/ui/blazor/App.razor");
pub const BLAZOR_INDEX_HTML: &str = include_str!("../templates/plugin/ui/blazor/index.html");
pub const BLAZOR_WWWROOT_INDEX: &str =
    include_str!("../templates/plugin/ui/blazor/wwwroot/index.html");

pub const UNO_CSPROJ: &str = include_str!("../templates/plugin/ui/uno/app.csproj");
pub const UNO_INDEX_HTML: &str = include_str!("../templates/plugin/ui/uno/index.html");
