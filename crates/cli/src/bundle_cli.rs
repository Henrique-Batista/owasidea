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
         default). Use `owasidea call` or any HTTP client to invoke exports.\n",
    )?;

    println!("CLI bundle: {}", output.display());
    println!(
        "  {} manifest(s), {} component(s), UI {}",
        staged.manifests,
        staged.components,
        if staged.ui { "staged" } else { "absent" }
    );
    println!("  launcher: {}", launcher.display());
    Ok(())
}
