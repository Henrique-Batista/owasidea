wit_bindgen::generate!({ world: "ticker", path: "wit" });

use std::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);

struct Component;

impl Guest for Component {
    fn start_service() -> Result<String, String> {
        RUNNING.store(true, Ordering::SeqCst);
        Ok("ticker started".to_string())
    }

    fn service_status() -> bool {
        RUNNING.load(Ordering::SeqCst)
    }
}

export!(Component);
