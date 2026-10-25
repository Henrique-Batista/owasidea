wit_bindgen::generate!({
    world: "attr-example",
    path: "wit",
    generate_all,
});

use poc::host::host_kv;
use poc::host::host_log;

#[owasidea::host_log]
#[owasidea::host_kv]
#[owasidea::world("attr-example")]
struct AttrExample;

impl Guest for AttrExample {
    #[owasidea::export]
    fn greet(name: String) -> String {
        host_log::log(&format!("greet called with {name}"));
        format!("Hello, {name}! (from attr-example)")
    }

    #[owasidea::export]
    fn compute(a: i32, b: i32) -> i32 {
        host_log::log(&format!("compute {a} + {b}"));
        a + b
    }

    #[owasidea::export]
    fn remember(key: String, value: String) -> String {
        host_kv::set(&key, &value);
        host_log::log(&format!("remembered {key}"));
        format!("stored {key}")
    }
}

export!(AttrExample);
