//! HTTP client used by the TUI. It only talks to the public host API, so the
//! dashboard behaves the same whether the host is a child of `owasidea dev` or
//! a separately started `owasidea run`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceRow {
    pub running: bool,
    pub address: Option<String>,
    pub last_error: Option<String>,
    pub restarts: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginRow {
    pub name: String,
    pub kind: String,
    pub runtime: String,
    pub language: String,
    pub exports: Vec<String>,
    pub served_on: Option<String>,
    pub service: Option<ServiceRow>,
}

impl PluginRow {
    pub fn is_service(&self) -> bool {
        matches!(self.kind.as_str(), "service" | "hybrid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadRecord {
    pub plugin: String,
    pub ok: bool,
    pub http_status: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub build_id: Option<String>,
    pub watch: bool,
    pub hot_reload: bool,
    pub plugins: Vec<PluginRow>,
    pub last_reload: Vec<ReloadRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub ts: u64,
    pub level: String,
    pub request_id: u64,
    pub message: String,
}

#[derive(Deserialize)]
struct PluginWire {
    name: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    runtime: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    exports: Vec<String>,
    #[serde(default)]
    served_on: Option<String>,
}

#[derive(Deserialize, Default)]
struct OverviewWire {
    #[serde(default)]
    build_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct DevWire {
    #[serde(default)]
    active: bool,
    #[serde(default)]
    hot_reload: bool,
    #[serde(default)]
    last_reload: BTreeMap<String, ReloadWire>,
}

#[derive(Deserialize)]
struct ReloadWire {
    ok: bool,
    #[serde(default)]
    http_status: u16,
}

#[derive(Deserialize, Default)]
struct ServiceWire {
    #[serde(default)]
    running: bool,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    restarts: u64,
}

#[derive(Deserialize)]
struct LogsWire {
    #[serde(default)]
    entries: Vec<LogWire>,
}

#[derive(Deserialize)]
struct LogWire {
    #[serde(default)]
    ts: u64,
    #[serde(default)]
    level: String,
    #[serde(default)]
    request_id: u64,
    #[serde(default)]
    message: String,
}

/// Interface exports (`pkg:ns/iface@ver`) are not callable through `/call`,
/// so the call view lists only plain function names.
pub fn callable_exports(exports: Vec<String>) -> Vec<String> {
    exports
        .into_iter()
        .filter(|export| !export.contains(':'))
        .collect()
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    pub fn new(host_url: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            http,
            base: host_url.trim_end_matches('/').to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .http
            .get(self.url(path))
            .send()
            .await
            .with_context(|| format!("GET {path} failed"))?;
        let status = response.status();
        if !status.is_success() {
            bail!("GET {path}: {status}");
        }
        response
            .json()
            .await
            .with_context(|| format!("GET {path}: invalid JSON"))
    }

    async fn send_text(&self, request: reqwest::RequestBuilder) -> Result<String> {
        let response = request.send().await.context("request failed")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let detail = body.trim();
            if detail.is_empty() {
                bail!("{status}");
            }
            bail!("{status}: {detail}");
        }
        Ok(body.trim().to_string())
    }

    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.http
            .get(self.url("/health"))
            .send()
            .await
            .context("host unreachable")?
            .error_for_status()
            .context("host unhealthy")?;
        let overview: OverviewWire = self.get_json("/overview").await.unwrap_or_default();
        let dev: DevWire = self.get_json("/dev/status").await.unwrap_or_default();
        let wires: Vec<PluginWire> = self.get_json("/plugins").await?;

        let mut plugins = Vec::with_capacity(wires.len());
        for wire in wires {
            let runtime = if wire.runtime.is_empty() {
                "wasi-component".to_string()
            } else {
                wire.runtime
            };
            let exports = callable_exports(wire.exports);
            let mut row = PluginRow {
                name: wire.name,
                kind: wire.kind,
                runtime,
                language: wire.language,
                exports,
                served_on: wire.served_on,
                service: None,
            };
            if row.is_service() {
                row.service = Some(self.service_status(&row.name).await);
            }
            plugins.push(row);
        }

        Ok(Snapshot {
            build_id: overview.build_id,
            watch: dev.active,
            hot_reload: dev.hot_reload,
            plugins,
            last_reload: dev
                .last_reload
                .into_iter()
                .map(|(plugin, record)| ReloadRecord {
                    plugin,
                    ok: record.ok,
                    http_status: record.http_status,
                })
                .collect(),
        })
    }

    async fn service_status(&self, name: &str) -> ServiceRow {
        match self
            .get_json::<ServiceWire>(&format!("/plugin/{name}/service/status"))
            .await
        {
            Ok(status) => ServiceRow {
                running: status.running,
                address: status.address,
                last_error: status.last_error,
                restarts: status.restarts,
            },
            Err(error) => ServiceRow {
                running: false,
                address: None,
                last_error: Some(format!("status unavailable: {error}")),
                restarts: 0,
            },
        }
    }

    pub async fn manifest(&self, name: &str) -> Result<Value> {
        self.get_json(&format!("/plugin/{name}/manifest")).await
    }

    pub async fn logs(&self, name: &str) -> Result<Vec<LogLine>> {
        let body: LogsWire = self.get_json(&format!("/plugin/{name}/logs")).await?;
        Ok(body
            .entries
            .into_iter()
            .map(|entry| LogLine {
                ts: entry.ts,
                level: entry.level,
                request_id: entry.request_id,
                message: entry.message,
            })
            .collect())
    }

    pub async fn reload(&self, name: &str) -> Result<String> {
        self.send_text(self.http.post(self.url(&format!("/plugin/{name}/reload"))))
            .await
    }

    pub async fn service_start(&self, name: &str) -> Result<String> {
        self.send_text(
            self.http
                .post(self.url(&format!("/plugin/{name}/service/start")))
                .json(&serde_json::json!({ "region_json": "" })),
        )
        .await
    }

    pub async fn service_stop(&self, name: &str) -> Result<String> {
        self.send_text(
            self.http
                .post(self.url(&format!("/plugin/{name}/service/stop"))),
        )
        .await
    }

    pub async fn call(&self, name: &str, function: &str, args: Vec<Value>) -> Result<Value> {
        let response = self
            .http
            .post(self.url(&format!("/plugin/{name}/call")))
            .json(&serde_json::json!({ "function": function, "args": args }))
            .send()
            .await
            .context("call request failed")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{status}: {}", body.trim());
        }
        Ok(serde_json::from_str(&body).unwrap_or(Value::String(body)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_kinds_are_detected() {
        let mut row = PluginRow {
            kind: "service".into(),
            ..Default::default()
        };
        assert!(row.is_service());
        row.kind = "hybrid".into();
        assert!(row.is_service());
        row.kind = "library".into();
        assert!(!row.is_service());
    }

    #[test]
    fn interface_exports_are_not_callable() {
        let exports = vec!["greet".to_string(), "poc:notes/api@0.1.0".to_string()];
        assert_eq!(callable_exports(exports), vec!["greet".to_string()]);
    }

    #[test]
    fn client_trims_trailing_slash() {
        let client = Client::new("http://127.0.0.1:8787/").unwrap();
        assert_eq!(client.url("/health"), "http://127.0.0.1:8787/health");
    }
}
