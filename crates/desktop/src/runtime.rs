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
    #[serde(default)]
    pub manifests_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    pub host_url: String,
    pub bind_addr: String,
    pub app: String,
    pub version: String,
    pub build_id: Option<String>,
    pub manifests_dir: Option<PathBuf>,
}

pub fn resolve(resource_dir: Option<PathBuf>) -> ResolvedRuntime {
    let file_path = std::env::var("OWASIDEA_RUNTIME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            resource_dir
                .as_ref()
                .map(|d| d.join("owasidea.runtime.json"))
        });

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
        .unwrap_or_else(|| "http://127.0.0.1:8787".to_string());

    let app = std::env::var("OWASIDEA_APP_NAME")
        .ok()
        .or_else(|| meta.app.clone())
        .unwrap_or_else(|| "Owasidea".to_string());

    let version = std::env::var("OWASIDEA_APP_VERSION")
        .ok()
        .or_else(|| meta.version.clone())
        .unwrap_or_else(|| "0.1.0".to_string());

    let build_id = std::env::var("OWASIDEA_BUILD_ID")
        .ok()
        .or(meta.build_id.clone());

    let bind_addr = std::env::var("HOST_ADDR").ok().unwrap_or_else(|| {
        host_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string()
    });

    let manifests_dir = std::env::var("OWASIDEA_MANIFESTS")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            meta.manifests_dir.as_ref().map(|m| {
                let p = PathBuf::from(m);
                if p.is_absolute() {
                    p
                } else {
                    resource_dir
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(p)
                }
            })
        });

    ResolvedRuntime {
        host_url,
        bind_addr,
        app,
        version,
        build_id,
        manifests_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resolves_defaults_without_file() {
        let r = resolve(None);
        assert!(r.host_url.starts_with("http://"));
        assert!(!r.bind_addr.is_empty());
    }

    #[test]
    fn file_and_overrides() {
        let dir = std::env::temp_dir().join("owasidea-runtime-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("owasidea.runtime.json");
        let mut f = std::fs::File::create(&p).unwrap();
        write!(f, r#"{{"host":"http://127.0.0.1:9999","app":"MyApp","version":"2.0.0","build_id":"abc","manifests_dir":"manifests"}}"#).unwrap();
        std::env::set_var("OWASIDEA_RUNTIME", p.to_str().unwrap());
        std::env::set_var("OWASIDEA_APP_VERSION", "3.0.0");
        let r = resolve(Some(dir.clone()));
        assert_eq!(r.host_url, "http://127.0.0.1:9999");
        assert_eq!(r.app, "MyApp");
        assert_eq!(r.version, "3.0.0");
        assert_eq!(r.build_id.as_deref(), Some("abc"));
        std::env::remove_var("OWASIDEA_RUNTIME");
        std::env::remove_var("OWASIDEA_APP_VERSION");
    }
}
