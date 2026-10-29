wit_bindgen::generate!({
    world: "core",
    path: "wit",
    generate_all,
});

use poc::host::host_kv;
use poc::host::host_log;
use std::fs;

fn score_release(changes: &[String], checks_passed: u32, checks_total: u32) -> i32 {
    let change_points = (changes.len() as i32 * 4).min(60);
    let check_points = if checks_total == 0 {
        0
    } else {
        ((checks_passed.min(checks_total) as i32) * 40) / checks_total as i32
    };
    let mut score = change_points + check_points;
    for change in changes {
        let lower = change.to_lowercase();
        if lower.contains("breaking") || lower.contains("security") {
            score -= 12;
        }
    }
    score.clamp(0, 100)
}

fn safe_key(key: &str) -> Option<String> {
    let key: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    (!key.is_empty()).then_some(key)
}

struct Core;

impl Guest for Core {
    fn ping() -> String {
        host_log::log("core-rust ping");
        "shipyard core online".to_string()
    }
}

impl exports::shipyard::core::readiness::Guest for Core {
    fn score(changes: Vec<String>, checks_passed: u32, checks_total: u32) -> i32 {
        let score = score_release(&changes, checks_passed, checks_total);
        host_log::log(&format!(
            "scored release: {} changes, {}/{} checks -> {}",
            changes.len(),
            checks_passed,
            checks_total,
            score
        ));
        score
    }

    fn verdict(score: i32) -> String {
        match score {
            85..=100 => "ship it".to_string(),
            65..=84 => "review required".to_string(),
            40..=64 => "hold for hardening".to_string(),
            _ => "blocked".to_string(),
        }
    }

    fn save_report(key: String, content: String) -> Result<String, String> {
        let key =
            safe_key(&key).ok_or_else(|| "report key is empty after sanitizing".to_string())?;
        let path = format!("reports/{key}.md");
        fs::write(&path, &content).map_err(|error| format!("write {path}: {error}"))?;
        let kv = host_kv::open_kv("shipyard-core")?;
        kv.set(&format!("report:{key}"), &content)?;
        host_log::log(&format!("saved report {path} ({} bytes)", content.len()));
        Ok(path)
    }

    fn load_report(key: String) -> Option<String> {
        let key = safe_key(&key)?;
        let value = host_kv::open_kv("shipyard-core")
            .ok()
            .and_then(|kv| kv.get(&format!("report:{key}")));
        host_log::log(&format!("loaded report {key}: {}", value.is_some()));
        value
    }
}

export!(Core);
