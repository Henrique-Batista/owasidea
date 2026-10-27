#![allow(dead_code)]
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ToolLock {
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub min_version: Option<String>,
    pub install: Option<InstallCmds>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InstallCmds {
    pub linux: Option<String>,
    pub macos: Option<String>,
    pub windows: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
    Unknown,
}

pub fn detect_os() -> Os {
    if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else {
        Os::Unknown
    }
}

pub fn install_command(spec: &ToolSpec, os: Os) -> Option<String> {
    let cmds = spec.install.as_ref()?;
    match os {
        Os::Linux => cmds.linux.clone(),
        Os::Macos => cmds.macos.clone(),
        Os::Windows => cmds.windows.clone(),
        Os::Unknown => None,
    }
}

pub fn parse_lock(content: &str) -> Result<ToolLock, toml::de::Error> {
    toml::from_str(content)
}

pub fn version_ok(actual: Option<&str>, min: Option<&str>) -> bool {
    match (actual, min) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(a), Some(m)) => a.trim() >= m.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_lock_basic() {
        let c = r#"
[[tools]]
name = "rustc"
min_version = "1.80"
[tools.install]
linux = "rustup toolchain install 1.80"
macos = "rustup toolchain install 1.80"
windows = "rustup toolchain install 1.80"
"#;
        let l = parse_lock(c).unwrap();
        assert_eq!(l.tools.len(), 1);
        assert_eq!(l.tools[0].name, "rustc");
    }
    #[test]
    fn install_command_os() {
        let spec = ToolSpec {
            name: "x".into(),
            min_version: None,
            install: Some(InstallCmds {
                linux: Some("apt install x".into()),
                macos: None,
                windows: None,
            }),
        };
        assert_eq!(
            install_command(&spec, Os::Linux),
            Some("apt install x".into())
        );
        assert_eq!(install_command(&spec, Os::Macos), None);
    }
    #[test]
    fn version_ok_works() {
        assert!(version_ok(Some("1.90"), Some("1.80")));
        assert!(!version_ok(Some("1.70"), Some("1.80")));
        assert!(version_ok(None, None));
    }
    #[test]
    fn dry_run_does_not_execute() {
        assert_eq!(
            install_command(
                &ToolSpec {
                    name: "x".into(),
                    min_version: None,
                    install: Some(InstallCmds {
                        linux: Some("echo hi".into()),
                        macos: None,
                        windows: None
                    })
                },
                Os::Linux
            ),
            Some("echo hi".into())
        );
    }
}
