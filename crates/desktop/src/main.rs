use owasidea_host;
use tauri::Manager;

fn main() {
    // Start the embedded host in the background.
    tauri::async_runtime::spawn(async {
        let manifests = std::env::var("OWASIDEA_MANIFESTS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("../manifests"));
        let addr = std::env::var("HOST_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
        if let Err(e) = owasidea_host::run(manifests, addr).await {
            eprintln!("host error: {e}");
        }
    });

    tauri::Builder::default()
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
