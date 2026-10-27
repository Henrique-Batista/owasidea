mod runtime;
mod updater;

use runtime::resolve;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            updater::updater_check,
            updater::updater_install
        ])
        .setup(|app| {
            let resource_dir = app.path().resource_dir().ok();
            let resolved = resolve(resource_dir);

            if let Some(bid) = &resolved.build_id {
                std::env::set_var("OWASIDEA_BUILD_ID", bid);
            }
            if std::env::var("OWASIDEA_DEV_NO_HOST").as_deref() != Ok("1") {
                let manifests = resolved
                    .manifests_dir
                    .clone()
                    .unwrap_or_else(|| std::path::PathBuf::from("../manifests"));
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
