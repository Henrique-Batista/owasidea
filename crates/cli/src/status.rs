use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Deserialize, Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(rename = "plugin_type")]
    pub plugin_type: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct OverviewRes {
    pub build_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct ServiceStatus {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

fn base(addr: &str) -> String {
    format!("http://{}", addr.trim_start_matches("http://"))
}

pub async fn fetch_status(addr: &str) -> Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let health: String = client
        .get(format!("{}/health", base(addr)))
        .send()
        .await?
        .text()
        .await
        .unwrap_or_default();
    let overview: OverviewRes = client
        .get(format!("{}/overview", base(addr)))
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    let plugins: Vec<PluginInfo> = client
        .get(format!("{}/plugins", base(addr)))
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();

    let mut plugin_out = Vec::new();
    let mut services = Vec::new();
    for p in &plugins {
        let kind = p.kind.clone().or(p.plugin_type.clone()).unwrap_or_default();
        let runtime = p
            .runtime
            .clone()
            .unwrap_or_else(|| "wasi-component".to_string());
        let mut status = "loaded".to_string();
        if kind == "service" || kind == "hybrid" {
            let url = format!("{}/plugin/{}/service/status", base(addr), p.name);
            match client.get(url).send().await {
                Ok(r) => {
                    let s: ServiceStatus = r.json().await.unwrap_or_default();
                    status = s.state.clone().unwrap_or_else(|| "unknown".to_string());
                    services.push(json!({
                        "name": p.name,
                        "state": s.state,
                        "error": s.error,
                    }));
                }
                Err(_) => {
                    status = "unknown".to_string();
                }
            }
        }
        plugin_out.push(json!({
            "name": p.name,
            "type": kind,
            "runtime": runtime,
            "status": status,
        }));
    }

    Ok(json!({
        "health": health.trim(),
        "build_id": overview.build_id,
        "plugins": plugin_out,
        "services": services,
    }))
}

pub async fn cmd_status(addr: &str, watch: bool, json: bool) -> Result<()> {
    if !watch {
        match fetch_status(addr).await {
            Ok(v) => {
                if json {
                    println!("{}", v);
                } else {
                    println!("health: {}", v["health"]);
                    println!("build_id: {}", v["build_id"].as_str().unwrap_or("-"));
                    println!("plugins:");
                    for p in v["plugins"].as_array().cloned().unwrap_or_default() {
                        println!(
                            "  {} ({}/{}) - {}",
                            p["name"], p["type"], p["runtime"], p["status"]
                        );
                    }
                    println!("services:");
                    for s in v["services"].as_array().cloned().unwrap_or_default() {
                        println!("  {} - {}", s["name"], s["state"].as_str().unwrap_or("-"));
                    }
                }
                Ok(())
            }
            Err(e) => {
                if json {
                    println!(
                        "{}",
                        json!({"health":"unreachable","build_id":null,"plugins":[],"services":[]})
                    );
                } else {
                    eprintln!("host unreachable: {}", e);
                }
                std::process::exit(1);
            }
        }
    } else {
        loop {
            let v = fetch_status(addr).await.unwrap_or_else(
                |_| json!({"health":"unreachable","build_id":null,"plugins":[],"services":[]}),
            );
            if json {
                println!("{}", v);
            } else {
                print!("\x1B[2J\x1B[H");
                println!("health: {}", v["health"]);
                println!("build_id: {}", v["build_id"].as_str().unwrap_or("-"));
                for p in v["plugins"].as_array().cloned().unwrap_or_default() {
                    println!(
                        "  {} ({}/{}) - {}",
                        p["name"], p["type"], p["runtime"], p["status"]
                    );
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}
