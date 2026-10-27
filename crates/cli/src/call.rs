use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

pub fn parse_target(target: &str) -> Result<(String, String)> {
    let (plugin, func) = target.split_once('.').ok_or_else(|| {
        anyhow::anyhow!(
            "invalid target `{target}`: expected `<plugin>.<function>` (e.g. core-rust.ping)"
        )
    })?;
    if plugin.is_empty() || func.is_empty() || func.contains('.') {
        anyhow::bail!(
            "invalid target `{target}`: expected `<plugin>.<function>` (e.g. core-rust.ping)"
        );
    }
    Ok((plugin.to_string(), func.to_string()))
}

pub fn parse_args_value(input: &str) -> Result<Vec<Value>> {
    let v: Value =
        serde_json::from_str(input).with_context(|| format!("invalid JSON in --args: {input}"))?;
    match v {
        Value::Array(arr) => Ok(arr),
        other => anyhow::bail!(
            "--args must be a JSON array of arguments, got {} (hint: wrap in [...], e.g. --args '[[\"feat\"],9,10]')",
            other_type_name(&other)
        ),
    }
}

fn other_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub fn load_args(spec: &str) -> Result<Vec<Value>> {
    let raw = if spec == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read --args from stdin")?;
        buf
    } else if let Some(path) = spec.strip_prefix('@') {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read --args file `{path}`"))?
    } else {
        spec.to_string()
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    parse_args_value(trimmed)
}

pub fn normalize_base(addr: &str) -> String {
    let a = addr.trim();
    if a.starts_with("http://") || a.starts_with("https://") {
        a.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", a.trim_end_matches('/'))
    }
}

pub async fn cmd_call(
    config: &Path,
    http_addr: Option<&str>,
    target: &str,
    args_spec: &str,
    timeout_secs: u64,
    json: bool,
) -> Result<()> {
    use crate::config::read_host_addr;
    let addr = http_addr
        .map(|s| s.to_string())
        .or_else(|| read_host_addr(config))
        .unwrap_or_else(|| "127.0.0.1:8787".to_string());
    let base = normalize_base(&addr);
    let (plugin, func) = parse_target(target)?;
    let args = load_args(args_spec)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("failed to build HTTP client")?;
    let url = format!("{base}/plugin/{plugin}/call");
    let body = serde_json::json!({ "function": func, "args": args });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("request to {url} failed"))?;

    let status = resp.status();
    let text = resp.text().await.context("failed to read response body")?;

    if !status.is_success() {
        eprintln!("HTTP {status}");
        if !text.trim().is_empty() {
            eprintln!("{text}");
        }
        anyhow::bail!("call failed with HTTP {status}");
    }

    let value: Value =
        serde_json::from_str(&text).with_context(|| format!("invalid JSON response: {text}"))?;

    if json {
        println!("{}", text.trim());
    } else {
        println!("{}", serde_json::to_string_pretty(&value).unwrap_or(text));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_ok() {
        assert_eq!(
            parse_target("core-rust.ping").unwrap(),
            ("core-rust".into(), "ping".into())
        );
    }

    #[test]
    fn parse_target_rejects() {
        assert!(parse_target("nope").is_err());
        assert!(parse_target(".fn").is_err());
        assert!(parse_target("plugin.").is_err());
        assert!(parse_target("a.b.c").is_err());
    }

    #[test]
    fn parse_args_inline_array() {
        assert_eq!(
            parse_args_value("[1,2]").unwrap(),
            vec![serde_json::json!(1), serde_json::json!(2)]
        );
    }

    #[test]
    fn parse_args_rejects_non_array() {
        let err = parse_args_value("{\"a\":1}").unwrap_err().to_string();
        assert!(err.contains("JSON array"), "{err}");
    }

    #[test]
    fn parse_args_empty_string_defaults_empty() {
        assert!(load_args("").is_ok());
    }

    #[test]
    fn load_args_file() {
        let dir = std::env::temp_dir();
        let p = dir.join("args_dl03.json");
        std::fs::write(&p, "[\"hi\", 5]").unwrap();
        let spec = format!("@{}", p.display());
        assert_eq!(
            load_args(&spec).unwrap(),
            vec![serde_json::json!("hi"), serde_json::json!(5)]
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn normalize_base_handles_scheme() {
        assert_eq!(normalize_base("127.0.0.1:8787"), "http://127.0.0.1:8787");
        assert_eq!(normalize_base("http://x/"), "http://x");
    }
}
