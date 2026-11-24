//! CLI/headless bundle: a runnable directory with the host binary, manifests,
//! components and a launcher. No Tauri, no desktop shell.

use std::path::Path;

pub fn cmd_bundle_cli(root: &Path, config: &Path, output: &Path) -> anyhow::Result<()> {
    if output.exists() {
        std::fs::remove_dir_all(output)?;
    }
    std::fs::create_dir_all(output)?;

    let exe_name = if cfg!(windows) {
        "owasidea.exe"
    } else {
        "owasidea"
    };
    let exe = std::env::current_exe()?;
    std::fs::copy(&exe, output.join(exe_name))?;
    std::fs::copy(config, output.join("wasm-app.toml"))?;

    let staged = crate::dist_stage::stage_dist(root, output)?;
    rewrite_native_entries(output)?;

    let launcher = output.join(if cfg!(windows) { "run.bat" } else { "run.sh" });
    if cfg!(windows) {
        std::fs::write(
            &launcher,
            "@echo off\r\ncd /d \"%~dp0\"\r\nowasidea.exe --config wasm-app.toml run %*\r\n",
        )?;
    } else {
        std::fs::write(
            &launcher,
            "#!/bin/sh\nset -eu\ncd \"$(dirname \"$0\")\"\nexec ./owasidea --config wasm-app.toml run \"$@\"\n",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&launcher)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&launcher, permissions)?;
        }
    }

    std::fs::write(
        output.join("README.txt"),
        "CLI bundle\n\
         \n\
         Run ./run.sh (or run.bat on Windows). It starts the host with the\n\
         generated manifests and components in this directory.\n\
         \n\
         The HTTP API listens on [host].addr from wasm-app.toml (loopback by\n\
         default). Use `owasidea call` or any HTTP client to invoke exports.\n\
         \n\
         Native UI binaries live under native/ and start with the app; pass\n\
         --no-native-ui to run only the host.\n",
    )?;

    println!("CLI bundle: {}", output.display());
    println!(
        "  {} manifest(s), {} component(s), {} native UI(s), UI {}",
        staged.manifests,
        staged.components,
        staged.native_uis,
        if staged.ui { "staged" } else { "absent" }
    );
    println!("  launcher: {}", launcher.display());
    Ok(())
}

/// Point the bundled config at the staged native UI copies. The launcher then
/// starts them through the normal `owasidea run` path.
fn rewrite_native_entries(output: &Path) -> anyhow::Result<()> {
    let index_path = output.join("manifests").join("native-ui.json");
    if !index_path.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&index_path)?;
    let index: crate::native_ui::Index = serde_json::from_str(&raw)?;
    let config_path = output.join("wasm-app.toml");
    let mut config: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
    for entry in &index.plugins {
        if let Some(plugin) = config
            .get_mut("plugins")
            .and_then(toml::Value::as_table_mut)
            .and_then(|plugins| plugins.get_mut(&entry.name))
            .and_then(toml::Value::as_table_mut)
        {
            plugin.insert(
                "native_entry".to_string(),
                toml::Value::String(entry.binary.clone()),
            );
        }
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
