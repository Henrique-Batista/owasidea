wit_bindgen::generate!({
    world: "notes",
    path: "wit",
    generate_all,
});

use poc::host::host_kv;
use poc::host::host_log;

struct Notes;

impl Guest for Notes {
    fn greet(name: String) -> String {
        host_log::log(&format!("greet called with {name}"));
        format!("Hello, {name}! (from the Rust notes component)")
    }

    fn complete(prompt: String) -> Result<String, String> {
        host_log::log(&format!("complete called with: {prompt}"));
        let key = format!("cache:{prompt}");
        if let Some(cached) = host_kv::get(&key) {
            host_log::log("cache hit");
            return Ok(format!("[cached] {cached}"));
        }
        let result = format!("Notes response to: {prompt}");
        host_kv::set(&key, &result);
        Ok(result)
    }
}

export!(Notes);
