use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Deserialize, Debug, Clone, Serialize)]
struct LogEntry {
    ts: u64,
    level: String,
    plugin: String,
    request_id: u64,
    message: String,
    #[serde(default)]
    fields: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct LogsRes {
    #[serde(default)]
    logs: Vec<String>,
    #[serde(default)]
    entries: Vec<LogEntry>,
}

fn base(addr: &str) -> String {
    format!("http://{}", addr.trim_start_matches("http://"))
}

fn addr(cli_addr: Option<&str>, config_addr: Option<&str>) -> String {
    cli_addr
        .map(|s| s.to_string())
        .or_else(|| config_addr.map(|s| s.to_string()))
        .unwrap_or_else(|| "127.0.0.1:8787".to_string())
}

fn level_rank(level: &str) -> i32 {
    match level.to_lowercase().as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" | "warning" => 2,
        "error" => 3,
        _ => 1,
    }
}

fn level_ok_entry(entry: &LogEntry, level: Option<&str>) -> bool {
    let Some(lv) = level else {
        return true;
    };
    let thr = match lv.to_lowercase().as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" | "warning" => 2,
        "error" => 3,
        _ => 0,
    };
    level_rank(&entry.level) >= thr
}

pub async fn run_logs(
    plugin: &str,
    follow: bool,
    level: Option<&str>,
    json: bool,
    http_addr: Option<&str>,
    config_addr: Option<&str>,
) -> Result<()> {
    let addr = addr(http_addr, config_addr);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    // history
    if !follow {
        let mut url = format!("{}/plugin/{}/logs", base(&addr), plugin);
        if let Some(lv) = level {
            url = format!("{}?level={}", url, lv);
        }
        let res = client.get(&url).send().await.context("request failed")?;
        if !res.status().is_success() {
            anyhow::bail!("host returned {}", res.status());
        }
        let body: LogsRes = res.json().await.context("invalid json")?;
        let entries: Vec<LogEntry> = body
            .entries
            .into_iter()
            .filter(|e| level_ok_entry(e, level))
            .collect();
        if entries.is_empty() && body.logs.is_empty() {
            // compat fallback
        }
        if json {
            for e in &entries {
                println!("{}", serde_json::to_string(e)?);
            }
            if entries.is_empty() {
                // fallback to old logs strings
                for l in &body.logs {
                    println!("{}", serde_json::to_string(l)?);
                }
            }
        } else {
            if !entries.is_empty() {
                for e in &entries {
                    println!("[{}] {} {} {}", e.level, e.ts, e.request_id, e.message);
                }
            } else {
                for l in &body.logs {
                    println!("{}", l);
                }
            }
        }
        return Ok(());
    }
    // follow SSE
    let mut url = format!("{}/plugin/{}/logs/stream", base(&addr), plugin);
    if let Some(lv) = level {
        url = format!("{}?level={}", url, lv);
    }
    let res = client.get(&url).send().await.context("request failed")?;
    if !res.status().is_success() {
        anyhow::bail!("host returned {}", res.status());
    }
    use futures_util::StreamExt;
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("stream error")?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = buf.find('\n') {
            let line = buf[..idx].to_string();
            buf = buf[idx + 1..].to_string();
            if let Some(data) = line.strip_prefix("data:") {
                let msg = data.trim_start().to_string();
                if msg.is_empty() {
                    continue;
                }
                // try parse as LogEntry
                if let Ok(entry) = serde_json::from_str::<LogEntry>(&msg) {
                    if !level_ok_entry(&entry, level) {
                        continue;
                    }
                    if json {
                        println!("{}", serde_json::to_string(&entry)?);
                    } else {
                        println!(
                            "[{}] {} {} {}",
                            entry.level, entry.ts, entry.request_id, entry.message
                        );
                    }
                } else {
                    if json {
                        println!("{}", serde_json::to_string(&msg)?);
                    } else {
                        println!("{}", msg);
                    }
                }
            }
        }
    }
    Ok(())
}
