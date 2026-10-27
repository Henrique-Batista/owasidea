//! ABI and manifest deprecation checks (RL-07).
//!
//! `validate` never fails because a manifest is old: legacy manifests keep
//! loading under host ABI 1. Instead it prints one warning per deprecated shape
//! so authors can regenerate before the removal window in
//! `docs/ABI_POLICY.md` closes.
//!
//! Warnings are advisory only. Incompatibilities that the host cannot honour
//! (unknown `schema_version`, `host_abi` mismatch, `min_host` newer than the
//! host) remain hard errors in `cmd_validate`.

use serde_json::Value;

/// Manifest schema version written by `owasidea build`.
pub const CURRENT_SCHEMA_VERSION: u64 = 1;

/// Pre-CT-08 manifests: no `schema_version` field, or an explicit `0`.
pub const LEGACY_SCHEMA_VERSION: u64 = 0;

/// Stable warning codes so CI logs and tests can match without parsing prose.
pub const LEGACY_CODE: &str = "ABI_LEGACY_MANIFEST";
pub const MISSING_HOST_FIELDS_CODE: &str = "ABI_MISSING_HOST_FIELDS";

/// Doc every warning points at.
pub const POLICY_DOC: &str = "docs/ABI_POLICY.md";

/// Advisory warnings for one generated manifest. Empty means "current shape".
pub fn manifest_warnings(manifest: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    match manifest.get("schema_version").and_then(Value::as_u64) {
        None => {
            warnings.push(legacy_warning("no integer schema_version field"));
            push_missing_host_fields(manifest, &mut warnings);
        }
        Some(LEGACY_SCHEMA_VERSION) => {
            warnings.push(legacy_warning("schema_version 0"));
            push_missing_host_fields(manifest, &mut warnings);
        }
        Some(_) => {}
    }
    warnings
}

fn legacy_warning(detail: &str) -> String {
    format!(
        "{LEGACY_CODE}: deprecated legacy manifest ({detail}); host ABI {} still accepts it \
         with defaults, but a future host ABI will reject it. Regenerate with \
         `owasidea build` (see {POLICY_DOC}).",
        owasidea_host::HOST_ABI
    )
}

fn push_missing_host_fields(manifest: &Value, warnings: &mut Vec<String>) {
    let missing: Vec<&str> = ["host_abi", "min_host"]
        .into_iter()
        .filter(|field| manifest.get(*field).and_then(Value::as_str).is_none())
        .collect();
    if missing.is_empty() {
        return;
    }
    warnings.push(format!(
        "{MISSING_HOST_FIELDS_CODE}: manifest has no {}; the host applies defaults. \
         Regenerate with `owasidea build` (see {POLICY_DOC}).",
        missing.join(" or ")
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn current() -> Value {
        json!({
            "schema_version": CURRENT_SCHEMA_VERSION,
            "host_abi": owasidea_host::HOST_ABI,
            "min_host": owasidea_host::HOST_VERSION,
        })
    }

    #[test]
    fn current_manifest_has_no_warnings() {
        assert!(manifest_warnings(&current()).is_empty());
    }

    #[test]
    fn missing_schema_version_is_legacy_but_not_an_error() {
        let warnings = manifest_warnings(&json!({"name": "old"}));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].starts_with(LEGACY_CODE));
        assert!(warnings[0].contains("no integer schema_version field"));
        assert!(warnings[1].starts_with(MISSING_HOST_FIELDS_CODE));
        assert!(warnings[1].contains("host_abi or min_host"));
        assert!(warnings[0].contains(POLICY_DOC));
        assert!(warnings[0].contains("owasidea build"));
    }

    #[test]
    fn explicit_zero_schema_version_is_reported_as_zero() {
        let warnings = manifest_warnings(&json!({"schema_version": 0}));
        assert!(warnings[0].contains("schema_version 0"));
    }

    #[test]
    fn legacy_manifest_with_host_fields_only_warns_about_schema() {
        let warnings = manifest_warnings(&json!({
            "schema_version": 0,
            "host_abi": owasidea_host::HOST_ABI,
            "min_host": owasidea_host::HOST_VERSION,
        }));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].starts_with(LEGACY_CODE));
    }

    #[test]
    fn non_integer_schema_version_is_not_treated_as_current() {
        // `"1"` is malformed JSON for the schema; the host refuses it at load.
        // This layer only reports deprecation, so it must not claim it is fine.
        let warnings = manifest_warnings(&json!({"schema_version": "1"}));
        assert!(!warnings.is_empty());
        assert!(warnings[0].starts_with(LEGACY_CODE));
        assert!(warnings[0].contains("no integer schema_version field"));
    }

    #[test]
    fn future_schema_version_is_left_to_the_hard_error_path() {
        // `validate` reports unsupported schema versions as errors; the warning
        // layer must not duplicate that as advisory text.
        assert!(manifest_warnings(&json!({"schema_version": 2})).is_empty());
    }
}
