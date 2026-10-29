use serde_json::Value;
use std::collections::BTreeSet;

pub fn effective_permissions(m: &Value) -> Vec<String> {
    let mut perms: Vec<String> = Vec::new();
    if let Some(fs) = m["permissions"]["filesystem"].as_array() {
        for e in fs {
            perms.push(format!(
                "fs:{}:{}",
                e["access"].as_str().unwrap_or(""),
                e["path"].as_str().unwrap_or("")
            ));
        }
    }
    if let Some(entries) = m["permissions"]["kv"].as_array() {
        for kv in entries {
            perms.push(format!(
                "kv:{}:{}",
                kv["access"].as_str().unwrap_or(""),
                kv["namespace"].as_str().unwrap_or("")
            ));
        }
    }
    if m["permissions"]["http"]["outbound_any"]
        .as_bool()
        .unwrap_or(false)
    {
        perms.push("http:outbound-any".to_string());
    }
    if let Some(arr) = m["permissions"]["http"]["outbound"].as_array() {
        for h in arr {
            perms.push(format!("http:outbound:{}", h.as_str().unwrap_or("")));
        }
    }
    if m["permissions"]["network"]["outbound_any"]
        .as_bool()
        .unwrap_or(false)
    {
        perms.push("network:outbound-any".to_string());
    }
    if let Some(arr) = m["permissions"]["network"]["outbound"].as_array() {
        for h in arr {
            perms.push(format!("network:outbound:{}", h.as_str().unwrap_or("")));
        }
    }
    if let Some(arr) = m["permissions"]["network"]["inbound"].as_array() {
        for h in arr {
            perms.push(format!("network:inbound:{}", h.as_str().unwrap_or("")));
        }
    }
    if let Some(arr) = m["imports"]["host"].as_array() {
        for h in arr {
            perms.push(format!("import:host:{}", h.as_str().unwrap_or("")));
        }
    }
    if let Some(arr) = m["imports"]["wasi"].as_array() {
        for w in arr {
            perms.push(format!("import:wasi:{}", w.as_str().unwrap_or("")));
        }
    }
    perms.sort();
    perms.dedup();
    perms
}

pub fn diff(old: &[String], new: &[String]) -> (Vec<String>, Vec<String>) {
    let old_set: BTreeSet<String> = old.iter().cloned().collect();
    let new_set: BTreeSet<String> = new.iter().cloned().collect();
    let added: Vec<String> = new_set.difference(&old_set).cloned().collect();
    let removed: Vec<String> = old_set.difference(&new_set).cloned().collect();
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest_with(
        fs: &[&str],
        kv: Option<&str>,
        http: &[&str],
        net_out: &[&str],
        net_in: &[&str],
        host: &[&str],
        wasi: &[&str],
    ) -> Value {
        let fs_perms: Vec<Value> = fs
            .iter()
            .map(|v| {
                let (p, a) = if let Some((pp, aa)) = v.rsplit_once(':') {
                    (pp, aa)
                } else {
                    (*v, "read-write")
                };
                json!({"path": p, "access": a, "preopen": true})
            })
            .collect();
        let kv_val = kv.map(|k| {
            let (n, a) = if let Some((nn, aa)) = k.rsplit_once(':') {
                (nn, aa)
            } else {
                (k, "read-write")
            };
            vec![json!({"namespace": n, "access": a})]
        });
        json!({
            "permissions": {
                "filesystem": fs_perms,
                "http": {"outbound": http, "inbound": false},
                "network": {"outbound": net_out, "inbound": net_in},
                "kv": kv_val.unwrap_or_default()
            },
            "imports": {"host": host, "wasi": wasi}
        })
    }

    #[test]
    fn effective_permissions_flattens() {
        let m = manifest_with(
            &["/a:read"],
            Some("ns:read"),
            &["example.com"],
            &["1.2.3.4"],
            &["5.6.7.8"],
            &["poc:host/host-kv"],
            &["wasi:io"],
        );
        let p = effective_permissions(&m);
        assert!(p.contains(&"fs:read:/a".to_string()));
        assert!(p.contains(&"kv:read:ns".to_string()));
        assert!(p.contains(&"http:outbound:example.com".to_string()));
        assert!(p.contains(&"network:outbound:1.2.3.4".to_string()));
        assert!(p.contains(&"network:inbound:5.6.7.8".to_string()));
        assert!(!p.contains(&"network:outbound-any".to_string()));
        assert!(p.contains(&"import:host:poc:host/host-kv".to_string()));
        assert!(p.contains(&"import:wasi:wasi:io".to_string()));
    }

    #[test]
    fn network_outbound_preset_is_part_of_the_lock() {
        let manifest = json!({
            "permissions": {
                "filesystem": [],
                "http": {"outbound": [], "outbound_any": false},
                "network": {"outbound": [], "outbound_any": true, "inbound": []}
            },
            "imports": {"host": [], "wasi": []}
        });
        assert!(effective_permissions(&manifest).contains(&"network:outbound-any".to_string()));
    }

    #[test]
    fn diff_detects_added_and_removed() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["b".to_string(), "c".to_string()];
        let (added, removed) = diff(&old, &new);
        assert_eq!(added, vec!["c".to_string()]);
        assert_eq!(removed, vec!["a".to_string()]);
    }

    #[test]
    fn diff_no_change_empty() {
        let v = vec!["x".to_string()];
        let (a, r) = diff(&v, &v);
        assert!(a.is_empty());
        assert!(r.is_empty());
    }
}
