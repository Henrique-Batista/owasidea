use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RuntimeMeta {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,

}

#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    pub host_url: String,
    pub bind_addr: String,
    pub app: String,
    pub version: String,
    pub build_id: Option<String>,
}

pub fn resolve(resource_dir: Option<PathBuf>) -> ResolvedRuntime {
    let file_path = std::env::var("OWASIDEA_RUNTIME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| resource_dir.as_ref().map(|d| d.join("owasidea.runtime.json")));

    let mut meta = RuntimeMeta::default();
    if let Some(p) = &file_path {
        if let Ok(raw) = std::fs::read_to_string(p) {
            if let Ok(m) = serde_json::from_str::<RuntimeMeta>(&raw) {
                meta = m;
            }
        }
    }

    let host_url = std::env::var("OWASIDEA_HOST_URL")
        .ok()
        .or_else(|| meta.host.clone())
        .unwrap_or_else(|| "http://127.0.0.1:8790".to_string());

    let app = std::env::var("OWASIDEA_APP_NAME")
        .ok()
        .or_else(|| meta.app.clone())
        .unwrap_or_else(|| "Shipyard".to_string());

    let version = std::env::var("OWASIDEA_APP_VERSION")
        .ok()
        .or_else(|| meta.version.clone())
        .unwrap_or_else(|| "0.1.0".to_string());

    let build_id = std::env::var("OWASIDEA_BUILD_ID")
        .ok()
        .or(meta.build_id.clone());

    let bind_addr = std::env::var("HOST_ADDR")
        .ok()
        .unwrap_or_else(|| {
            host_url
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .trim_end_matches('/')
                .to_string()
        });

    ResolvedRuntime {
        host_url,
        bind_addr,
        app,
        version,
        build_id,
    }
}
