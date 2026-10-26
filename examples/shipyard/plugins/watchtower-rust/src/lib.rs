wit_bindgen::generate!({
    world: "watchtower",
    path: "wit",
    generate_all,
});

use poc::host::host_log;
use std::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);

struct Watchtower;

impl Guest for Watchtower {
    fn start_service() -> Result<String, String> {
        RUNNING.store(true, Ordering::SeqCst);
        host_log::log("watchtower service started");
        Ok("watchtower armed; lifecycle instance is persistent".to_string())
    }

    fn service_status() -> bool {
        let running = RUNNING.load(Ordering::SeqCst);
        host_log::log(&format!("watchtower status queried: {running}"));
        running
    }
}

export!(Watchtower);
