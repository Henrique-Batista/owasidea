wit_bindgen::generate!({ world: "{{name}}", path: ".owasidea/wit" });

use std::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);

#[owasidea::world("{{name}}")]
struct Component;

impl Guest for Component {
    #[owasidea::export]
    fn start_service() -> Result<String, String> {
        RUNNING.store(true, Ordering::SeqCst);
        Ok("service started".to_string())
    }

    #[owasidea::export]
    fn service_status() -> bool {
        RUNNING.load(Ordering::SeqCst)
    }
}

export!(Component);
