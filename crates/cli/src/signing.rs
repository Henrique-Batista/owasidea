//! RL-03: code signing / notarization planning.
//!
//! CI passes certificates and credentials through environment variables. This
//! module reads **presence only** (never prints or serializes values) and
//! decides whether the Windows/macOS signing steps run, or are skipped safely.
//!
//! A partial configuration is reported as `invalid` instead of silently
//! skipping: a missing password or identity is a mistake, not a "no
//! certificate" situation.
//!
//! The actual signing is done by the Tauri CLI (Authenticode via
//! `signtool`/`signCommand`, macOS `codesign` + `notarytool`) using the config
//! and environment described in `docs/CODE_SIGNING.md`.

use serde::Serialize;

/// Environment variables that must be present together for a PFX import.
const WINDOWS_PFX_VARS: [&str; 2] = ["WINDOWS_CERTIFICATE", "WINDOWS_CERTIFICATE_PASSWORD"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "windows" | "win" | "win32" => Ok(Self::Windows),
            "macos" | "mac" | "darwin" => Ok(Self::Macos),
            "linux" => Ok(Self::Linux),
            other => {
                anyhow::bail!("unknown signing platform: {other} (expected windows|macos|linux)")
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }

    /// Platform of the machine running the CLI (the signing host).
    pub fn host() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// No credentials: the build stays unsigned (documented skip).
    Skip,
    /// PFX imported into the runner store, or a preinstalled certificate.
    WindowsPfx,
    /// Custom `signCommand` (Azure Artifact Signing, relic, ...).
    WindowsTrustedSigning,
    /// Developer ID signing, with or without notarization.
    MacosDeveloperId,
    /// Partial or contradictory configuration; the CI step must fail.
    Invalid,
}

#[derive(Clone, Debug, Serialize)]
pub struct SigningPlan {
    pub platform: &'static str,
    pub mode: Mode,
    /// The bundle step should produce signed artifacts.
    pub sign: bool,
    /// Apple notarization credentials are complete.
    pub notarize: bool,
    /// Decode/import `WINDOWS_CERTIFICATE` into the certificate store first.
    pub import_certificate: bool,
    /// Write `APPLE_API_KEY_P8` to a temporary file before bundling.
    pub materialize_api_key: bool,
    /// Names of missing variables (never values).
    pub missing: Vec<&'static str>,
    pub reason: String,
    pub warnings: Vec<String>,
}

impl SigningPlan {
    fn skip(platform: Platform, reason: impl Into<String>) -> Self {
        Self {
            platform: platform.as_str(),
            mode: Mode::Skip,
            sign: false,
            notarize: false,
            import_certificate: false,
            materialize_api_key: false,
            missing: Vec::new(),
            reason: reason.into(),
            warnings: Vec::new(),
        }
    }

    fn invalid(platform: Platform, reason: impl Into<String>, missing: Vec<&'static str>) -> Self {
        Self {
            platform: platform.as_str(),
            mode: Mode::Invalid,
            sign: false,
            notarize: false,
            import_certificate: false,
            materialize_api_key: false,
            missing,
            reason: reason.into(),
            warnings: Vec::new(),
        }
    }
}

/// Build a plan from an environment lookup. Kept separate from `plan` so tests
/// do not have to mutate the process environment.
pub fn plan_with<F>(platform: Platform, get: F) -> SigningPlan
where
    F: Fn(&str) -> Option<String>,
{
    let has = |name: &str| {
        get(name)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    };
    match platform {
        Platform::Windows => windows_plan(&has),
        Platform::Macos => macos_plan(&has, |name| get(name)),
        Platform::Linux => SigningPlan::skip(
            platform,
            "Linux packages are not code-signed; Windows and macOS signing is skipped",
        ),
    }
}

pub fn plan(platform: Platform) -> SigningPlan {
    plan_with(platform, |name| std::env::var(name).ok())
}

