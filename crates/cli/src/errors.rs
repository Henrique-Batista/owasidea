//! Taxonomia de erros do CLI Owasidea.
//!
//! Códigos estáveis no formato `OWASIDEA_E_*`. Cada entrada carrega
//! `message`, `hint` e `docs` (URL/âncora). Use `owasidea explain <code>`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEntry {
    pub code: &'static str,
    pub message: &'static str,
    pub hint: &'static str,
    pub docs: &'static str,
}

pub const CATALOG: &[ErrorEntry] = &[
    ErrorEntry {
        code: "OWASIDEA_E_CONFIG_INVALID",
        message: "Application configuration (wasm-app.toml) is invalid or missing.",
        hint: "Check the config path, TOML syntax and required sections ([app], [host], [plugins.*]).",
        docs: "docs/ERRORS.md#owasidea_e_config_invalid",
    },
    ErrorEntry {
        code: "OWASIDEA_E_TOOLCHAIN_MISSING",
        message: "A required toolchain binary is missing or too old.",
        hint: "Run `owasidea doctor` to see which tool is missing and how to install it.",
        docs: "docs/ERRORS.md#owasidea_e_toolchain_missing",
    },
    ErrorEntry {
        code: "OWASIDEA_E_MANIFEST_INVALID",
        message: "A generated or loaded plugin manifest is invalid.",
        hint: "Re-run `owasidea build` to regenerate manifests from WIT and inspect the reported field.",
        docs: "docs/ERRORS.md#owasidea_e_manifest_invalid",
    },
    ErrorEntry {
        code: "OWASIDEA_E_SIGNATURE_INVALID",
        message: "Plugin manifest signature verification failed.",
        hint: "Rebuild/sign the plugin with the configured key or disable signing only for local dev.",
        docs: "docs/ERRORS.md#owasidea_e_signature_invalid",
    },
    ErrorEntry {
        code: "OWASIDEA_E_HTTP_CALL",
        message: "HTTP/network call to the host or a plugin failed.",
        hint: "Confirm the host is running, the address/port, and the request body/args; retry.",
        docs: "docs/ERRORS.md#owasidea_e_http_call",
    },
    ErrorEntry {
        code: "OWASIDEA_E_PLUGIN_NOT_FOUND",
        message: "The requested plugin does not exist in the manifest directory.",
        hint: "Check the plugin name and run `owasidea build`; use `owasidea inspect <plugin>`.",
        docs: "docs/ERRORS.md#owasidea_e_plugin_not_found",
    },
    ErrorEntry {
        code: "OWASIDEA_E_ABI_INCOMPATIBLE",
        message: "Plugin ABI/runtime is incompatible with this host.",
        hint: "Rebuild the plugin with the supported WASI component target and matching WIT.",
        docs: "docs/ERRORS.md#owasidea_e_abi_incompatible",
    },
    ErrorEntry {
        code: "OWASIDEA_E_BUILD_FAILED",
        message: "Plugin build failed.",
        hint: "Read the compiler/build tool output above; fix the source and retry `owasidea build`.",
        docs: "docs/ERRORS.md#owasidea_e_build_failed",
    },
    ErrorEntry {
        code: "OWASIDEA_E_VALIDATE_FAILED",
        message: "Manifest validation failed (checksum, capability or config mismatch).",
        hint: "Run `owasidea validate` and align manifests with wasm-app.toml and component WIT.",
        docs: "docs/ERRORS.md#owasidea_e_validate_failed",
    },
    ErrorEntry {
        code: "OWASIDEA_E_UNKNOWN",
        message: "Unknown error code.",
        hint: "Run `owasidea explain <code>` with a known code; see docs/ERRORS.md.",
        docs: "docs/ERRORS.md",
    },
];

pub fn lookup(code: &str) -> Option<&'static ErrorEntry> {
    CATALOG.iter().find(|e| e.code == code)
}

pub fn cmd_explain(code: &str, json: bool) -> anyhow::Result<()> {
    match lookup(code) {
        Some(entry) => {
            if json {
                println!("{}", serde_json::to_string_pretty(entry)?);
            } else {
                println!("{}", entry.code);
                println!("{}", entry.message);
                println!("hint: {}", entry.hint);
                println!("docs: {}", entry.docs);
            }
            Ok(())
        }
        None => anyhow::bail!("unknown error code: {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known() {
        let e = lookup("OWASIDEA_E_CONFIG_INVALID").unwrap();
        assert_eq!(e.code, "OWASIDEA_E_CONFIG_INVALID");
        assert!(!e.message.is_empty());
        assert!(!e.hint.is_empty());
        assert!(!e.docs.is_empty());
    }

    #[test]
    fn lookup_unknown() {
        assert!(lookup("OWASIDEA_E_NOPE").is_none());
    }

    #[test]
    fn all_codes_unique() {
        let mut seen = std::collections::HashSet::new();
        for e in CATALOG {
            assert!(seen.insert(e.code), "duplicate code {}", e.code);
        }
    }
}
