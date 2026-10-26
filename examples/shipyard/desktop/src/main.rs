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

fn manifests_dir(handle: &tauri::AppHandle) -> PathBuf {
    if let Some(path) = std::env::var_os("OWASIDEA_MANIFESTS") {
        return PathBuf::from(path);
    }
    let resource = handle.path().resource_dir().ok();
    let exe = exe_dir();
    let build_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../manifests");
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
        "shipyard: manifests not found; tried: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    build_tree
}

fn trust_key(handle: &tauri::AppHandle) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("OWASIDEA_PUBLIC_KEY") {
        return Some(PathBuf::from(path));
    }
    let resource = handle.path().resource_dir().ok();
    let exe = exe_dir();
    let build_tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../keys/owasidea.pub");
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
        .setup(|app| {
            let handle = app.handle().clone();

            let manifests = manifests_dir(&handle);
            let data_dir = handle.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            std::env::set_var("OWASIDEA_DATA_DIR", &data_dir);

            // Release builds require signed manifests. Find the trust key next
            // to the executable, in Tauri resources, or in the build tree when
            // running the binary directly from an external target directory.
            std::env::set_var("OWASIDEA_REQUIRE_SIGNATURES", "true");
            match trust_key(&handle) {
                Some(path) => {
                    std::env::set_var("OWASIDEA_PUBLIC_KEY", &path);
                    eprintln!("shipyard trust key: {}", path.display());
                }
                None => {
                    eprintln!(
                        "shipyard: trust key not found; set OWASIDEA_PUBLIC_KEY or bundle trust/owasidea.pub"
                    );
                }
            }

            eprintln!("shipyard manifests: {}", manifests.display());
            eprintln!("shipyard data: {}", data_dir.display());

            let addr = std::env::var("HOST_ADDR").unwrap_or_else(|_| "127.0.0.1:8790".to_string());
            tauri::async_runtime::spawn(async move {
                if let Err(error) = owasidea_host::run(manifests, addr).await {
                    eprintln!("shipyard host error: {error}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Shipyard");
}