fn windows_plan(has: &impl Fn(&str) -> bool) -> SigningPlan {
    let sign_command = has("OWASIDEA_WINDOWS_SIGN_COMMAND");
    let pfx = has("WINDOWS_CERTIFICATE");
    let pfx_password = has("WINDOWS_CERTIFICATE_PASSWORD");
    let thumbprint = has("OWASIDEA_WINDOWS_CERT_THUMBPRINT");

    if sign_command {
        let mut plan = SigningPlan {
            platform: Platform::Windows.as_str(),
            mode: Mode::WindowsTrustedSigning,
            sign: true,
            notarize: false,
            import_certificate: false,
            materialize_api_key: false,
            missing: Vec::new(),
            reason: "custom signCommand configured (for example Azure Artifact Signing)"
                .to_string(),
            warnings: Vec::new(),
        };
        if pfx || pfx_password || thumbprint {
            plan.warnings.push(
                "OWASIDEA_WINDOWS_SIGN_COMMAND overrides the PFX/thumbprint settings; the certificate secrets are ignored"
                    .to_string(),
            );
        }
        return plan;
    }

    if pfx != pfx_password {
        let missing = WINDOWS_PFX_VARS
            .iter()
            .copied()
            .filter(|name| !has(name))
            .collect();
        return SigningPlan::invalid(
            Platform::Windows,
            "incomplete Windows PFX configuration; set both WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD",
            missing,
        );
    }

    if pfx {
        return SigningPlan {
            platform: Platform::Windows.as_str(),
            mode: Mode::WindowsPfx,
            sign: true,
            notarize: false,
            import_certificate: true,
            materialize_api_key: false,
            missing: Vec::new(),
            reason: "PFX certificate will be imported into the runner certificate store"
                .to_string(),
            warnings: Vec::new(),
        };
    }

    if thumbprint {
        return SigningPlan {
            platform: Platform::Windows.as_str(),
            mode: Mode::WindowsPfx,
            sign: true,
            notarize: false,
            import_certificate: false,
            materialize_api_key: false,
            missing: Vec::new(),
            reason: "using a preinstalled certificate selected by OWASIDEA_WINDOWS_CERT_THUMBPRINT"
                .to_string(),
            warnings: Vec::new(),
        };
    }

    SigningPlan::skip(
        Platform::Windows,
        "no Windows certificate configured; NSIS/MSI installers stay unsigned",
    )
}

fn macos_plan(has: &impl Fn(&str) -> bool, get: impl Fn(&str) -> Option<String>) -> SigningPlan {
    let identity_value = get("APPLE_SIGNING_IDENTITY").unwrap_or_default();
    let ad_hoc = identity_value.trim() == "-";
    let certificate = has("APPLE_CERTIFICATE");
    let certificate_password = has("APPLE_CERTIFICATE_PASSWORD");
    let identity = has("APPLE_SIGNING_IDENTITY");

    let api_key = has("APPLE_API_KEY");
    let api_issuer = has("APPLE_API_ISSUER");
    let api_key_path = has("APPLE_API_KEY_PATH");
    let api_key_p8 = has("APPLE_API_KEY_P8");
    let apple_id = has("APPLE_ID");
    let apple_password = has("APPLE_PASSWORD");
    let apple_team = has("APPLE_TEAM_ID");

    let any_signing = certificate || certificate_password || identity;
    let any_api = api_key || api_issuer || api_key_path || api_key_p8;
    let any_apple_id = apple_id || apple_password || apple_team;
    let any_notarization = any_api || any_apple_id;

    if !any_signing && !any_notarization {
        return SigningPlan::skip(
            Platform::Macos,
            "no Apple signing/notarization credentials configured; the .app/.dmg stay unsigned",
        );
    }

    if !ad_hoc && !(certificate && certificate_password && identity) {
        let missing = [
            "APPLE_CERTIFICATE",
            "APPLE_CERTIFICATE_PASSWORD",
            "APPLE_SIGNING_IDENTITY",
        ]
        .into_iter()
        .filter(|name| !has(name))
        .collect();
        return SigningPlan::invalid(
            Platform::Macos,
            "incomplete macOS signing configuration; set the certificate, its password and the signing identity",
            missing,
        );
    }

    if ad_hoc && any_notarization {
        return SigningPlan::invalid(
            Platform::Macos,
            "ad-hoc signing (APPLE_SIGNING_IDENTITY=-) cannot be notarized; remove the notarization credentials or use a Developer ID identity",
            Vec::new(),
        );
    }

    let api_complete = api_key && api_issuer && (api_key_path || api_key_p8);
    let apple_id_complete = apple_id && apple_password && apple_team;

    let mut plan = SigningPlan {
        platform: Platform::Macos.as_str(),
        mode: Mode::MacosDeveloperId,
        sign: true,
        notarize: api_complete || apple_id_complete,
        import_certificate: false,
        materialize_api_key: api_complete && api_key_p8 && !api_key_path,
        missing: Vec::new(),
        reason: if api_complete || apple_id_complete {
            "Developer ID signing and notarization configured".to_string()
        } else {
            "Developer ID signing configured without complete notarization credentials".to_string()
        },
        warnings: Vec::new(),
    };

    if !plan.notarize {
        if any_api && !api_complete {
            plan.missing = ["APPLE_API_KEY", "APPLE_API_ISSUER"]
                .into_iter()
                .filter(|name| !has(name))
                .collect();
            if !api_key_path && !api_key_p8 {
                plan.missing.push("APPLE_API_KEY_PATH or APPLE_API_KEY_P8");
            }
        } else if any_apple_id && !apple_id_complete {
            plan.missing = ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"]
                .into_iter()
                .filter(|name| !has(name))
                .collect();
        }
        if !plan.missing.is_empty() {
            return SigningPlan::invalid(
                Platform::Macos,
                "incomplete Apple notarization credentials; set one complete credential set",
                plan.missing,
            );
        }
        plan.warnings.push(
            "notarization is not configured; Gatekeeper will warn on first launch (see docs/CODE_SIGNING.md)"
                .to_string(),
        );
    }

    if plan.materialize_api_key {
        plan.warnings.push(
            "APPLE_API_KEY_P8 will be written to a temporary file and removed after the bundle"
                .to_string(),
        );
    }

    plan
}

