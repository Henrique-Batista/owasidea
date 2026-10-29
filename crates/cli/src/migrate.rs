use anyhow::{Context, Result};
use std::path::Path;

pub fn cmd_migrate(path: &Path, write: bool, json: bool) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    let (mut migrated, mut changed) = migrate_kv_text(&raw);
    if !changed {
        if let Ok(mut value) = toml::from_str::<toml::Value>(&raw) {
            if migrate_kv_value(&mut value) {
                migrated = toml::to_string_pretty(&value)? + "\n";
                changed = true;
            }
        }
    }
    toml::from_str::<toml::Value>(&migrated)
        .with_context(|| format!("invalid TOML config {}", path.display()))?;
    if write && changed {
        std::fs::write(path, &migrated)?;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({
                "changed": changed,
                "written": write && changed,
                "requires_review": ["Declare each local WIT dependency in plugins.<name>.dependencies; dependencies are never inferred."],
            })
        );
    } else if changed {
        println!("1.1 KV string converted to one-entry list.");
        if write {
            println!("Updated {}.", path.display());
        } else {
            println!("No files changed. Re-run with --write to apply.");
        }
        println!(
            "Review WIT imports and declare local dependencies in plugins.<name>.dependencies."
        );
    } else {
        println!("No supported 1.1 config forms found; no files changed.");
        println!(
            "Review WIT imports and declare local dependencies in plugins.<name>.dependencies."
        );
    }
    Ok(())
}

/// Rewrite only `kv = "ns:access"` lines inside `[plugins.*]`. Textual editing
/// keeps comments and key order; a full TOML round-trip would drop both.
fn migrate_kv_text(raw: &str) -> (String, bool) {
    let mut out = String::with_capacity(raw.len() + 16);
    let mut in_plugins = false;
    let mut changed = false;
    for line in raw.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let suffix = &line[body.len()..];
        let trimmed = body.trim_start();
        if trimmed.starts_with('[') {
            in_plugins = trimmed.starts_with("[plugins.");
        }
        let indent = &body[..body.len() - trimmed.len()];
        if in_plugins {
            if let Some((value, rest)) = legacy_kv_value(trimmed) {
                changed = true;
                out.push_str(&format!("{indent}kv = [\"{value}\"]{rest}{suffix}"));
                continue;
            }
        }
        out.push_str(line);
    }
    (out, changed)
}

/// Fallback for inline tables and quoted keys, where a line-oriented rewrite
/// cannot preserve structure. Comments are lost only in these exotic forms.
fn migrate_kv_value(config: &mut toml::Value) -> bool {
    let Some(plugins) = config
        .get_mut("plugins")
        .and_then(toml::Value::as_table_mut)
    else {
        return false;
    };
    let mut changed = false;
    for plugin in plugins
        .iter_mut()
        .filter_map(|(_, value)| value.as_table_mut())
    {
        let Some(toml::Value::String(value)) = plugin.get("kv").cloned() else {
            continue;
        };
        plugin.insert(
            "kv".to_string(),
            toml::Value::Array(vec![toml::Value::String(value)]),
        );
        changed = true;
    }
    changed
}

fn legacy_kv_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    if key.trim() != "kv" {
        return None;
    }
    let value = value.trim_start();
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &value[quote.len_utf8()..];
    let end = value.find(quote)?;
    let rest = &value[end + 1..];
    if rest.trim_start().starts_with('#') || rest.trim().is_empty() {
        Some((&value[..end], rest))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_legacy_kv_once_and_preserves_comments_and_order() {
        let raw = "# top comment\n[plugins.notes]\npath = 'plugins/notes' # keep\nkv = \"notes:read-write\"\n";
        let (once, changed) = migrate_kv_text(raw);
        assert!(changed);
        assert!(once.contains("# top comment"));
        assert!(once.contains("path = 'plugins/notes' # keep"));
        assert!(once.contains("kv = [\"notes:read-write\"]"));
        let (twice, changed) = migrate_kv_text(&once);
        assert!(!changed);
        assert_eq!(twice, once);
    }

    #[test]
    fn fallback_converts_inline_tables_and_quoted_keys() {
        let mut config: toml::Value = toml::from_str(
            "[plugins]\nnotes = { path = 'plugins/notes', language = 'rust', \"kv\" = \"notes:read\" }\n",
        )
        .unwrap();
        assert!(migrate_kv_value(&mut config));
        assert_eq!(
            config["plugins"]["notes"]["kv"][0].as_str(),
            Some("notes:read")
        );
    }

    #[test]
    fn does_not_touch_other_tables_or_non_string_kv() {
        let raw = "[app]\nkv = \"top:read\"\n[plugins.notes]\nkv = [\"notes:read-write\"]\n";
        let (out, changed) = migrate_kv_text(raw);
        assert!(!changed);
        assert_eq!(out, raw);
    }

    #[test]
    fn dry_run_preserves_config_and_write_leaves_kv_data_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wasm-app.toml");
        let original = "[plugins.notes]\npath='plugins/notes'\nkv = \"notes:read-write\"\n";
        std::fs::write(&config_path, original).unwrap();
        let db = dir.path().join("data/notes/kv.sqlite");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::write(&db, b"existing database bytes").unwrap();

        cmd_migrate(&config_path, false, true).unwrap();
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), original);
        cmd_migrate(&config_path, true, true).unwrap();

        let migrated = std::fs::read_to_string(config_path).unwrap();
        assert!(
            migrated.contains("kv=[\"notes:read-write\"]")
                || migrated.contains("kv = [\"notes:read-write\"]")
        );
        assert_eq!(std::fs::read(db).unwrap(), b"existing database bytes");
    }
}
