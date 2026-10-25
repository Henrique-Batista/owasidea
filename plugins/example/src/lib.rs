wit_bindgen::generate!({
    world: "example",
    path: "wit",
    generate_all,
});

use poc::host::host_kv;
use poc::host::host_log;

struct Example;

impl Guest for Example {
    fn greet(name: String) -> String {
        host_log::log(&format!("greet called with {name}"));
        format!("Hello, {name}! (from example)")
    }

    fn compute(a: i32, b: i32) -> i32 {
        host_log::log(&format!("compute {a} + {b}"));
        a + b
    }

    fn remember(key: String, value: String) -> String {
        host_kv::set(&key, &value);
        host_log::log(&format!("remembered {key}"));
        format!("stored {key}")
    }
}

export!(Example);