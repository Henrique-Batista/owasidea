use anyhow::Context;
use std::path::Path;

pub fn cmd_verify(dist: &Path, json: bool) -> anyhow::Result<()> {
    use base64::Engine;
    use serde::Serialize;
    use sha2::{Digest, Sha256};

    #[derive(Serialize, Clone)]
    struct ErrObj {
        file: String,
        code: String,
        message: String,
    }
    #[derive(Serialize)]
    struct Report {
        ok: bool,
        errors: Vec<ErrObj>,
        checked: usize,
    }

    let mut errors: Vec<ErrObj> = Vec::new();
    let mut checked = 0usize;
    let mut push_err = |file: String, code: &str, message: String| {
        errors.push(ErrObj {
            file,
            code: code.to_string(),
            message,
        });
    };

    if !dist.is_dir() {
        push_err(
            dist.display().to_string(),
            "NOT_DIR",
            "dist path is not a directory".to_string(),
        );
        let report = Report {
            ok: false,
            errors,
            checked,
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        anyhow::bail!("verification failed");
    }

    let manifest_dir = if dist.join("manifests").is_dir() {
        dist.join("manifests")
    } else {
        dist.to_path_buf()
    };

    let mut manifest_files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&manifest_dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".manifest.json") {
                manifest_files.push(entry.path());
            }
        }
    }
    manifest_files.sort();
    if manifest_files.is_empty() {
        push_err(
            manifest_dir.display().to_string(),
            "NO_MANIFESTS",
            "no *.manifest.json found; run `owasidea bundle` (it stages manifests into dist) or point verify at a manifest directory"
                .to_string(),
        );
    }

    // public key
    let pk_path = if let Ok(p) = std::env::var("OWASIDEA_PUBLIC_KEY") {
        Some(std::path::PathBuf::from(p))
    } else if dist.join("trust/owasidea.pub").is_file() {
        Some(dist.join("trust/owasidea.pub"))
    } else if dist.join("keys/owasidea.pub").is_file() {
        Some(dist.join("keys/owasidea.pub"))
    } else {
        None
    };
    let public_key_bytes: Option<Vec<u8>> = pk_path.as_ref().and_then(|p| {
        std::fs::read_to_string(p).ok().and_then(|s| {
            base64::engine::general_purpose::STANDARD
                .decode(s.trim())
                .ok()
        })
    });

    let ui_bases = [
        dist.join("ui/public"),
        dist.join("ui/dist"),
        dist.join("ui"),
        dist.join("public"),
        dist.to_path_buf(),
    ];

    for mpath in &manifest_files {
        checked += 1;
        let rel_file = mpath
            .strip_prefix(dist)
            .unwrap_or(mpath)
            .display()
            .to_string();

        let raw = match std::fs::read_to_string(mpath) {
            Ok(r) => r,
            Err(e) => {
                push_err(
                    rel_file.clone(),
                    "READ_ERROR",
                    format!("read manifest: {e}"),
                );
                continue;
            }
        };
        let manifest: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                push_err(
                    rel_file.clone(),
                    "INVALID_JSON",
                    format!("invalid json: {e}"),
                );
                continue;
            }
        };

        // CT-08 ABI
        let sv = manifest["schema_version"].as_u64().unwrap_or(0);
        if sv == 1 {
            match manifest["host_abi"].as_str() {
                Some(ha) if ha == owasidea_host::HOST_ABI => {}
                Some(ha) => push_err(
                    rel_file.clone(),
                    "HOST_ABI_MISMATCH",
                    format!("host_abi {} != {}", ha, owasidea_host::HOST_ABI),
                ),
                None => push_err(
                    rel_file.clone(),
                    "HOST_ABI_MISSING",
                    "host_abi required for schema_version 1".to_string(),
                ),
            }
            match manifest["min_host"].as_str() {
                Some(mh) => {
                    let required = semver::Version::parse(mh).ok();
                    let host = semver::Version::parse(owasidea_host::HOST_VERSION).ok();
                    let bad = match (required, host) {
                        (Some(r), Some(h)) => r > h,
                        _ => true,
                    };
                    if bad {
                        push_err(
                            rel_file.clone(),
                            "MIN_HOST",
                            format!("min_host {} > host {}", mh, owasidea_host::HOST_VERSION),
                        );
                    }
                }
                None => push_err(
                    rel_file.clone(),
                    "MIN_HOST_MISSING",
                    "min_host required for schema_version 1".to_string(),
                ),
            }
        } else if sv > 1 {
            push_err(
                rel_file.clone(),
                "SCHEMA_VERSION",
                format!("unsupported schema_version {}", sv),
            );
        }

        let runtime = manifest["runtime"].as_str().unwrap_or("");
        let name = manifest["name"].as_str().unwrap_or("").to_string();

        if runtime == "js-wasm" {
            // UI manifest validation
            let ui = &manifest["metadata"]["ui"];
            let entry = ui.get("entry").and_then(|v| v.as_str());
            if entry.is_none() {
                push_err(
                    rel_file.clone(),
                    "UI_ENTRY_MISSING",
                    "missing metadata.ui.entry".to_string(),
                );
            }
            let files_map = ui.get("files").cloned().unwrap_or(serde_json::json!({}));
            let files: std::collections::BTreeMap<String, String> =
                serde_json::from_value(files_map.clone()).unwrap_or_default();
            if files.is_empty() {
                push_err(
                    rel_file.clone(),
                    "UI_FILES_EMPTY",
                    "UI plugin must list files".to_string(),
                );
            }
            if let Some(entry) = entry {
                if !files.contains_key(entry) {
                    push_err(
                        rel_file.clone(),
                        "UI_ENTRY_NOT_IN_FILES",
                        "entry not in files".to_string(),
                    );
                }
            }
            for (path, digest) in &files {
                if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                    push_err(
                        rel_file.clone(),
                        "UI_DIGEST_INVALID",
                        format!("invalid digest for {path}"),
                    );
                }
                // asset exists and hash matches
                let mut found: Option<std::path::PathBuf> = None;
                for base in &ui_bases {
                    let c = base.join(path);
                    if c.is_file() {
                        found = Some(c);
                        break;
                    }
                    let c2 = base.join("plugins").join(&name).join(path);
                    if c2.is_file() {
                        found = Some(c2);
                        break;
                    }
                }
                match found {
                    Some(p) => {
                        let bytes = std::fs::read(&p).unwrap_or_default();
                        let actual = format!("{:x}", Sha256::digest(&bytes));
                        if actual != *digest {
                            push_err(
                                rel_file.clone(),
                                "UI_ASSET_HASH",
                                format!("hash mismatch for {path}"),
                            );
                        }
                    }
                    None => push_err(
                        rel_file.clone(),
                        "UI_ASSET_MISSING",
                        format!("asset not found: {path}"),
                    ),
                }
            }
            // bundle digest
            if let Some(sha) = ui.get("sha256").and_then(|v| v.as_str()) {
                let bundle_digest = crate::verify::digest_ui_file_list(&files);
                if sha != bundle_digest {
                    push_err(
                        rel_file.clone(),
                        "UI_BUNDLE_DIGEST",
                        "UI bundle digest mismatch".to_string(),
                    );
                }
            } else {
                push_err(
                    rel_file.clone(),
                    "UI_BUNDLE_DIGEST_MISSING",
                    "missing metadata.ui.sha256".to_string(),
                );
            }
            // signature
            if let Err(e) = verify_dist_signature(&manifest, &[], public_key_bytes.as_deref()) {
                push_err(rel_file.clone(), "SIGNATURE", e.to_string());
            }
        } else {
            // component
            let comp_rel = manifest["component"].as_str().unwrap_or("").to_string();
            if comp_rel.is_empty() {
                push_err(
                    rel_file.clone(),
                    "COMPONENT_MISSING",
                    "manifest has no component path".to_string(),
                );
            } else {
                let comp_path = mpath.parent().unwrap_or(dist).join(&comp_rel);
                if !comp_path.exists() {
                    push_err(
                        rel_file.clone(),
                        "COMPONENT_NOT_FOUND",
                        format!("component not found: {}", comp_path.display()),
                    );
                } else {
                    let bytes = std::fs::read(&comp_path).unwrap_or_default();
                    let actual = format!("{:x}", Sha256::digest(&bytes));
                    let expected = manifest["metadata"]["sha256"].as_str().unwrap_or("");
                    if expected.is_empty() {
                        push_err(
                            rel_file.clone(),
                            "CHECKSUM_MISSING",
                            "no sha256 in manifest".to_string(),
                        );
                    } else if expected != actual {
                        push_err(
                            rel_file.clone(),
                            "CHECKSUM_MISMATCH",
                            format!("expected {} got {}", expected, actual),
                        );
                    }
                    if let Err(e) =
                        verify_dist_signature(&manifest, &bytes, public_key_bytes.as_deref())
                    {
                        push_err(rel_file.clone(), "SIGNATURE", e.to_string());
                    }
                }
            }
        }
    }

    // ui-plugins.json index
    let index_candidates = [
        dist.join("ui/public/ui-plugins.json"),
        dist.join("ui/ui-plugins.json"),
        dist.join("public/ui-plugins.json"),
        dist.join("ui-plugins.json"),
    ];
    for ic in &index_candidates {
        if ic.is_file() {
            let idx_raw = std::fs::read_to_string(ic).unwrap_or_default();
            let idx: serde_json::Value = match serde_json::from_str(&idx_raw) {
                Ok(v) => v,
                Err(e) => {
                    push_err(
                        ic.strip_prefix(dist).unwrap_or(ic).display().to_string(),
                        "UI_INDEX_INVALID",
                        format!("invalid json: {e}"),
                    );
                    break;
                }
            };
            if let Some(arr) = idx.as_array() {
                for entry in arr {
                    let ename = entry["name"].as_str().unwrap_or("").to_string();
                    // find manifest
                    let mpath = manifest_dir.join(format!("{}.manifest.json", ename));
                    if !mpath.is_file() {
                        push_err(
                            ic.strip_prefix(dist).unwrap_or(ic).display().to_string(),
                            "UI_INDEX_UNKNOWN",
                            format!("unknown plugin {}", ename),
                        );
                        continue;
                    }
                    let mraw = std::fs::read_to_string(&mpath).unwrap_or_default();
                    let m: serde_json::Value = match serde_json::from_str(&mraw) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if m["metadata"]["ui"]["sha256"].as_str() != entry["sha256"].as_str() {
                        push_err(
                            ic.strip_prefix(dist).unwrap_or(ic).display().to_string(),
                            "UI_INDEX_SHA",
                            format!("sha256 mismatch for {}", ename),
                        );
                    }
                    if m["metadata"]["ui"]["files"] != entry["files"] {
                        push_err(
                            ic.strip_prefix(dist).unwrap_or(ic).display().to_string(),
                            "UI_INDEX_FILES",
                            format!("files mismatch for {}", ename),
                        );
                    }
                    // entry path exists?
                    if let Some(e) = entry["entry"].as_str() {
                        let mut found = false;
                        for base in &ui_bases {
                            if base
                                .join(
                                    e.trim_start_matches("plugins/")
                                        .trim_start_matches(&format!("{}/", ename)),
                                )
                                .is_file()
                            {
                                found = true;
                                break;
                            }
                            // try direct
                            if base.join(e).is_file() {
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            // not hard fail? We'll fail to be strict
                            push_err(
                                ic.strip_prefix(dist).unwrap_or(ic).display().to_string(),
                                "UI_INDEX_ENTRY_MISSING",
                                format!("entry asset missing for {}", ename),
                            );
                        }
                    }
                }
            }
            break;
        }
    }

    let ok = errors.is_empty();
    let report = Report {
        ok,
        errors: errors.clone(),
        checked,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        if ok {
            println!("Verification OK ({} checked).", checked);
        } else {
            for e in &errors {
                println!("{}: [{}] {}", e.file, e.code, e.message);
            }
            println!("Verification FAILED ({} error(s)).", errors.len());
        }
    }
    if ok {
        Ok(())
    } else {
        anyhow::bail!("verification failed with {} error(s)", errors.len())
    }
}

