mod runtime;
mod updater;

use runtime::resolve;
use std::path::{Path, PathBuf};

use tauri::Manager;

fn first_dir(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_dir())
}

fn first_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// Resolve `$RESOURCE/manifests` for a packaged app, then the executable
/// directory, then the build tree when running the binary directly.
fn manifests_dir(handle: &tauri::AppHandle) -> PathBuf {
    if let Some(path) = std::env::var_os("OWASIDEA_MANIFESTS") {
        return PathBuf::from(path);
    }
    let resource = handle.path().resource_dir().ok();
    let exe = exe_dir();
    let build_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../manifests");
    let candidates = resource
        .iter()
        .flat_map(|dir| [dir.join("manifests"), dir.join("resources/manifests")])
        .chain(
            exe.iter()
                .flat_map(|dir| [dir.join("manifests"), dir.join("resources/manifests")]),
        )
        .chain(std::iter::once(build_tree.clone()))
        .collect::<Vec<_>>();
    if let Some(path) = first_dir(candidates.iter().cloned()) {
        return path;
    }
    eprintln!(
        "owasidea: manifests not found; tried: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    build_tree
}

/// Resolve the release trust key from the environment, Tauri resources, the
/// executable directory, or the build tree.
fn trust_key(handle: &tauri::AppHandle) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("OWASIDEA_PUBLIC_KEY") {
        return Some(PathBuf::from(path));
    }
    let resource = handle.path().resource_dir().ok();
    let exe = exe_dir();
    let build_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../keys/owasidea.pub");
    first_file(
        resource
            .iter()
            .flat_map(|dir| {
                [
                    dir.join("trust/owasidea.pub"),
                    dir.join("resources/trust/owasidea.pub"),
                ]
            })
            .chain(exe.iter().flat_map(|dir| {
                [
                    dir.join("trust/owasidea.pub"),
                    dir.join("resources/trust/owasidea.pub"),
                ]
            }))
            .chain(std::iter::once(build_tree)),
    )
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            updater::updater_check,
            updater::updater_install
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let resource_dir = handle.path().resource_dir().ok();
            let resolved = resolve(resource_dir);

            // Mutable host state (KV SQLite, relative fs preopens) must never
            // resolve against the process working directory.
            let data_dir = handle.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            std::env::set_var("OWASIDEA_DATA_DIR", &data_dir);

            if let Some(bid) = &resolved.build_id {
                std::env::set_var("OWASIDEA_BUILD_ID", bid);
            }
            match trust_key(&handle) {
                Some(path) => {
                    std::env::set_var("OWASIDEA_PUBLIC_KEY", &path);
                    eprintln!("owasidea trust key: {}", path.display());
                }
                None => {
                    eprintln!(
                        "owasidea: trust key not found; set OWASIDEA_PUBLIC_KEY or bundle trust/owasidea.pub"
                    );
                }
            }

            let manifests = resolved
                .manifests_dir
                .clone()
                .unwrap_or_else(|| manifests_dir(&handle));
            eprintln!("owasidea manifests: {}", manifests.display());
            eprintln!("owasidea data: {}", data_dir.display());

            if std::env::var("OWASIDEA_DEV_NO_HOST").as_deref() != Ok("1") {
                let addr = resolved.bind_addr.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = owasidea_host::run(manifests, addr).await {
                        eprintln!("host error: {e}");
                    }
                });
            }

            let script_json = serde_json::json!({
                "host": resolved.host_url,
                "app": resolved.app,
                "version": resolved.version,
                "build_id": resolved.build_id,
            });
            let script_payload = serde_json::to_string(&script_json).unwrap_or_else(|_| "{}".to_string());
            let script = format!(
                "if (window.location.origin === 'tauri://localhost' || window.location.origin === 'https://tauri.localhost' || window.location.origin.startsWith('http://localhost') || window.location.origin.startsWith('http://127.0.0.1')) {{ window.__OWASIDEA__ = {}; }}",
                script_payload
            );

            let window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Owasidea")
            .inner_size(1024.0, 768.0)
            .initialization_script(script)
            .build()?;
            let _ = window;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
