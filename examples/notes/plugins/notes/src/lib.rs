wit_bindgen::generate!({ world: "notes", path: "wit", generate_all });

use poc::host::host_kv;

const KEY: &str = "notes";

fn load() -> Vec<String> {
    host_kv::get(KEY)
        .map(|raw| {
            raw.lines()
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn save(notes: &[String]) {
    host_kv::set(KEY, &notes.join("\n"));
}

struct Component;

impl Guest for Component {
    fn add_note(text: String) -> u32 {
        let mut notes = load();
        let text = text.replace(['\n', '\r'], " ").trim().to_string();
        if !text.is_empty() {
            notes.push(text);
            save(&notes);
        }
        notes.len() as u32
    }

    fn list_notes() -> String {
        load().join("\n")
    }

    fn clear_notes() -> u32 {
        save(&[]);
        0
    }
}

export!(Component);