pub fn verify_dist_signature(
    manifest: &serde_json::Value,
    artifact: &[u8],
    public_key: Option<&[u8]>,
) -> anyhow::Result<()> {
    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("plugin");
    let signature = manifest
        .get("metadata")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("signature"))
        .and_then(|v| v.as_str());
    if signature.is_none() {
        anyhow::bail!("plugin {} is unsigned", name);
    }
    let signature = signature.unwrap();
    if manifest
        .get("metadata")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("signature_format"))
        .and_then(|v| v.as_str())
        != Some("owasidea-manifest-v1")
    {
        anyhow::bail!("unsupported signature format for {}", name);
    }
    let pk = public_key
        .ok_or_else(|| anyhow::anyhow!("OWASIDEA_PUBLIC_KEY or dist trust key required"))?;
    use base64::Engine;
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature)
        .context("decode signature")?;
    // reuse host signed_manifest_payload logic
    let mut value = manifest.clone();
    if let Some(metadata) = value.get_mut("metadata").and_then(|v| v.as_object_mut()) {
        metadata.remove("signature");
    }
    let manifest_json = serde_json::to_vec(&value)?;
    let mut payload = b"owasidea-plugin-signature-v1\0".to_vec();
    payload.extend_from_slice(&manifest_json);
    payload.push(0);
    payload.extend_from_slice(artifact);
    use ring::signature::{UnparsedPublicKey, ED25519};
    UnparsedPublicKey::new(&ED25519, pk)
        .verify(&payload, &sig_bytes)
        .map_err(|_| anyhow::anyhow!("signature verification failed for {}", name))?;
    Ok(())
}

pub fn digest_ui_file_list(files: &std::collections::BTreeMap<String, String>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (path, digest) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}
