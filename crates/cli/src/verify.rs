use anyhow::Context;
use std::path::{Path, PathBuf};

/// Where a verification trust root came from.
#[derive(Debug, PartialEq, Eq)]
enum TrustKeyOutcome {
    Key(Vec<u8>),
    Missing,
    /// A key staged inside the artifact tree was found but is not a pinned root.
    Unpinned(PathBuf),
}

/// Resolve the Ed25519 public key used as the trust root.
///
/// A key copied into `dist/` by `bundle` cannot authenticate that same bundle:
/// whoever can rewrite the artifacts can also rewrite the staged key. The
/// trust root must be supplied out of band (`--public-key` or
/// `OWASIDEA_PUBLIC_KEY`); `--allow-dist-trust` is an explicit opt-in for local
/// smoke checks only.
fn resolve_trust_key(
    dist: &Path,
    explicit: Option<&Path>,
    allow_dist_trust: bool,
) -> anyhow::Result<TrustKeyOutcome> {
    use base64::Engine;
    let decode = |path: &Path| -> anyhow::Result<Vec<u8>> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read public key {}", path.display()))?;
        base64::engine::general_purpose::STANDARD
            .decode(raw.trim())
            .with_context(|| format!("decode public key {}", path.display()))
    };

    if let Some(path) = explicit {
        return Ok(TrustKeyOutcome::Key(decode(path)?));
    }
    if let Ok(value) = std::env::var("OWASIDEA_PUBLIC_KEY") {
        let path = PathBuf::from(value.trim());
        if !path.as_os_str().is_empty() {
            return Ok(TrustKeyOutcome::Key(decode(&path)?));
        }
    }

    let staged = [
        dist.join("trust/owasidea.pub"),
        dist.join("keys/owasidea.pub"),
    ]
    .into_iter()
    .find(|path| path.is_file());
    match staged {
        Some(path) if allow_dist_trust => Ok(TrustKeyOutcome::Key(decode(&path)?)),
        Some(path) => Ok(TrustKeyOutcome::Unpinned(path)),
        None => Ok(TrustKeyOutcome::Missing),
    }
}

pub fn cmd_verify(
    dist: &Path,
    public_key: Option<&Path>,
    allow_dist_trust: bool,
    json: bool,
) -> anyhow::Result<()> {
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

    // Trust root: pinned out of band, never defaulted to the key staged in
    // the artifact tree being verified.
    let public_key_bytes: Option<Vec<u8>> = match resolve_trust_key(
        dist,
        public_key,
        allow_dist_trust,
    ) {
        Ok(TrustKeyOutcome::Key(bytes)) => Some(bytes),
        Ok(TrustKeyOutcome::Missing) => {
            push_err(
                dist.display().to_string(),
                "TRUST_ROOT_MISSING",
                "no pinned public key; pass --public-key <file> or set OWASIDEA_PUBLIC_KEY"
                    .to_string(),
            );
            None
        }
        Ok(TrustKeyOutcome::Unpinned(path)) => {
            let message = format!(
                "{} is staged inside dist and cannot be its own trust root; pass --public-key <file> or set OWASIDEA_PUBLIC_KEY (or --allow-dist-trust for local checks)",
                path.display()
            );
            push_err(dist.display().to_string(), "TRUST_ROOT_UNPINNED", message);
            None
        }
        Err(error) => {
            push_err(
                dist.display().to_string(),
                "PUBLIC_KEY_INVALID",
                error.to_string(),
            );
            None
        }
    };

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
            if let Some(public_key) = public_key_bytes.as_deref() {
                if let Err(e) = verify_dist_signature(&manifest, &[], Some(public_key)) {
                    push_err(rel_file.clone(), "SIGNATURE", e.to_string());
                }
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
                    if let Some(public_key) = public_key_bytes.as_deref() {
                        if let Err(e) = verify_dist_signature(&manifest, &bytes, Some(public_key)) {
                            push_err(rel_file.clone(), "SIGNATURE", e.to_string());
                        }
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
    let pk =
        public_key.ok_or_else(|| anyhow::anyhow!("pinned public key required for {}", name))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "owasidea-verify-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("trust")).unwrap();
        // base64 of b"k"
        std::fs::write(dir.join("trust/owasidea.pub"), b"aw==\n").unwrap();
        dir
    }

    #[test]
    fn dist_trust_key_is_rejected_without_pinned_root() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_PUBLIC_KEY");
        let dist = temp_dir("unpinned");

        let outcome = resolve_trust_key(&dist, None, false).unwrap();
        assert!(matches!(outcome, TrustKeyOutcome::Unpinned(_)));

        let _ = std::fs::remove_dir_all(&dist);
    }

    #[test]
    fn dist_trust_key_requires_explicit_opt_in() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_PUBLIC_KEY");
        let dist = temp_dir("opt-in");

        let outcome = resolve_trust_key(&dist, None, true).unwrap();
        assert_eq!(outcome, TrustKeyOutcome::Key(b"k".to_vec()));

        let _ = std::fs::remove_dir_all(&dist);
    }

    #[test]
    fn explicit_public_key_outside_dist_is_the_trust_root() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_PUBLIC_KEY");
        let dist = temp_dir("explicit");
        let key = dist.join("release.pub");
        std::fs::write(&key, b"aw==\n").unwrap();

        let outcome = resolve_trust_key(&dist, Some(&key), false).unwrap();
        assert_eq!(outcome, TrustKeyOutcome::Key(b"k".to_vec()));

        let _ = std::fs::remove_dir_all(&dist);
    }

    #[test]
    fn missing_trust_root_is_reported() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OWASIDEA_PUBLIC_KEY");
        let dist =
            std::env::temp_dir().join(format!("owasidea-verify-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dist);
        std::fs::create_dir_all(&dist).unwrap();

        let outcome = resolve_trust_key(&dist, None, false).unwrap();
        assert_eq!(outcome, TrustKeyOutcome::Missing);

        let _ = std::fs::remove_dir_all(&dist);
    }
}
