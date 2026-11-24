use std::sync::atomic::{AtomicU32, Ordering};

wit_bindgen::generate!({ world: "counter", path: ".owasidea/wit", generate_all });

static VALUE: AtomicU32 = AtomicU32::new(0);

#[owasidea::world("counter")]
struct Component;

impl Guest for Component {
    #[owasidea::export]
    fn increment(by: u32) -> u32 {
        VALUE.fetch_add(by, Ordering::SeqCst) + by
    }

    #[owasidea::export]
    fn value() -> u32 {
        VALUE.load(Ordering::SeqCst)
    }
}

export!(Component);
