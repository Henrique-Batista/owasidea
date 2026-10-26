wit_bindgen::generate!({
    world: "heartbeat",
    path: "wit",
});

use std::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);

struct Heartbeat;

impl Guest for Heartbeat {
    fn start_service() -> Result<String, String> {
        RUNNING.store(true, Ordering::SeqCst);
        Ok("heartbeat service started".to_string())
    }

    fn service_status() -> bool {
        RUNNING.load(Ordering::SeqCst)
    }
}

export!(Heartbeat);