pub fn cmd_signing_plan(platform: Option<&str>, json: bool, require: bool) -> anyhow::Result<()> {
    let platform = match platform {
        Some(value) => Platform::parse(value)?,
        None => Platform::host(),
    };
    let plan = plan(platform);

    if json {
        let mut bytes = serde_json::to_vec_pretty(&plan)?;
        bytes.push(b'\n');
        print!("{}", String::from_utf8_lossy(&bytes));
    } else {
        println!("platform: {}", plan.platform);
        println!("mode: {:?}", plan.mode);
        println!("sign: {}", plan.sign);
        println!("notarize: {}", plan.notarize);
        if !plan.missing.is_empty() {
            println!("missing: {}", plan.missing.join(", "));
        }
        println!("reason: {}", plan.reason);
        for warning in &plan.warnings {
            println!("warning: {warning}");
        }
    }

    if plan.mode == Mode::Invalid {
        anyhow::bail!(
            "invalid code signing configuration for {}: {}{}",
            plan.platform,
            plan.reason,
            if plan.missing.is_empty() {
                String::new()
            } else {
                format!(" (missing: {})", plan.missing.join(", "))
            }
        );
    }
    if require && !plan.sign {
        anyhow::bail!(
            "code signing required for {} but not configured: {}",
            plan.platform,
            plan.reason
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn plan_for(platform: Platform, vars: &[(&str, &str)]) -> SigningPlan {
        let env: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        plan_with(platform, |name| env.get(name).cloned())
    }

    #[test]
    fn windows_skips_without_certificate() {
        let plan = plan_for(Platform::Windows, &[]);
        assert_eq!(plan.mode, Mode::Skip);
        assert!(!plan.sign);
        assert!(!plan.import_certificate);
        assert!(plan.reason.contains("unsigned"));
    }

    #[test]
    fn windows_pfx_signs_and_imports() {
        let plan = plan_for(
            Platform::Windows,
            &[
                ("WINDOWS_CERTIFICATE", "base64"),
                ("WINDOWS_CERTIFICATE_PASSWORD", "secret"),
            ],
        );
        assert_eq!(plan.mode, Mode::WindowsPfx);
        assert!(plan.sign);
        assert!(plan.import_certificate);
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn windows_partial_pfx_is_invalid_and_names_the_missing_variable() {
        let plan = plan_for(Platform::Windows, &[("WINDOWS_CERTIFICATE", "base64")]);
        assert_eq!(plan.mode, Mode::Invalid);
        assert_eq!(plan.missing, vec!["WINDOWS_CERTIFICATE_PASSWORD"]);
        assert!(!plan.sign);
    }

    #[test]
    fn windows_sign_command_uses_custom_signing() {
        let plan = plan_for(
            Platform::Windows,
            &[(
                "OWASIDEA_WINDOWS_SIGN_COMMAND",
                "artifact-signing-cli -e https://example %1",
            )],
        );
        assert_eq!(plan.mode, Mode::WindowsTrustedSigning);
        assert!(plan.sign);
        assert!(!plan.import_certificate);
    }

    #[test]
    fn windows_sign_command_wins_over_pfx_with_warning() {
        let plan = plan_for(
            Platform::Windows,
            &[
                ("OWASIDEA_WINDOWS_SIGN_COMMAND", "relic sign --file %1"),
                ("WINDOWS_CERTIFICATE", "base64"),
                ("WINDOWS_CERTIFICATE_PASSWORD", "secret"),
            ],
        );
        assert_eq!(plan.mode, Mode::WindowsTrustedSigning);
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn windows_preinstalled_thumbprint_signs_without_import() {
        let plan = plan_for(
            Platform::Windows,
            &[("OWASIDEA_WINDOWS_CERT_THUMBPRINT", "A1B2")],
        );
        assert_eq!(plan.mode, Mode::WindowsPfx);
        assert!(plan.sign);
        assert!(!plan.import_certificate);
    }

    #[test]
    fn macos_skips_without_credentials() {
        let plan = plan_for(Platform::Macos, &[]);
        assert_eq!(plan.mode, Mode::Skip);
        assert!(!plan.sign);
        assert!(!plan.notarize);
    }

    #[test]
    fn macos_developer_id_with_api_key_notarizes() {
        let plan = plan_for(
            Platform::Macos,
            &[
                ("APPLE_CERTIFICATE", "base64"),
                ("APPLE_CERTIFICATE_PASSWORD", "secret"),
                (
                    "APPLE_SIGNING_IDENTITY",
                    "Developer ID Application: Example",
                ),
                ("APPLE_API_KEY", "KEYID"),
                ("APPLE_API_ISSUER", "issuer-uuid"),
                ("APPLE_API_KEY_P8", "-----BEGIN PRIVATE KEY-----"),
            ],
        );
        assert_eq!(plan.mode, Mode::MacosDeveloperId);
        assert!(plan.sign);
        assert!(plan.notarize);
        assert!(plan.materialize_api_key);
    }

    #[test]
    fn macos_developer_id_with_apple_id_notarizes() {
        let plan = plan_for(
            Platform::Macos,
            &[
                ("APPLE_CERTIFICATE", "base64"),
                ("APPLE_CERTIFICATE_PASSWORD", "secret"),
                (
                    "APPLE_SIGNING_IDENTITY",
                    "Developer ID Application: Example",
                ),
                ("APPLE_ID", "dev@example.com"),
                ("APPLE_PASSWORD", "app-specific-password"),
                ("APPLE_TEAM_ID", "TEAMID1234"),
            ],
        );
        assert!(plan.notarize);
        assert!(!plan.materialize_api_key);
    }

    #[test]
    fn macos_signs_without_notarization_with_warning() {
        let plan = plan_for(
            Platform::Macos,
            &[
                ("APPLE_CERTIFICATE", "base64"),
                ("APPLE_CERTIFICATE_PASSWORD", "secret"),
                (
                    "APPLE_SIGNING_IDENTITY",
                    "Developer ID Application: Example",
                ),
            ],
        );
        assert!(plan.sign);
        assert!(!plan.notarize);
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn macos_partial_certificate_is_invalid() {
        let plan = plan_for(
            Platform::Macos,
            &[
                ("APPLE_CERTIFICATE", "base64"),
                (
                    "APPLE_SIGNING_IDENTITY",
                    "Developer ID Application: Example",
                ),
            ],
        );
        assert_eq!(plan.mode, Mode::Invalid);
        assert_eq!(plan.missing, vec!["APPLE_CERTIFICATE_PASSWORD"]);
    }

    #[test]
    fn macos_partial_api_key_is_invalid() {
        let plan = plan_for(
            Platform::Macos,
            &[
                ("APPLE_CERTIFICATE", "base64"),
                ("APPLE_CERTIFICATE_PASSWORD", "secret"),
                (
                    "APPLE_SIGNING_IDENTITY",
                    "Developer ID Application: Example",
                ),
                ("APPLE_API_KEY", "KEYID"),
            ],
        );
        assert_eq!(plan.mode, Mode::Invalid);
        assert!(plan.missing.contains(&"APPLE_API_ISSUER"));
    }

    #[test]
    fn macos_ad_hoc_cannot_notarize() {
        let plan = plan_for(
            Platform::Macos,
            &[
                ("APPLE_SIGNING_IDENTITY", "-"),
                ("APPLE_ID", "dev@example.com"),
                ("APPLE_PASSWORD", "pw"),
                ("APPLE_TEAM_ID", "TEAMID1234"),
            ],
        );
        assert_eq!(plan.mode, Mode::Invalid);
    }

    #[test]
    fn linux_is_always_skip() {
        let plan = plan_for(Platform::Linux, &[("WINDOWS_CERTIFICATE", "base64")]);
        assert_eq!(plan.mode, Mode::Skip);
        assert!(!plan.sign);
    }

    #[test]
    fn json_never_contains_secret_values() {
        let plan = plan_for(
            Platform::Windows,
            &[
                ("WINDOWS_CERTIFICATE", "super-secret-pfx-base64"),
                ("WINDOWS_CERTIFICATE_PASSWORD", "super-secret-password"),
            ],
        );
        let json = serde_json::to_string(&plan).unwrap();
        assert!(!json.contains("super-secret"));
        assert!(json.contains("windows-pfx"));
    }
}
