use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize, Debug)]
struct LogsRes {
    logs: Vec<String>,
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
        let url = format!("{}/plugin/{}/logs", base(&addr), plugin);
        let res = client.get(&url).send().await.context("request failed")?;
        if !res.status().is_success() {
            anyhow::bail!("host returned {}", res.status());
        }
        let body: LogsRes = res.json().await.context("invalid json")?;
        let filtered: Vec<String> = body
            .logs
            .into_iter()
            .filter(|l| level_ok(l, level))
            .collect();
        if json {
            println!("{}", serde_json::to_string(&filtered)?);
        } else {
            for l in filtered {
                println!("{}", l);
            }
        }
        return Ok(());
    }
    // follow SSE
    let url = format!("{}/plugin/{}/logs/stream", base(&addr), plugin);
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
                if !level_ok(&msg, level) {
                    continue;
                }
                if json {
                    println!("{}", serde_json::to_string(&msg)?);
                } else {
                    println!("{}", msg);
                }
            }
        }
    }
    Ok(())
}

fn level_ok(msg: &str, level: Option<&str>) -> bool {
    let Some(lv) = level else {
        return true;
    };
    let lv = lv.to_lowercase();
    // filter: keep messages whose inferred level >= requested
    let rank = |m: &str| -> i32 {
        let low = m.to_lowercase();
        if low.contains("error") {
            3
        } else if low.contains("warn") {
            2
        } else if low.contains("info") {
            1
        } else if low.contains("debug") {
            0
        } else {
            1
        }
    };
    let thr = match lv.as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" | "warning" => 2,
        "error" => 3,
        _ => 0,
    };
    rank(msg) >= thr
}
