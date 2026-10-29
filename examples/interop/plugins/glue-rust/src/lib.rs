wit_bindgen::generate!({
    world: "glue",
    path: "wit",
    generate_all,
});

use poc::host::host_kv;

struct Glue;

impl Guest for Glue {
    fn call_provider(value: String) -> String {
        interop::provider::catalog::echo(&value)
    }

    fn remember(key: String, value: String) -> Result<String, String> {
        let kv = host_kv::open_kv("app-state")?;
        kv.set(&key, &value)?;
        Ok(format!("saved {key}"))
    }

    fn recall(key: String) -> Option<String> {
        host_kv::open_kv("app-state")
            .ok()
            .and_then(|kv| kv.get(&key))
    }

    fn cache_read_only() -> bool {
        let Ok(kv) = host_kv::open_kv("catalog-cache") else {
            return false;
        };
        kv.set("probe", "denied").is_err()
    }
}

export!(Glue);
