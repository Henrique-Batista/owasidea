use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware,
    response::{
        sse::{Event, KeepAlive, Sse},
        Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{sync::Mutex, task::JoinHandle};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::Instrument;
use wasmtime::component::{Component, Linker, ResourceTable, Type, Val};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{
    sockets::SocketAddrUse, FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView,
};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

mod otel;
mod plugin_metrics;

// Experimental long-call jobs (ADR-0005, RL-06). Off by default so the stable
// `/call` surface and its routes are byte-for-byte unchanged.
#[cfg(feature = "streaming")]
mod jobs;

// Resource-limit coverage (FIX-01): fuel, timeout, memory and instance slots.
#[cfg(test)]
mod resource_limits_tests;

// ---------------------------------------------------------------------------
// Host ABI version (CT-08)
// ---------------------------------------------------------------------------

pub const HOST_ABI: &str = "1";
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default timeout for one library call when the manifest does not set
/// `permissions.resources.timeout_ms`.
pub(crate) const DEFAULT_CALL_TIMEOUT_MS: u64 = 30_000;
/// Hard ceiling for long-call jobs (ADR-0005). Manifests may reduce it.
#[cfg(feature = "streaming")]
pub(crate) const MAX_CALL_TIMEOUT_MS: u64 = 300_000;

// ---------------------------------------------------------------------------
// Manifest (subset used by the POC host)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Manifest {
    name: String,
    version: String,
    language: String,
    abi: String,
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    host_abi: Option<String>,
    #[serde(default)]
    min_host: Option<String>,
    #[serde(default)]
    runtime: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    component: String,
    #[serde(default)]
    exports: Vec<Export>,
    #[serde(default)]
    imports: Imports,
    #[serde(default)]
    permissions: Permissions,
    #[serde(default)]
    state: StateCfg,
    #[serde(default)]
    composition: Composition,
    #[serde(default)]
    metadata: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ExportParam {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    ty: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ExportResult {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    ty: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Export {
    name: String,
    signature: String,
    #[serde(default)]
    params: Vec<ExportParam>,
    #[serde(default)]
    results: Vec<ExportResult>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct Imports {
    #[serde(default)]
    wasi: Vec<String>,
    #[serde(default)]
    host: Vec<String>,
    #[serde(default)]
    plugins: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct Permissions {
    #[serde(default)]
    filesystem: Vec<FsPerm>,
    #[serde(default)]
    http: HttpPerm,
    #[serde(default)]
    network: NetworkPerm,
    #[serde(default)]
    kv: Option<KvPerm>,
    #[serde(default)]
    resources: Resources,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct NetworkPerm {
    /// Outbound hostnames or IP addresses. Hostnames are resolved at instance
    /// creation and sockets are restricted to the resulting addresses.
    #[serde(default)]
    outbound: Vec<String>,
    /// Local IP addresses the component may bind/listen on. Use explicit
    /// wildcard addresses only when external reachability is intended.
    #[serde(default)]
    inbound: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct FsPerm {
    path: String,
    access: String,
    #[serde(default = "default_true")]
    preopen: bool,
}

fn default_true() -> bool {
    true
}
fn default_schema_version() -> u32 {
    0
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct HttpPerm {
    #[serde(default)]
    outbound: Vec<String>,
    #[serde(default)]
    inbound: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct KvPerm {
    namespace: String,
    access: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct Resources {
    #[serde(default)]
    max_memory_mb: Option<u64>,
    #[serde(default)]
    max_fuel: Option<u64>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_instances: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct StateCfg {
    #[serde(default)]
    persistent: bool,
    #[serde(default)]
    storage: Option<String>,
    #[serde(default)]
    root: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct Composition {
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    entry: bool,
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

struct PluginRuntime {
    manifest: Manifest,
    manifest_path: PathBuf,
    /// Compiled once when the plugin is loaded or reloaded. Wasmtime component
    /// compilation dominates call latency for Go/JS/C# components, so it must
    /// not happen on every /call.
    component: Component,
    kv: Arc<std::sync::Mutex<rusqlite::Connection>>,
    logs: Arc<Mutex<LogBuffer>>,
    instance_slots: Arc<tokio::sync::Semaphore>,
    http_pre: Option<wasmtime_wasi_http::p2::bindings::ProxyPre<HostState>>,
}

const MAX_LOG_ENTRIES: usize = 1_000;
const MAX_LOG_CHARS: usize = 1024;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: u64,
    pub level: String,
    pub plugin: String,
    pub request_id: u64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
}

struct LogBuffer {
    plugin: String,
    next_id: u64,
    entries: VecDeque<(u64, LogEntry)>,
}

impl LogBuffer {
    fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
            next_id: 0,
            entries: VecDeque::new(),
        }
    }

    fn push(
        &mut self,
        level: &str,
        message: String,
        request_id: u64,
        fields: Option<serde_json::Value>,
    ) {
        self.next_id += 1;
        let message = message.chars().take(MAX_LOG_CHARS).collect();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let entry = LogEntry {
            ts,
            level: level.to_string(),
            plugin: self.plugin.clone(),
            request_id,
            message,
            fields,
        };
        self.entries.push_back((self.next_id, entry));
        if self.entries.len() > MAX_LOG_ENTRIES {
            self.entries.pop_front();
        }
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
}

struct TailcatSession {
    store: Store<HostState>,
    instance: wasmtime::component::Instance,
    address: Option<String>,
    running: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

struct ServiceSession {
    store: Store<HostState>,
    instance: wasmtime::component::Instance,
    timeout_ms: u64,
    address: Option<String>,
}

/// Lifecycle outcome for one generic service plugin. It outlives individual
/// component instances so `/service/status` can explain the last failure after
/// the instance has been dropped.
#[derive(Debug, Clone, Default)]
struct ServiceHealth {
    last_error: Option<String>,
    restarts: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum StartOutcome {
    Started(String),
    AlreadyRunning,
}

#[derive(Clone)]
struct AppState {
    engine: Engine,
    plugins: Arc<std::sync::RwLock<HashMap<String, std::sync::Arc<PluginRuntime>>>>,
    ui_plugins: Arc<Vec<UiPluginInfo>>,
    tailcat_session: Arc<Mutex<Option<TailcatSession>>>,
    services: Arc<Mutex<HashMap<String, ServiceSession>>>,
    service_health: Arc<Mutex<HashMap<String, ServiceHealth>>>,
    requests: Arc<std::sync::atomic::AtomicU64>,
    errors: Arc<std::sync::atomic::AtomicU64>,
    plugin_metrics: Arc<plugin_metrics::PluginMetricsMap>,
    dev_mode: bool,
    dev_hot_reload: bool,
    dev_watch_paths: Vec<String>,
    dev_last_reload: Arc<std::sync::RwLock<std::collections::BTreeMap<String, ReloadStatus>>>,
    manifests_dir: PathBuf,
    #[cfg(feature = "streaming")]
    jobs: jobs::JobManager,
}

#[derive(Debug, Clone, Serialize)]
struct ReloadStatus {
    ok: bool,
    http_status: u16,
    timestamp_unix: u64,
}

#[derive(Serialize)]
struct DevStatusRes {
    active: bool,
    hot_reload: bool,
    watch_paths: Vec<String>,
    last_reload: std::collections::BTreeMap<String, ReloadStatus>,
}

impl AppState {
    fn plugin(&self, name: &str) -> Option<std::sync::Arc<PluginRuntime>> {
        self.plugins.read().ok()?.get(name).cloned()
    }
}

struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
    http_hooks: AllowlistHooks,
    limits: wasmtime::StoreLimits,
    _instance_permit: tokio::sync::OwnedSemaphorePermit,
    request_id: u64,
}

#[derive(Clone)]
struct AllowlistHooks {
    allowed: Vec<String>,
}

impl wasmtime_wasi_http::WasiHttpHooks for AllowlistHooks {
    fn send_request(
        &mut self,
        request: http::Request<wasmtime_wasi_http::WasiBody>,
        options: Option<wasmtime_wasi_http::RequestOptions>,
        fut: Box<
            dyn std::future::Future<
                    Output = wasmtime_wasi_http::Result<(), wasmtime_wasi_http::Error>,
                > + Send,
        >,
    ) -> Box<
        dyn std::future::Future<
                Output = wasmtime_wasi_http::Result<(
                    http::Response<wasmtime_wasi_http::WasiBody>,
                    Box<
                        dyn std::future::Future<
                                Output = wasmtime_wasi_http::Result<(), wasmtime_wasi_http::Error>,
                            > + Send,
                    >,
                )>,
            > + Send,
    > {
        let host = request.uri().host().unwrap_or("").to_string();
        let allowed = self.allowed.iter().any(|a| a.eq_ignore_ascii_case(&host));
        if !allowed {
            _ = fut;
            return Box::new(async move { Err(wasmtime_wasi_http::Error::HttpRequestDenied) });
        }
        Box::new(async move {
            use http_body_util::BodyExt;
            let (res, io) = wasmtime_wasi_http::default_send_request(request, options).await?;
            Ok((
                res.map(BodyExt::boxed_unsync),
                Box::new(io) as Box<dyn std::future::Future<Output = _> + Send>,
            ))
        })
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.http_hooks,
        }
    }
}

impl wasmtime::ResourceLimiter for HostState {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        self.limits.memory_growing(current, desired, maximum)
    }
    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> Result<(), wasmtime::Error> {
        self.limits.memory_grow_failed(error)
    }
    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        self.limits.table_growing(current, desired, maximum)
    }
    fn table_grow_failed(&mut self, error: wasmtime::Error) -> Result<(), wasmtime::Error> {
        self.limits.table_grow_failed(error)
    }
}

// ---------------------------------------------------------------------------
// Component instantiation
// ---------------------------------------------------------------------------

fn build_linker(engine: &Engine, plugin: &PluginRuntime) -> Result<Linker<HostState>> {
    let mut linker: Linker<HostState> = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)
        .map_err(|e| anyhow!("failed to add WASI p2 to linker: {e}"))?;
    wasmtime_wasi::p3::add_to_linker(&mut linker)
        .map_err(|e| anyhow!("failed to add WASI p3 to linker: {e}"))?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
        .map_err(|e| anyhow!("failed to add WASI HTTP to linker: {e}"))?;

    // Only expose host interfaces the manifest declares. Instantiation fails
    // if the component imports anything else, catching capability escalation.
    if plugin
        .manifest
        .imports
        .host
        .iter()
        .any(|h| h.starts_with("poc:host/host-kv"))
    {
        let can_read = plugin
            .manifest
            .permissions
            .kv
            .as_ref()
            .is_some_and(|permission| matches!(permission.access.as_str(), "read" | "read-write"));
        let can_write = plugin
            .manifest
            .permissions
            .kv
            .as_ref()
            .is_some_and(|permission| permission.access == "read-write");
        let kv = plugin.kv.clone();
        let mut root = linker.root();
        let mut inst = root
            .instance("poc:host/host-kv@0.1.0")
            .map_err(|e| anyhow!("missing host-kv interface: {e}"))?;
        let kv_get = kv.clone();
        inst.func_wrap(
            "get",
            move |_store: wasmtime::StoreContextMut<'_, HostState>, (key,): (String,)| {
                if !can_read {
                    return Ok((None::<String>,));
                }
                let value = kv_get.lock().ok().and_then(|conn| {
                    conn.query_row(
                        "SELECT value FROM kv WHERE key = ?1",
                        rusqlite::params![key],
                        |r| r.get::<_, String>(0),
                    )
                    .ok()
                });
                Ok((value,))
            },
        )?;
        let kv_set = kv.clone();
        inst.func_wrap(
            "set",
            move |_store: wasmtime::StoreContextMut<'_, HostState>,
                  (key, value): (String, String)| {
                if can_write {
                    if let Ok(conn) = kv_set.lock() {
                        let _ = conn.execute(
                            "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
                            rusqlite::params![key, value],
                        );
                    }
                }
                Ok(())
            },
        )?;
    }

    if plugin
        .manifest
        .imports
        .host
        .iter()
        .any(|h| h.starts_with("poc:host/host-log"))
    {
        let logs = plugin.logs.clone();
        let mut root = linker.root();
        let mut inst = root
            .instance("poc:host/host-log@0.1.0")
            .map_err(|e| anyhow!("missing host-log interface: {e}"))?;
        inst.func_wrap(
            "log",
            move |store: wasmtime::StoreContextMut<'_, HostState>, (msg,): (String,)| {
                let rid = store.data().request_id;
                if let Ok(mut l) = logs.try_lock() {
                    l.push("info", msg, rid, None);
                }
                Ok(())
            },
        )?;
    }

    Ok(linker)
}

async fn instantiate(
    engine: &Engine,
    plugin: &PluginRuntime,
) -> Result<(Store<HostState>, wasmtime::component::Instance)> {
    let linker = build_linker(engine, plugin)?;
    let mut store = create_store(engine, plugin).await?;
    let instance = linker
        .instantiate_async(&mut store, &plugin.component)
        .await
        .map_err(|e| anyhow!("failed to instantiate component: {e}"))?;
    Ok((store, instance))
}

/// Verify the component bytes against the signed manifest and compile them.
/// Callers cache the returned component; never call this per invocation.
fn verify_and_compile_component(
    engine: &Engine,
    manifest: &Manifest,
    manifest_value: &serde_json::Value,
    component_path: &Path,
) -> Result<Component> {
    if !manifest.imports.plugins.is_empty() {
        anyhow::bail!(
            "plugin {} declares unresolved plugin imports; compose dependencies at build time",
            manifest.name
        );
    }
    if manifest
        .permissions
        .http
        .outbound
        .iter()
        .any(|host| host == "*")
    {
        anyhow::bail!(
            "wildcard HTTP permissions are not allowed for {}",
            manifest.name
        );
    }
    let bytes = std::fs::read(component_path).map_err(|e| anyhow!("read component: {e}"))?;
    // Verify component checksum if declared in manifest metadata.
    if let Some(expected) = manifest.metadata.get("sha256").and_then(|v| v.as_str()) {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            return Err(anyhow!(
                "checksum mismatch for {}: expected {}, got {}",
                manifest.name,
                expected,
                actual
            ));
        }
    }
    verify_manifest_signature(manifest_value, &bytes)?;
    let component = Component::from_file(engine, component_path)
        .map_err(|e| anyhow!("failed to load component {component_path:?}: {e}"))?;
    validate_component_imports(engine, &component, manifest)?;
    Ok(component)
}

fn prepare_http_plugin(
    engine: &Engine,
    plugin: &PluginRuntime,
) -> Result<Option<wasmtime_wasi_http::p2::bindings::ProxyPre<HostState>>> {
    if !matches!(plugin.manifest.kind.as_str(), "executable" | "hybrid") {
        return Ok(None);
    }
    if !plugin.manifest.permissions.http.inbound {
        anyhow::bail!(
            "HTTP plugin {} lacks permissions.http.inbound",
            plugin.manifest.name
        );
    }
    if !plugin
        .manifest
        .exports
        .iter()
        .any(|export| export.name.starts_with("wasi:http/incoming-handler"))
    {
        anyhow::bail!(
            "HTTP plugin {} does not export wasi:http/incoming-handler",
            plugin.manifest.name
        );
    }
    let linker = build_linker(engine, plugin)?;
    Ok(Some(wasmtime_wasi_http::p2::bindings::ProxyPre::new(
        linker.instantiate_pre(&plugin.component)?,
    )?))
}

async fn create_store(engine: &Engine, plugin: &PluginRuntime) -> Result<Store<HostState>> {
    let timeout_ms = plugin
        .manifest
        .permissions
        .resources
        .timeout_ms
        .unwrap_or(30_000);
    let permit = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        plugin.instance_slots.clone().acquire_owned(),
    )
    .await
    .map_err(|_| anyhow!("plugin instance limit wait timed out"))??;
    let mut builder = WasiCtxBuilder::new();
    if plugin.manifest.name == "tailcat-wasi" {
        let (gateway, self_ip) = network_hints();
        if let Some(gateway) = gateway {
            builder.env("TAILCAT_WASI_GATEWAY", gateway);
        }
        if let Some(self_ip) = self_ip {
            builder.env("TAILCAT_WASI_SELF_IP", self_ip);
        }
        for key in ["TAILCAT_WASI_VERBOSE_PORTMAPPER", "TAILCAT_WASI_LOG_HTTP"] {
            if let Ok(value) = std::env::var(key) {
                builder.env(key, value);
            }
        }
    }

    let has_tcp = plugin
        .manifest
        .imports
        .wasi
        .iter()
        .any(|w| w.starts_with("wasi:sockets/tcp"));
    let has_udp = plugin
        .manifest
        .imports
        .wasi
        .iter()
        .any(|w| w.starts_with("wasi:sockets/udp"));
    let has_name_lookup = plugin
        .manifest
        .imports
        .wasi
        .iter()
        .any(|w| w.starts_with("wasi:sockets/ip-name-lookup"));
    if has_tcp || has_udp || has_name_lookup {
        let outbound = resolve_network_hosts(&plugin.manifest.permissions.network.outbound).await?;
        let inbound = parse_inbound_addresses(&plugin.manifest.permissions.network.inbound)?;
        let outbound_for_check = outbound.clone();
        let inbound_for_check = inbound.clone();
        builder
            .allow_tcp(has_tcp && (!outbound.is_empty() || !inbound.is_empty()))
            .allow_udp(has_udp && (!outbound.is_empty() || !inbound.is_empty()))
            .allow_ip_name_lookup(has_name_lookup && !outbound.is_empty())
            .socket_addr_check(move |addr, usage| {
                let outbound = outbound_for_check.clone();
                let inbound = inbound_for_check.clone();
                Box::pin(async move { socket_address_allowed(addr, usage, &outbound, &inbound) })
            });
    }
    for fs in &plugin.manifest.permissions.filesystem {
        if !fs.preopen {
            continue;
        }
        let host_path = resolve_state_path(&fs.path);
        std::fs::create_dir_all(&host_path)
            .with_context(|| format!("create filesystem permission path {}", fs.path))?;
        let host_path = std::fs::canonicalize(&host_path)
            .with_context(|| format!("canonicalize filesystem permission path {}", fs.path))?;
        let perms = match fs.access.as_str() {
            "read" => FsPerms::ReadOnly,
            "read-write" => FsPerms::ReadWrite,
            other => anyhow::bail!("unsupported filesystem access {other:?} for {}", fs.path),
        };
        builder
            .preopened_dir(&host_path, &fs.path, perms)
            .map_err(|e| anyhow!("failed to preopen {}: {e}", fs.path))?;
    }

    let mut store = Store::new(
        engine,
        HostState {
            wasi: builder.build(),
            table: ResourceTable::new(),
            http: WasiHttpCtx::new(),
            http_hooks: AllowlistHooks {
                allowed: plugin.manifest.permissions.http.outbound.clone(),
            },
            limits: {
                let mut b = wasmtime::StoreLimitsBuilder::new();
                let mb = plugin
                    .manifest
                    .permissions
                    .resources
                    .max_memory_mb
                    .unwrap_or(64);
                let bytes = mb
                    .checked_mul(1024 * 1024)
                    .and_then(|bytes| usize::try_from(bytes).ok())
                    .filter(|bytes| *bytes > 0)
                    .ok_or_else(|| anyhow!("invalid memory limit for {}", plugin.manifest.name))?;
                b = b.memory_size(bytes);
                b.build()
            },
            _instance_permit: permit,
            request_id: 0,
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_fuel(
        plugin
            .manifest
            .permissions
            .resources
            .max_fuel
            .unwrap_or(1_000_000_000),
    )?;
    Ok(store)
}

fn validate_component_imports(
    engine: &Engine,
    component: &Component,
    manifest: &Manifest,
) -> Result<()> {
    let declared_wasi: HashSet<&str> = manifest.imports.wasi.iter().map(String::as_str).collect();
    let declared_host: HashSet<&str> = manifest.imports.host.iter().map(String::as_str).collect();
    let mut actual_wasi = HashSet::new();
    let mut actual_host = HashSet::new();
    let component_type = component.component_type();

    for (import_name, import) in component_type.imports(engine) {
        let interface = import.implements.unwrap_or(import_name);
        let (actual, declared, kind) = if interface.starts_with("wasi:") {
            (&mut actual_wasi, &declared_wasi, "WASI")
        } else {
            (&mut actual_host, &declared_host, "host")
        };
        if !declared
            .iter()
            .any(|name| *name == interface || import.is_implements(name))
        {
            anyhow::bail!(
                "plugin {} imports undeclared {kind} interface {interface}",
                manifest.name
            );
        }
        actual.insert(interface);
    }

    for name in &declared_wasi {
        if !actual_wasi.contains(name) {
            anyhow::bail!(
                "manifest declares unused WASI interface {name} for {}",
                manifest.name
            );
        }
    }
    for name in &declared_host {
        if !actual_host.contains(name) {
            anyhow::bail!(
                "manifest declares unused host interface {name} for {}",
                manifest.name
            );
        }
    }
    let imports_kv = actual_host
        .iter()
        .any(|interface| interface.starts_with("poc:host/host-kv"));
    match (&manifest.permissions.kv, imports_kv) {
        (Some(permission), true) => {
            if !matches!(permission.access.as_str(), "read" | "read-write") {
                anyhow::bail!(
                    "unsupported KV access {:?} for {}",
                    permission.access,
                    manifest.name
                );
            }
            if permission.namespace.is_empty()
                || !permission
                    .namespace
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                anyhow::bail!("invalid KV namespace for {}", manifest.name);
            }
        }
        (Some(_), false) => {
            anyhow::bail!("manifest grants unused KV permission to {}", manifest.name);
        }
        (None, true) => {
            anyhow::bail!(
                "plugin {} imports host-kv without KV permission",
                manifest.name
            );
        }
        (None, false) => {}
    }
    let imports_filesystem = actual_wasi
        .iter()
        .any(|interface| interface.starts_with("wasi:filesystem/"));
    if !manifest.permissions.filesystem.is_empty() && !imports_filesystem {
        anyhow::bail!(
            "manifest grants unused filesystem permission to {}",
            manifest.name
        );
    }
    for permission in &manifest.permissions.filesystem {
        if !matches!(permission.access.as_str(), "read" | "read-write") {
            anyhow::bail!(
                "unsupported filesystem access {:?} for {}",
                permission.access,
                manifest.name
            );
        }
    }
    let imports_sockets = actual_wasi
        .iter()
        .any(|interface| interface.starts_with("wasi:sockets/"));
    if (!manifest.permissions.network.outbound.is_empty()
        || !manifest.permissions.network.inbound.is_empty())
        && !imports_sockets
    {
        anyhow::bail!(
            "manifest grants unused network permission to {}",
            manifest.name
        );
    }
    let imports_http_outbound = actual_wasi
        .iter()
        .any(|interface| interface.starts_with("wasi:http/outgoing-handler"));
    if !manifest.permissions.http.outbound.is_empty() && !imports_http_outbound {
        anyhow::bail!(
            "manifest grants unused HTTP outbound permission to {}",
            manifest.name
        );
    }
    let exports_http_inbound = manifest
        .exports
        .iter()
        .any(|export| export.name.starts_with("wasi:http/incoming-handler"));
    if manifest.permissions.http.inbound != exports_http_inbound {
        anyhow::bail!(
            "HTTP inbound permission/export mismatch for {}",
            manifest.name
        );
    }
    if manifest.permissions.http.inbound
        && !matches!(manifest.kind.as_str(), "executable" | "hybrid")
    {
        anyhow::bail!(
            "only executable/hybrid plugins may request HTTP inbound: {}",
            manifest.name
        );
    }
    Ok(())
}

fn signed_manifest_payload(manifest: &serde_json::Value, component: &[u8]) -> Result<Vec<u8>> {
    let mut value = manifest.clone();
    if let Some(metadata) = value
        .get_mut("metadata")
        .and_then(serde_json::Value::as_object_mut)
    {
        metadata.remove("signature");
    }
    let manifest_json = serde_json::to_vec(&value)?;
    let mut payload = b"owasidea-plugin-signature-v1\0".to_vec();
    payload.extend_from_slice(&manifest_json);
    payload.push(0);
    payload.extend_from_slice(component);
    Ok(payload)
}

fn verify_manifest_signature(manifest: &serde_json::Value, artifact: &[u8]) -> Result<()> {
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("plugin");
    let signature = manifest
        .get("metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|m| m.get("signature"))
        .and_then(serde_json::Value::as_str);
    let require_signatures = std::env::var("OWASIDEA_REQUIRE_SIGNATURES")
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(!cfg!(debug_assertions));
    if require_signatures && signature.is_none() {
        anyhow::bail!("plugin {} is unsigned but signatures are required", name);
    }
    let Some(signature) = signature else {
        return Ok(());
    };
    let public_key_path = match std::env::var("OWASIDEA_PUBLIC_KEY") {
        Ok(path) => path,
        Err(_) if require_signatures => {
            anyhow::bail!("OWASIDEA_PUBLIC_KEY is required to verify signed plugins")
        }
        Err(_) => {
            tracing::warn!(plugin = %name, "plugin signature present but not verified; configure OWASIDEA_PUBLIC_KEY");
            return Ok(());
        }
    };
    if manifest
        .get("metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|m| m.get("signature_format"))
        .and_then(serde_json::Value::as_str)
        != Some("owasidea-manifest-v1")
    {
        anyhow::bail!("unsupported or legacy signature format for {}", name);
    }
    use base64::Engine;
    let public_key = std::fs::read_to_string(public_key_path).context("read public key")?;
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(public_key.trim())
        .context("decode public key")?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature)
        .context("decode signature")?;
    verify_signature_with_key(manifest, artifact, &public_key, &signature, name)
}

fn instance_slots(manifest: &Manifest) -> Result<Arc<tokio::sync::Semaphore>> {
    let limit = manifest.permissions.resources.max_instances.unwrap_or(1);
    if limit == 0 {
        anyhow::bail!("max_instances must be at least 1 for {}", manifest.name);
    }
    Ok(Arc::new(tokio::sync::Semaphore::new(limit as usize)))
}

fn verify_signature_with_key(
    manifest: &serde_json::Value,
    artifact: &[u8],
    public_key: &[u8],
    signature: &[u8],
    name: &str,
) -> Result<()> {
    use ring::signature::{UnparsedPublicKey, ED25519};
    let payload = signed_manifest_payload(manifest, artifact)?;
    UnparsedPublicKey::new(&ED25519, &public_key)
        .verify(&payload, signature)
        .map_err(|_| anyhow!("signature verification failed for {}", name))
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GreetReq {
    name: String,
}

#[derive(Deserialize)]
struct CompleteReq {
    prompt: String,
}

#[derive(Deserialize)]
struct CallReq {
    function: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

#[derive(Serialize, Default)]
struct ServiceStatusRes {
    running: bool,
    address: Option<String>,
    /// Reason the last start attempt failed, if any. Cleared by a successful start.
    last_error: Option<String>,
    /// Explicit restarts that replaced a live instance.
    restarts: u64,
}

impl ServiceStatusRes {
    fn new(running: bool, address: Option<String>, health: &ServiceHealth) -> Self {
        Self {
            running,
            address,
            last_error: health.last_error.clone(),
            restarts: health.restarts,
        }
    }
}

#[derive(Deserialize)]
struct ServiceStartReq {
    #[serde(default)]
    region_json: String,
}

#[derive(Deserialize)]
struct BindReq {
    port: u16,
}

#[derive(Deserialize)]
struct ParseReq {
    addr: String,
}

#[derive(Deserialize)]
struct DialReq {
    host: String,
    port: u16,
}

#[derive(Deserialize)]
struct StartReq {
    region_json: String,
}

#[derive(Deserialize)]
struct SendTextReq {
    addr: String,
    text: String,
}

#[derive(Serialize)]
struct DrainRes {
    messages: Vec<String>,
    events: Vec<String>,
}

#[derive(Deserialize)]
struct TlsReq {
    host: String,
    port: u16,
    server_name: String,
}

#[derive(Deserialize)]
struct P3Req {
    host: String,
    port: u16,
}

#[derive(Serialize)]
struct OutputRes {
    output: String,
}

#[derive(Serialize, Deserialize)]
struct LogsQuery {
    level: Option<String>,
}

#[derive(Serialize)]
struct LogsRes {
    logs: Vec<String>,
    entries: Vec<LogEntry>,
}

#[derive(Serialize)]
struct PluginInfo {
    name: String,
    version: String,
    language: String,
    runtime: String,
    #[serde(rename = "type")]
    kind: String,
    exports: Vec<String>,
    imports_wasi: usize,
    imports_host: Vec<String>,
    served_on: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct UiPluginInfo {
    name: String,
    version: String,
    language: String,
    entry: String,
    sha256: String,
    files: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    bridge: Vec<String>,
    /// Bridge timeout declared by the manifest (ms). Absent means the parent
    /// uses the 10 s default from ADR-0004; the parent caps it at 30 s.
    #[serde(rename = "timeoutMs", skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
}

fn ui_bridge_timeout_ms(ui: &serde_json::Value, manifest: &serde_json::Value) -> Option<u64> {
    ui.get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            manifest
                .pointer("/permissions/resources/timeout_ms")
                .and_then(serde_json::Value::as_u64)
        })
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
struct OverviewRes {
    host: &'static str,
    plugins: usize,
    libraries: usize,
    executables: usize,
    services: usize,
    runtimes: Vec<String>,
    build_id: Option<String>,
}

fn record_plugin_metric(state: &AppState, name: &str, success: bool, elapsed_ms: u64, fuel: u64) {
    plugin_metrics::record(&state.plugin_metrics, name, success, elapsed_ms, fuel);
}

async fn metrics(State(state): State<AppState>) -> String {
    plugin_metrics::render(
        state.requests.load(std::sync::atomic::Ordering::Relaxed),
        state.errors.load(std::sync::atomic::Ordering::Relaxed),
        &state.plugin_metrics,
    )
}

async fn dev_status(State(state): State<AppState>) -> Json<DevStatusRes> {
    Json(DevStatusRes {
        active: state.dev_mode,
        hot_reload: state.dev_hot_reload,
        watch_paths: state.dev_watch_paths.clone(),
        last_reload: state
            .dev_last_reload
            .read()
            .map(|records| records.clone())
            .unwrap_or_default(),
    })
}

async fn overview(State(state): State<AppState>) -> Json<OverviewRes> {
    let mut libraries = 0;
    let mut executables = 0;
    let mut services = 0;
    let mut runtimes = std::collections::BTreeSet::new();
    for p in state.plugins.read().unwrap().values() {
        match p.manifest.kind.as_str() {
            "executable" => executables += 1,
            "service" | "hybrid" => services += 1,
            _ => libraries += 1,
        }
        runtimes.insert(
            p.manifest
                .runtime
                .clone()
                .unwrap_or_else(|| "wasi-component".to_string()),
        );
    }
    Json(OverviewRes {
        host: "ok",
        plugins: state.plugins.read().unwrap().len(),
        libraries,
        executables,
        services,
        runtimes: runtimes.into_iter().collect(),
        build_id: std::env::var("OWASIDEA_BUILD_ID")
            .ok()
            .filter(|s| !s.trim().is_empty()),
    })
}

async fn list_plugins(State(state): State<AppState>) -> Json<Vec<PluginInfo>> {
    let mut out = Vec::new();
    for (name, p) in state.plugins.read().unwrap().iter() {
        out.push(PluginInfo {
            name: name.clone(),
            version: p.manifest.version.clone(),
            language: p.manifest.language.clone(),
            runtime: p
                .manifest
                .runtime
                .clone()
                .unwrap_or_else(|| "wasi-component".to_string()),
            kind: p.manifest.kind.clone(),
            exports: p.manifest.exports.iter().map(|e| e.name.clone()).collect(),
            imports_wasi: p.manifest.imports.wasi.len(),
            imports_host: p.manifest.imports.host.clone(),
            served_on: matches!(p.manifest.kind.as_str(), "executable" | "hybrid")
                .then(|| format!("/app/{name}")),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Json(out)
}

async fn list_ui_plugins(State(state): State<AppState>) -> Json<Vec<UiPluginInfo>> {
    Json(state.ui_plugins.as_ref().clone())
}

async fn plugin_manifest(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Manifest>, (StatusCode, String)> {
    state
        .plugin(&name)
        .map(|p| Json(p.manifest.clone()))
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))
}

async fn permissions_diff(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = state.manifests_dir.join("permissions.diff.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|_| (StatusCode::NOT_FOUND, "no permission diff".to_string()))?;
    let v: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(v))
}

async fn plugin_logs(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<LogsRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let guard = p.logs.lock().await;
    let mut entries: Vec<LogEntry> = guard.entries.iter().map(|(_, e)| e.clone()).collect();
    if let Some(lv) = q.level.as_deref() {
        let thr = LogBuffer::level_rank(lv);
        entries.retain(|e| LogBuffer::level_rank(&e.level) >= thr);
    }
    let logs = entries.iter().map(|e| e.message.clone()).collect();
    Ok(Json(LogsRes { logs, entries }))
}

async fn plugin_logs_stream(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Query(q): Query<LogsQuery>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, String),
> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let logs = p.logs.clone();
    let level_filter = q.level.clone();
    let stream = async_stream::stream! {
        let mut last = 0u64;
        loop {
            let snapshot: Vec<_> = {
                let l = logs.lock().await;
                l.entries
                    .iter()
                    .filter(|(id, _)| *id > last)
                    .cloned()
                    .collect()
            };
            if !snapshot.is_empty() {
                for (id, entry) in snapshot {
                    if let Some(lv) = level_filter.as_deref() {
                        let thr = LogBuffer::level_rank(lv);
                        if LogBuffer::level_rank(&entry.level) < thr {
                            last = id;
                            continue;
                        }
                    }
                    let data = serde_json::to_string(&entry).unwrap_or_default();
                    yield Ok(Event::default().event("log").id(id.to_string()).data(data));
                    last = id;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn reload(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if state.services.lock().await.contains_key(&name) {
        return Err((
            StatusCode::CONFLICT,
            format!("stop service {name} before reloading it"),
        ));
    }
    if name == "tailcat-wasi" && state.tailcat_session.lock().await.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "stop tailcat-wasi before reloading it".to_string(),
        ));
    }
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    // Re-read the manifest from disk so capability/permission edits are
    // enforced on reload (not just the component bytes).
    let raw = std::fs::read_to_string(&p.manifest_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read manifest: {e}"),
        )
    })?;
    let manifest_value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid manifest: {e}")))?;
    let manifest: Manifest = serde_json::from_value(manifest_value.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid manifest: {e}")))?;
    if let Err(e) = validate_manifest_abi(&manifest) {
        return Err((StatusCode::BAD_REQUEST, format!("invalid manifest: {e}")));
    }
    if manifest.name != name {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("manifest identity changed from {name} to {}", manifest.name),
        ));
    }
    let base = p
        .manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let component_path = base.join(&manifest.component);
    let namespace = manifest
        .permissions
        .kv
        .as_ref()
        .map(|permission| permission.namespace.as_str())
        .unwrap_or(&manifest.name);
    let previous_namespace = p
        .manifest
        .permissions
        .kv
        .as_ref()
        .map(|permission| permission.namespace.as_str())
        .unwrap_or(&p.manifest.name);
    let kv = if namespace == previous_namespace {
        p.kv.clone()
    } else {
        Arc::new(std::sync::Mutex::new(
            open_kv_db(namespace).map_err(internal)?,
        ))
    };
    let component =
        verify_and_compile_component(&state.engine, &manifest, &manifest_value, &component_path)
            .map_err(internal)?;
    let mut runtime = PluginRuntime {
        manifest,
        manifest_path: p.manifest_path.clone(),
        component,
        kv,
        logs: p.logs.clone(),
        instance_slots: p.instance_slots.clone(),
        http_pre: None,
    };
    runtime.http_pre = prepare_http_plugin(&state.engine, &runtime).map_err(internal)?;
    let timeout_ms = runtime
        .manifest
        .permissions
        .resources
        .timeout_ms
        .unwrap_or(30000);
    match tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        instantiate(&state.engine, &runtime),
    )
    .await
    {
        Ok(Ok(_)) => {
            if let Ok(mut m) = state.plugins.write() {
                m.insert(name.clone(), std::sync::Arc::new(runtime));
            }
            Ok(Json(serde_json::json!({ "reloaded": name })))
        }
        Ok(Err(e)) => Err(internal(e)),
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "reload timed out".to_string())),
    }
}

async fn greet(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<GreetReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let rid = state
        .requests
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    let span = otel::call_span(&name, "greet", "library");
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        store.data_mut().request_id = rid;
        let func = instance
            .get_typed_func::<(String,), (String,)>(&mut store, "greet")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.name,))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(Json(OutputRes { output }))
    }
    .instrument(span.clone());
    let result =
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
            Ok(r) => r,
            Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
        };
    otel::record_outcome(&span, result.is_ok());
    result
}

async fn complete(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<CompleteReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let prompt = req.prompt.clone();
    let rid = state
        .requests
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    let span = otel::call_span(&name, "complete", "library");
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        store.data_mut().request_id = rid;
        let func = instance
            .get_typed_func::<(String,), (Result<String, String>,)>(&mut store, "complete")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (prompt,))
            .await
            .map_err(internal)?;
        match output {
            Ok(s) => Ok::<_, (StatusCode, String)>(Json(OutputRes { output: s })),
            Err(e) => Err((StatusCode::BAD_REQUEST, e)),
        }
    }
    .instrument(span.clone());
    let result =
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
            Ok(r) => r,
            Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
        };
    otel::record_outcome(&span, result.is_ok());
    result
}

/// Invoke one export with positional JSON args and return the JSON response
/// body plus fuel consumed. Shared by `/call` and experimental jobs so both
/// paths use identical WIT type validation and fuel accounting.
async fn invoke_export(
    engine: &Engine,
    plugin: &PluginRuntime,
    function: &str,
    args: &[serde_json::Value],
    request_id: u64,
) -> Result<(serde_json::Value, u64), (StatusCode, String)> {
    let initial_fuel = plugin
        .manifest
        .permissions
        .resources
        .max_fuel
        .unwrap_or(1_000_000_000);
    let (mut store, instance) = instantiate(engine, plugin).await.map_err(internal)?;
    store.data_mut().request_id = request_id;
    // Resolve function: top-level first, then inside exported interfaces.
    let func = if let Some(f) = instance.get_func(&mut store, function) {
        f
    } else {
        let mut resolved = None;
        for exp in &plugin.manifest.exports {
            if let Some(idx) = instance.get_export_index(&mut store, None, &exp.name) {
                // ensure it's an instance export
                if let Some(func_idx) = instance.get_export_index(&mut store, Some(&idx), function)
                {
                    if let Some(f) = instance.get_func(&mut store, func_idx) {
                        resolved = Some(f);
                        break;
                    }
                }
            }
        }
        resolved.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("export {function} not found"),
            )
        })?
    };
    let ty = func.ty(&store);
    if ty.params().count() != args.len() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("expected {} args, got {}", ty.params().count(), args.len()),
        ));
    }
    let mut params = Vec::new();
    for (param, arg) in ty.params().zip(args.iter()) {
        params.push(json_to_val(&param.1, arg).map_err(|e| (StatusCode::BAD_REQUEST, e))?);
    }
    let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
    func.call_async(&mut store, &params, &mut results)
        .await
        .map_err(internal)?;
    let fuel_used = store
        .get_fuel()
        .map(|remaining| initial_fuel.saturating_sub(remaining))
        .unwrap_or(0);
    let json_results: Vec<serde_json::Value> = results.iter().map(val_to_json).collect();
    let body = if json_results.len() == 1 {
        serde_json::json!({ "result": json_results[0] })
    } else {
        serde_json::json!({ "results": json_results })
    };
    Ok((body, fuel_used))
}

async fn call_export(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<CallReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p
        .manifest
        .permissions
        .resources
        .timeout_ms
        .unwrap_or(DEFAULT_CALL_TIMEOUT_MS);
    let rid = state
        .requests
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    let mut fuel_used = 0u64;
    let start = std::time::Instant::now();
    let span = otel::call_span(&name, &req.function, "library");
    let work = async {
        let (body, used) =
            invoke_export(&state.engine, p.as_ref(), &req.function, &req.args, rid).await?;
        fuel_used = used;
        Ok::<_, (StatusCode, String)>(Json(body))
    }
    .instrument(span.clone());
    let result =
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
            Ok(r) => r,
            Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
        };
    otel::record_outcome(&span, result.is_ok());
    record_plugin_metric(
        &state,
        &name,
        result.is_ok(),
        start.elapsed().as_millis() as u64,
        fuel_used,
    );
    result
}

/// Accept a start output that is a bare `host:port` as the service address.
/// Anything else (for example "service started") is not an address.
fn service_address(output: &str) -> Option<String> {
    let candidate = output.trim();
    if candidate.is_empty() {
        return None;
    }
    if candidate.parse::<SocketAddr>().is_ok() {
        return Some(candidate.to_string());
    }
    let (host, port) = candidate.rsplit_once(':')?;
    if host.is_empty() || host.chars().any(char::is_whitespace) || port.parse::<u16>().is_err() {
        return None;
    }
    Some(candidate.to_string())
}

async fn service_start_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<ServiceStartReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    if !matches!(p.manifest.kind.as_str(), "service" | "hybrid") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("plugin {name} is not a stateful service"),
        ));
    }
    let region_json = if req.region_json.trim().is_empty() {
        default_region_json()
    } else {
        req.region_json
    };
    let span = otel::call_span(&name, "start-service", "service");
    let result = async {
        match start_service_session(&state, &name, p, region_json).await? {
            StartOutcome::Started(output) => Ok(Json(OutputRes { output })),
            StartOutcome::AlreadyRunning => Ok(Json(OutputRes {
                output: "already running".to_string(),
            })),
        }
    }
    .instrument(span.clone())
    .await;
    otel::record_outcome(&span, result.is_ok());
    result
}

/// Stop by dropping the instance, then start a fresh one (PX-08).
async fn service_restart_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    body: Option<Json<ServiceStartReq>>,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    if !matches!(p.manifest.kind.as_str(), "service" | "hybrid") {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "plugin {name} is not a stateful service; restart requires type service or hybrid"
            ),
        ));
    }
    let region_json = body
        .map(|Json(req)| req.region_json)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(default_region_json);

    // Drop the live instance exactly like `/service/stop`; the next start gets
    // fresh guest state and sockets.
    let span = otel::call_span(&name, "restart-service", "service");
    let dropped = state.services.lock().await.remove(&name).is_some();
    let result = async {
        match start_service_session(&state, &name, p, region_json).await? {
            StartOutcome::Started(_) => {}
            StartOutcome::AlreadyRunning => {
                return Err((
                    StatusCode::CONFLICT,
                    format!("plugin {name} was started concurrently; restart aborted"),
                ));
            }
        }
        {
            let mut health = state.service_health.lock().await;
            let entry = health.entry(name.clone()).or_default();
            entry.last_error = None;
            if dropped {
                entry.restarts += 1;
            }
        }
        tracing::info!(plugin = %name, replaced = dropped, "service restarted");
        service_status_impl(&state, &name).await
    }
    .instrument(span.clone())
    .await;
    otel::record_outcome(&span, result.is_ok());
    result
}

/// Instantiate and start one service instance. Records `last_error` for
/// `/service/status` and one PX-07 metric per attempt.
async fn start_service_session(
    state: &AppState,
    name: &str,
    p: Arc<PluginRuntime>,
    region_json: String,
) -> Result<StartOutcome, (StatusCode, String)> {
    let start = std::time::Instant::now();
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let rid = state
        .requests
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    let initial_fuel = p
        .manifest
        .permissions
        .resources
        .max_fuel
        .unwrap_or(1_000_000_000);
    let mut fuel_used = 0u64;
    let result: Result<StartOutcome, (StatusCode, String)> = async {
        let mut sessions = state.services.lock().await;
        if sessions.contains_key(name) {
            return Ok(StartOutcome::AlreadyRunning);
        }
        let work = async {
            let (mut store, instance) = instantiate(&state.engine, p.as_ref())
                .await
                .map_err(internal)?;
            store.data_mut().request_id = rid;
            for candidate in ["start-service", "start-server"] {
                if let Some(func) = instance.get_func(&mut store, candidate) {
                    let ty = func.ty(&store);
                    let mut params = Vec::new();
                    let parameter_types: Vec<_> = ty.params().collect();
                    if parameter_types.len() > 1 {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            format!("{candidate} may accept zero or one string argument"),
                        ));
                    }
                    if let Some((_, param_type)) = parameter_types.first() {
                        params.push(
                            json_to_val(
                                param_type,
                                &serde_json::Value::String(region_json.clone()),
                            )
                            .map_err(|error| (StatusCode::BAD_REQUEST, error))?,
                        );
                    }
                    let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
                    func.call_async(&mut store, &params, &mut results)
                        .await
                        .map_err(internal)?;
                    fuel_used = store
                        .get_fuel()
                        .map(|remaining| initial_fuel.saturating_sub(remaining))
                        .unwrap_or(0);
                    if let Some(Val::Result(Err(error))) = results.first() {
                        let error = error
                            .as_deref()
                            .map(val_to_json)
                            .unwrap_or(serde_json::Value::Null);
                        return Err((StatusCode::BAD_REQUEST, error.to_string()));
                    }
                    let output_value = match results.first() {
                        Some(Val::Result(Ok(Some(value)))) => Some(val_to_json(value)),
                        Some(value) => Some(val_to_json(value)),
                        None => None,
                    };
                    let out = output_value
                        .and_then(|value| value.as_str().map(str::to_string))
                        .unwrap_or_else(|| "started".to_string());
                    return Ok::<_, (StatusCode, String)>((store, instance, out));
                }
            }
            Err((StatusCode::NOT_FOUND, "no start-service export".to_string()))
        };
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
            Ok(Ok((store, instance, output))) => {
                sessions.insert(
                    name.to_string(),
                    ServiceSession {
                        store,
                        instance,
                        timeout_ms,
                        address: service_address(&output),
                    },
                );
                Ok(StartOutcome::Started(output))
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
        }
    }
    .await;
    {
        // Never hold the sessions lock while taking the health lock.
        let mut health = state.service_health.lock().await;
        let entry = health.entry(name.to_string()).or_default();
        match &result {
            Ok(_) => entry.last_error = None,
            Err((_, message)) => entry.last_error = Some(message.clone()),
        }
    }
    record_plugin_metric(
        state,
        name,
        result.is_ok(),
        start.elapsed().as_millis() as u64,
        fuel_used,
    );
    result
}

async fn service_stop_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let start = std::time::Instant::now();
    let span = otel::call_span(&name, "stop-service", "service");
    let result = async {
        if state.plugin(&name).is_none() {
            return Err((StatusCode::NOT_FOUND, format!("unknown plugin {name}")));
        }
        let stopped = state.services.lock().await.remove(&name).is_some();
        Ok::<_, (StatusCode, String)>(Json(serde_json::json!({"stopped": stopped})))
    }
    .instrument(span.clone())
    .await;
    otel::record_outcome(&span, result.is_ok());
    record_plugin_metric(
        &state,
        &name,
        result.is_ok(),
        start.elapsed().as_millis() as u64,
        0,
    );
    result
}

async fn service_status_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    let start = std::time::Instant::now();
    let result = service_status_impl(&state, &name).await;
    record_plugin_metric(
        &state,
        &name,
        result.is_ok(),
        start.elapsed().as_millis() as u64,
        0,
    );
    result
}

async fn service_status_impl(
    state: &AppState,
    name: &str,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    if state.plugin(name).is_none() {
        return Err((StatusCode::NOT_FOUND, format!("unknown plugin {name}")));
    }
    // Copy the lifecycle record before locking sessions (lock order:
    // service_health then services is never held simultaneously).
    let health = {
        let health = state.service_health.lock().await;
        health.get(name).cloned().unwrap_or_default()
    };
    let mut sessions = state.services.lock().await;
    let Some(session) = sessions.get_mut(name) else {
        return Ok(Json(ServiceStatusRes::new(false, None, &health)));
    };
    for candidate in ["service-status", "server-status"] {
        if let Some(func) = session.instance.get_func(&mut session.store, candidate) {
            let ty = func.ty(&session.store);
            let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
            match tokio::time::timeout(
                std::time::Duration::from_millis(session.timeout_ms),
                func.call_async(&mut session.store, &[], &mut results),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(internal(error)),
                Err(_) => {
                    return Err((
                        StatusCode::GATEWAY_TIMEOUT,
                        "service status timed out".to_string(),
                    ));
                }
            }
            let running = results
                .first()
                .and_then(|v| match v {
                    Val::Bool(b) => Some(*b),
                    _ => None,
                })
                .unwrap_or(true);
            return Ok(Json(ServiceStatusRes::new(
                running,
                session.address.clone(),
                &health,
            )));
        }
    }
    Ok(Json(ServiceStatusRes::new(
        true,
        session.address.clone(),
        &health,
    )))
}

/// Default timeout for the typed POC routes (`dial-tcp`, `tls-probe`, …) when
/// the manifest does not set `permissions.resources.timeout_ms`.
pub(crate) const DEFAULT_POC_TIMEOUT_MS: u64 = 10_000;

fn poc_timeout_ms(plugin: &PluginRuntime) -> u64 {
    plugin
        .manifest
        .permissions
        .resources
        .timeout_ms
        .unwrap_or(DEFAULT_POC_TIMEOUT_MS)
        .max(1)
}

/// Run one typed POC route call under the manifest timeout (default 10 s) and
/// answer `504` instead of hanging. Dropping the future on timeout drops the
/// `Store`, which returns its `max_instances` slot to the plugin semaphore.
async fn poc_call<F, T>(
    plugin: &PluginRuntime,
    operation: &str,
    work: F,
) -> Result<T, (StatusCode, String)>
where
    F: std::future::Future<Output = Result<T, (StatusCode, String)>>,
{
    let timeout_ms = poc_timeout_ms(plugin);
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(result) => result,
        Err(_) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            format!("{operation} timed out after {timeout_ms} ms"),
        )),
    }
}

async fn echo_udp(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<BindReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(u16,), (Result<String, String>,)>(&mut store, "echo-udp")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.port,))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "echo-udp", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn dial_echo(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<P3Req>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(String, u16), (Result<String, String>,)>(&mut store, "dial-echo")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.host, req.port))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "dial-echo", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn tls_probe(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<TlsReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(String, u16, String), (Result<String, String>,)>(
                &mut store,
                "tls-probe",
            )
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.host, req.port, req.server_name))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "tls-probe", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

fn ensure_ticker(state: &AppState, session: &mut TailcatSession) {
    if session
        .ticker
        .as_ref()
        .is_some_and(|handle| !handle.is_finished())
    {
        return;
    }
    let state2 = state.clone();
    session.ticker = Some(tokio::spawn(async move {
        // Host-driven scheduling: the Go runtime needs a chance to run
        // network goroutines between component calls. Keep ticking for the
        // lifetime of the session; start/stop only gate the server.
        loop {
            let mut guard = state2.tailcat_session.lock().await;
            let Some(session) = guard.as_mut() else {
                break;
            };
            let tick = session
                .instance
                .get_typed_func::<(), ()>(&mut session.store, "tick");
            let result = match tick {
                Ok(func) => func.call_async(&mut session.store, ()).await,
                Err(e) => {
                    tracing::warn!("tailcat tick lookup failed: {e}");
                    break;
                }
            };
            drop(guard);
            if let Err(e) = result {
                tracing::warn!("tailcat tick failed: {e}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }));
}

async fn start_service(
    State(state): State<AppState>,
    Json(req): Json<StartReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let region_json = if req.region_json.trim().is_empty() {
        default_region_json()
    } else {
        req.region_json
    };

    let mut guard = state.tailcat_session.lock().await;
    if guard.is_none() {
        let p = state.plugin("tailcat-wasi").ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "tailcat-wasi plugin not loaded".to_string(),
            )
        })?;
        let (store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        *guard = Some(TailcatSession {
            store,
            instance,
            address: None,
            running: Arc::new(AtomicBool::new(false)),
            ticker: None,
        });
    }
    let session = guard.as_mut().unwrap();
    if let Some(addr) = &session.address {
        return Ok(Json(OutputRes {
            output: addr.clone(),
        }));
    }

    let func = session
        .instance
        .get_typed_func::<(String,), (Result<String, String>,)>(&mut session.store, "start-server")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut session.store, (region_json,))
        .await
        .map_err(internal)?;
    let addr = match output {
        Ok(s) => s,
        Err(e) => return Err((StatusCode::BAD_REQUEST, e)),
    };
    session.address = Some(addr.clone());
    session.running.store(true, Ordering::SeqCst);

    ensure_ticker(&state, session);

    Ok(Json(OutputRes { output: addr }))
}

async fn stop_service(
    State(state): State<AppState>,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    let mut guard = state.tailcat_session.lock().await;
    // Stop by dropping the whole component instance. This closes its
    // sockets and network goroutines without waiting for tailcat's blocking
    // Close path inside the guest, which can outlive an HTTP request.
    if let Some(session) = guard.take() {
        session.running.store(false, Ordering::SeqCst);
    }
    Ok(Json(ServiceStatusRes {
        running: false,
        address: None,
        ..Default::default()
    }))
}

async fn service_status(
    State(state): State<AppState>,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    let mut guard = state.tailcat_session.lock().await;
    let Some(session) = guard.as_mut() else {
        return Ok(Json(ServiceStatusRes::default()));
    };
    ensure_ticker(&state, session);
    let status = session
        .instance
        .get_typed_func::<(), (bool,)>(&mut session.store, "server-status")
        .map_err(internal)?;
    let (running,) = status
        .call_async(&mut session.store, ())
        .await
        .map_err(internal)?;
    Ok(Json(ServiceStatusRes {
        running,
        address: session.address.clone(),
        ..Default::default()
    }))
}

async fn send_text(
    State(state): State<AppState>,
    Json(req): Json<SendTextReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let mut guard = state.tailcat_session.lock().await;
    let Some(session) = guard.as_mut() else {
        return Err((
            StatusCode::CONFLICT,
            "tailcat service is not started".to_string(),
        ));
    };
    ensure_ticker(&state, session);
    let func = session
        .instance
        .get_typed_func::<(String, String), (Result<(), String>,)>(&mut session.store, "start-send")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut session.store, (req.addr, req.text))
        .await
        .map_err(internal)?;
    match output {
        Ok(()) => Ok(Json(OutputRes {
            output: "queued".to_string(),
        })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn drain_tailcat(
    State(state): State<AppState>,
) -> Result<Json<DrainRes>, (StatusCode, String)> {
    let mut guard = state.tailcat_session.lock().await;
    let Some(session) = guard.as_mut() else {
        return Ok(Json(DrainRes {
            messages: Vec::new(),
            events: Vec::new(),
        }));
    };
    let inbox_func = session
        .instance
        .get_typed_func::<(), (Vec<String>,)>(&mut session.store, "drain-inbox")
        .map_err(internal)?;
    let (messages,) = inbox_func
        .call_async(&mut session.store, ())
        .await
        .map_err(internal)?;
    let events_func = session
        .instance
        .get_typed_func::<(), (Vec<String>,)>(&mut session.store, "drain-events")
        .map_err(internal)?;
    let (events,) = events_func
        .call_async(&mut session.store, ())
        .await
        .map_err(internal)?;
    Ok(Json(DrainRes { messages, events }))
}

fn network_hints() -> (Option<String>, Option<String>) {
    let gateway = std::env::var("TAILCAT_WASI_GATEWAY")
        .ok()
        .or_else(linux_default_gateway);
    let self_ip = std::env::var("TAILCAT_WASI_SELF_IP")
        .ok()
        .or_else(local_ipv4);
    (gateway, self_ip)
}

#[cfg(target_os = "linux")]
fn linux_default_gateway() -> Option<String> {
    let route = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in route.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 || fields[1] != "00000000" {
            continue;
        }
        let flags = u32::from_str_radix(fields[3], 16).ok()?;
        if flags & 0x2 == 0 {
            continue;
        }
        let raw = u32::from_str_radix(fields[2], 16).ok()?;
        return Some(Ipv4Addr::from(raw.to_le_bytes()).to_string());
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn linux_default_gateway() -> Option<String> {
    None
}

fn local_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connecting a UDP socket does not send traffic; it only selects the
    // interface route and lets us read the local address.
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) => Some(ip.to_string()),
        std::net::IpAddr::V6(_) => None,
    }
}

fn default_region_json() -> String {
    r#"{"RegionID":301,"RegionCode":"nyc","RegionName":"New York City","Nodes":[{"Name":"301a","RegionID":301,"HostName":"tc301a.ipn.dev","IPv4":"199.38.181.166","IPv6":"2607:f740:f::26b","CanPort80":true}]}"#.to_string()
}

async fn resolve_network_hosts(hosts: &[String]) -> Result<HashSet<IpAddr>> {
    let mut addresses = HashSet::new();
    for host in hosts {
        if host == "*" || host.contains('/') || host.contains(':') {
            if let Ok(ip) = host.parse::<IpAddr>() {
                addresses.insert(ip);
                continue;
            }
            anyhow::bail!("network permission must be an exact hostname or IP address: {host}");
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            addresses.insert(ip);
            continue;
        }
        let resolved = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::net::lookup_host((host.as_str(), 0)),
        )
        .await
        .with_context(|| format!("resolve network permission host {host} timed out"))?
        .with_context(|| format!("resolve network permission host {host}"))?;
        let resolved: HashSet<IpAddr> = resolved.map(|addr| addr.ip()).collect();
        if resolved.is_empty() {
            anyhow::bail!("network permission host {host} resolved to no addresses");
        }
        addresses.extend(resolved);
    }
    Ok(addresses)
}

fn parse_inbound_addresses(hosts: &[String]) -> Result<HashSet<IpAddr>> {
    hosts
        .iter()
        .map(|host| {
            host.parse::<IpAddr>().with_context(|| {
                format!("inbound network permission must be an IP address: {host}")
            })
        })
        .collect()
}

fn socket_address_allowed(
    addr: SocketAddr,
    usage: SocketAddrUse,
    outbound: &HashSet<IpAddr>,
    inbound: &HashSet<IpAddr>,
) -> bool {
    let ip = addr.ip();
    let local_bind_allowed = inbound.contains(&ip)
        || (ip.is_unspecified()
            && (inbound.contains(&IpAddr::V4(Ipv4Addr::UNSPECIFIED))
                || inbound.contains(&IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED))));
    match usage {
        SocketAddrUse::TcpBind | SocketAddrUse::TcpListen | SocketAddrUse::UdpBind => {
            local_bind_allowed
        }
        SocketAddrUse::TcpAccept => !inbound.is_empty(),
        SocketAddrUse::TcpConnect | SocketAddrUse::UdpSend => outbound.contains(&ip),
        SocketAddrUse::UdpReceive => outbound.contains(&ip) || !inbound.is_empty(),
    }
}

async fn start_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<StartReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(String,), (Result<String, String>,)>(&mut store, "start-server")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.region_json,))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "start-server", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn dial_tcp(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<DialReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(String, u16), (Result<String, String>,)>(&mut store, "dial-tcp")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.host, req.port))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "dial-tcp", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn parse_address(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<ParseReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(String,), (Result<String, String>,)>(&mut store, "parse-address")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.addr,))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "parse-address", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn bind_udp(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<BindReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let func = instance
            .get_typed_func::<(u16,), (Result<String, String>,)>(&mut store, "bind-udp")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.port,))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(output)
    };
    match poc_call(&p, "bind-udp", work).await? {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn cors_layer() -> Result<CorsLayer> {
    let origins = std::env::var("OWASIDEA_ALLOWED_ORIGINS").unwrap_or_else(|_| {
        "tauri://localhost,https://tauri.localhost,http://localhost:5173,http://127.0.0.1:5173"
            .to_string()
    });
    if origins.split(',').any(|origin| origin.trim() == "*") {
        anyhow::bail!("wildcard CORS origins are not allowed");
    }
    let origins = origins
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(HeaderValue::try_from)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if origins.is_empty() {
        anyhow::bail!("OWASIDEA_ALLOWED_ORIGINS must contain at least one origin");
    }
    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("last-event-id"),
        ]))
}

fn default_val(ty: &Type) -> Val {
    match ty {
        Type::Bool => Val::Bool(false),
        Type::S8 => Val::S8(0),
        Type::U8 => Val::U8(0),
        Type::S16 => Val::S16(0),
        Type::U16 => Val::U16(0),
        Type::S32 => Val::S32(0),
        Type::U32 => Val::U32(0),
        Type::S64 => Val::S64(0),
        Type::U64 => Val::U64(0),
        Type::Float32 => Val::Float32(0.0),
        Type::Float64 => Val::Float64(0.0),
        Type::Char => Val::Char('\0'),
        Type::String => Val::String(String::new()),
        Type::List(_) => Val::List(vec![]),
        Type::Option(_) => Val::Option(None),
        Type::Result(_) => Val::Result(Ok(None)),
        Type::Record(r) => {
            let fields = r
                .fields()
                .map(|f| (f.name.to_string(), default_val(&f.ty)))
                .collect();
            Val::Record(fields)
        }
        Type::Tuple(t) => Val::Tuple(t.types().map(|ty| default_val(&ty)).collect()),
        Type::Enum(_) => Val::Enum(String::new()),
        Type::Flags(_) => Val::Flags(vec![]),
        _ => Val::Bool(false),
    }
}

fn json_to_val(ty: &Type, value: &serde_json::Value) -> Result<Val, String> {
    match ty {
        Type::Bool => value
            .as_bool()
            .map(Val::Bool)
            .ok_or_else(|| "expected bool".to_string()),
        Type::S8 => value
            .as_i64()
            .map(|v| Val::S8(v as i8))
            .ok_or_else(|| "expected integer".to_string()),
        Type::U8 => value
            .as_u64()
            .map(|v| Val::U8(v as u8))
            .ok_or_else(|| "expected integer".to_string()),
        Type::S16 => value
            .as_i64()
            .map(|v| Val::S16(v as i16))
            .ok_or_else(|| "expected integer".to_string()),
        Type::U16 => value
            .as_u64()
            .map(|v| Val::U16(v as u16))
            .ok_or_else(|| "expected integer".to_string()),
        Type::S32 => value
            .as_i64()
            .map(|v| Val::S32(v as i32))
            .ok_or_else(|| "expected integer".to_string()),
        Type::U32 => value
            .as_u64()
            .map(|v| Val::U32(v as u32))
            .ok_or_else(|| "expected integer".to_string()),
        Type::S64 => value
            .as_i64()
            .map(Val::S64)
            .ok_or_else(|| "expected integer".to_string()),
        Type::U64 => value
            .as_u64()
            .map(Val::U64)
            .ok_or_else(|| "expected integer".to_string()),
        Type::Float32 => value
            .as_f64()
            .map(|v| Val::Float32(v as f32))
            .ok_or_else(|| "expected number".to_string()),
        Type::Float64 => value
            .as_f64()
            .map(Val::Float64)
            .ok_or_else(|| "expected number".to_string()),
        Type::Char => {
            let s = value
                .as_str()
                .ok_or_else(|| "expected string".to_string())?;
            let c = s
                .chars()
                .next()
                .ok_or_else(|| "expected char".to_string())?;
            Ok(Val::Char(c))
        }
        Type::String => {
            let s = value
                .as_str()
                .ok_or_else(|| "expected string".to_string())?;
            Ok(Val::String(s.to_string()))
        }
        Type::List(l) => {
            let arr = value
                .as_array()
                .ok_or_else(|| "expected array".to_string())?;
            let elem_ty = l.ty();
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                out.push(json_to_val(&elem_ty, v)?);
            }
            Ok(Val::List(out))
        }
        Type::Option(o) => {
            if value.is_null() {
                Ok(Val::Option(None))
            } else {
                let inner = json_to_val(&o.ty(), value)?;
                Ok(Val::Option(Some(Box::new(inner))))
            }
        }
        Type::Result(r) => {
            if let Some(obj) = value.as_object() {
                if let Some(okv) = obj.get("ok") {
                    let val = r
                        .ok()
                        .map(|t| json_to_val(&t, okv).map(Box::new))
                        .transpose()?;
                    return Ok(Val::Result(Ok(val)));
                }
                if let Some(errv) = obj.get("err") {
                    let val = r
                        .err()
                        .map(|t| json_to_val(&t, errv).map(Box::new))
                        .transpose()?;
                    return Ok(Val::Result(Err(val)));
                }
            }
            if value.is_null() {
                return Ok(Val::Result(Ok(None)));
            }
            Err("expected result object {ok:...} or {err:...}".to_string())
        }
        Type::Record(rec) => {
            let obj = value
                .as_object()
                .ok_or_else(|| "expected object".to_string())?;
            let mut fields = Vec::new();
            for field in rec.fields() {
                let fv = obj
                    .get(field.name)
                    .ok_or_else(|| format!("missing field {}", field.name))?;
                fields.push((field.name.to_string(), json_to_val(&field.ty, fv)?));
            }
            Ok(Val::Record(fields))
        }
        Type::Tuple(t) => {
            let arr = value
                .as_array()
                .ok_or_else(|| "expected array".to_string())?;
            let types: Vec<Type> = t.types().collect();
            if arr.len() != types.len() {
                return Err("tuple arity mismatch".to_string());
            }
            let mut out = Vec::new();
            for (ty, v) in types.iter().zip(arr.iter()) {
                out.push(json_to_val(ty, v)?);
            }
            Ok(Val::Tuple(out))
        }
        Type::Enum(_) => {
            let s = value
                .as_str()
                .ok_or_else(|| "expected string for enum".to_string())?;
            Ok(Val::Enum(s.to_string()))
        }
        Type::Flags(_) => {
            let arr = value
                .as_array()
                .ok_or_else(|| "expected array of strings for flags".to_string())?;
            let mut out = Vec::new();
            for v in arr {
                out.push(
                    v.as_str()
                        .ok_or_else(|| "expected string".to_string())?
                        .to_string(),
                );
            }
            Ok(Val::Flags(out))
        }
        _ => Err(format!("unsupported type {:?}", ty)),
    }
}

fn val_to_json(val: &Val) -> serde_json::Value {
    match val {
        Val::Bool(b) => serde_json::json!(b),
        Val::S8(v) => serde_json::json!(v),
        Val::U8(v) => serde_json::json!(v),
        Val::S16(v) => serde_json::json!(v),
        Val::U16(v) => serde_json::json!(v),
        Val::S32(v) => serde_json::json!(v),
        Val::U32(v) => serde_json::json!(v),
        Val::S64(v) => serde_json::json!(v),
        Val::U64(v) => serde_json::json!(v),
        Val::Float32(v) => serde_json::json!(v),
        Val::Float64(v) => serde_json::json!(v),
        Val::Char(c) => serde_json::json!(c.to_string()),
        Val::String(s) => serde_json::json!(s),
        Val::List(v) => serde_json::Value::Array(v.iter().map(val_to_json).collect()),
        Val::Record(fields) => {
            let mut map = serde_json::Map::new();
            for (k, v) in fields {
                map.insert(k.clone(), val_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Val::Tuple(v) => serde_json::Value::Array(v.iter().map(val_to_json).collect()),
        Val::Option(Some(b)) => val_to_json(b),
        Val::Option(None) => serde_json::Value::Null,
        Val::Result(Ok(Some(b))) => serde_json::json!({ "ok": val_to_json(b) }),
        Val::Result(Ok(None)) => serde_json::json!({ "ok": null }),
        Val::Result(Err(Some(b))) => serde_json::json!({ "err": val_to_json(b) }),
        Val::Result(Err(None)) => serde_json::json!({ "err": null }),
        Val::Enum(s) => serde_json::json!(s),
        Val::Flags(v) => serde_json::json!(v),
        _ => serde_json::Value::Null,
    }
}

/// Root for host-managed mutable state. Desktop bundles point this at the
/// platform app-data directory; development defaults to `./data`.
fn data_root() -> PathBuf {
    std::env::var_os("OWASIDEA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"))
}

fn resolve_state_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() || std::env::var_os("OWASIDEA_DATA_DIR").is_none() {
        path.to_path_buf()
    } else {
        data_root().join(path.strip_prefix("./").unwrap_or(path))
    }
}

fn open_kv_db(name: &str) -> Result<rusqlite::Connection> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        anyhow::bail!("invalid KV database namespace {name:?}");
    }
    let dir = data_root().join(name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("kv.sqlite");
    let conn = rusqlite::Connection::open(&path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// Executable plugin proxy (POC)
// ---------------------------------------------------------------------------

/// Response headers an executable plugin may set. Everything else (cookies,
/// CORS grants, framing, content-encoding, …) is dropped: `/app/{name}` is
/// served on the host origin, so a guest must not be able to set them.
const PROXIED_RESPONSE_HEADERS: [header::HeaderName; 6] = [
    header::CONTENT_TYPE,
    header::CONTENT_LENGTH,
    header::CACHE_CONTROL,
    header::ETAG,
    header::LAST_MODIFIED,
    header::EXPIRES,
];

/// Inert content types the proxy is willing to forward unchanged. HTML, XML,
/// JavaScript and CSS are served as `text/plain` so guest markup/script cannot
/// execute on the host origin; unknown types fall back to plain text too.
fn safe_content_type(value: Option<&HeaderValue>) -> HeaderValue {
    const PLAIN: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
    let raw = value.and_then(|value| value.to_str().ok()).unwrap_or("");
    let essence = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "text/plain"
        | "application/json"
        | "text/csv"
        | "application/octet-stream"
        | "image/png"
        | "image/jpeg"
        | "image/gif"
        | "image/webp" => HeaderValue::from_str(raw).unwrap_or(PLAIN),
        _ => PLAIN,
    }
}

/// Replace the guest's response headers with the allowlist plus host-enforced
/// hardening headers (`nosniff`, CSP `sandbox`).
fn sanitize_proxied_response(parts: &mut axum::http::response::Parts) {
    let guest = std::mem::take(&mut parts.headers);
    let mut headers = axum::http::HeaderMap::new();
    for name in PROXIED_RESPONSE_HEADERS {
        if let Some(value) = guest.get(&name) {
            headers.insert(name, value.clone());
        }
    }
    headers.insert(
        header::CONTENT_TYPE,
        safe_content_type(guest.get(header::CONTENT_TYPE)),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("sandbox"),
    );
    parts.headers = headers;
}

async fn proxy_app(
    State(state): State<AppState>,
    AxumPath(params): AxumPath<HashMap<String, String>>,
    request: axum::http::Request<Body>,
) -> Result<Response, (StatusCode, String)> {
    let name = params
        .get("name")
        .cloned()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing plugin name".to_string()))?;
    let p = state
        .plugin(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let pre = p.http_pre.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("plugin {name} has no wasi:http incoming handler"),
        )
    })?;
    let path = params.get("path").cloned().unwrap_or_default();
    let (mut parts, body) = request.into_parts();
    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(|error| (StatusCode::PAYLOAD_TOO_LARGE, error.to_string()))?;
    let target = format!(
        "http://plugin.local/{}{}",
        path,
        parts
            .uri
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    );
    parts.uri = target
        .parse()
        .map_err(|error: axum::http::uri::InvalidUri| {
            (StatusCode::BAD_REQUEST, error.to_string())
        })?;
    parts.headers.remove(header::HOST);
    let guest_request =
        axum::http::Request::from_parts(parts, http_body_util::Full::new(body_bytes));

    use wasmtime_wasi_http::p2::bindings::http::types::Scheme;
    let start = std::time::Instant::now();
    let operation = if path.is_empty() { "/" } else { path.as_str() };
    let span = otel::call_span(&name, operation, "executable");
    let result = async {
        let mut store = create_store(&state.engine, p.as_ref())
            .await
            .map_err(internal)?;
        let incoming_request = store
            .data_mut()
            .http()
            .new_incoming_request(Scheme::Http, guest_request)
            .map_err(internal)?;
        let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
        let response_outparam = store
            .data_mut()
            .http()
            .new_response_outparam(response_sender)
            .map_err(internal)?;
        let pre = pre.clone();
        let task = tokio::spawn(async move {
            let proxy = pre.instantiate_async(&mut store).await?;
            proxy
                .wasi_http_incoming_handler()
                .call_handle(&mut store, incoming_request, response_outparam)
                .await?;
            Ok::<(), wasmtime::Error>(())
        });
        let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
        let guest_response = match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            response_receiver,
        )
        .await
        {
            Ok(Ok(Ok(response))) => response,
            Ok(Ok(Err(error))) => return Err(internal(error)),
            Ok(Err(_)) => {
                let error = task
                    .await
                    .map_err(internal)?
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "guest did not set HTTP response".to_string());
                return Err((StatusCode::BAD_GATEWAY, error));
            }
            Err(_) => {
                task.abort();
                return Err((
                    StatusCode::GATEWAY_TIMEOUT,
                    "plugin request timed out".to_string(),
                ));
            }
        };
        let (mut parts, body) = guest_response.into_parts();
        sanitize_proxied_response(&mut parts);
        Ok::<_, (StatusCode, String)>(Response::from_parts(parts, Body::new(body)))
    }
    .instrument(span.clone())
    .await;
    let success = result
        .as_ref()
        .map(|response| response.status().is_success())
        .unwrap_or(false);
    otel::record_outcome(&span, success);
    record_plugin_metric(
        &state,
        &name,
        success,
        start.elapsed().as_millis() as u64,
        0,
    );
    result
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

fn compilation_cache() -> Option<wasmtime::Cache> {
    if std::env::var("OWASIDEA_WASMTIME_CACHE").as_deref() == Ok("off") {
        return None;
    }
    let dir = std::env::var_os("OWASIDEA_WASMTIME_CACHE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("OWASIDEA_DATA_DIR")
                .map(|dir| PathBuf::from(dir).join("wasmtime-cache"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("owasidea-wasmtime-cache"));
    let mut cache_config = wasmtime::CacheConfig::new();
    cache_config.with_directory(dir);
    match wasmtime::Cache::new(cache_config) {
        Ok(cache) => Some(cache),
        Err(error) => {
            tracing::warn!("wasmtime compilation cache disabled: {error}");
            None
        }
    }
}

/// Only `*.manifest.json` files are plugin manifests. Other JSON files that
/// live next to them (`permissions.lock.json`, `permissions.diff.json`, …) are
/// side artifacts and must not be parsed as manifests.
fn is_manifest_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".manifest.json"))
}

fn load_manifests(dir: &Path) -> Result<Vec<(Manifest, serde_json::Value, PathBuf)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if !is_manifest_file(&path) {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).with_context(|| format!("invalid manifest {path:?}"))?;
        let manifest: Manifest = serde_json::from_value(value.clone())
            .with_context(|| format!("invalid manifest {path:?}"))?;
        validate_manifest_abi(&manifest).with_context(|| format!("invalid manifest {path:?}"))?;
        out.push((manifest, value, path));
    }
    Ok(out)
}

fn validate_manifest_abi(manifest: &Manifest) -> Result<()> {
    match manifest.schema_version {
        0 => {
            // Legacy manifests: optional fields validated only if present.
            if let Some(host_abi) = &manifest.host_abi {
                if host_abi != HOST_ABI {
                    anyhow::bail!(
                        "plugin {} manifest host_abi {} is incompatible with host {}",
                        manifest.name,
                        host_abi,
                        HOST_ABI
                    );
                }
            }
            if let Some(min_host) = &manifest.min_host {
                let required = semver::Version::parse(min_host)
                    .with_context(|| format!("invalid min_host {min_host:?}"))?;
                let host = semver::Version::parse(HOST_VERSION)
                    .with_context(|| format!("invalid host version {HOST_VERSION:?}"))?;
                if required > host {
                    anyhow::bail!(
                        "plugin {} manifest min_host {} is greater than host {}",
                        manifest.name,
                        min_host,
                        HOST_VERSION
                    );
                }
            }
        }
        1 => {
            let host_abi = manifest.host_abi.as_deref().ok_or_else(|| {
                anyhow!(
                    "plugin {} manifest host_abi is required for schema_version 1",
                    manifest.name
                )
            })?;
            if host_abi != HOST_ABI {
                anyhow::bail!(
                    "plugin {} manifest host_abi {} is incompatible with host {}",
                    manifest.name,
                    host_abi,
                    HOST_ABI
                );
            }
            let min_host = manifest.min_host.as_deref().ok_or_else(|| {
                anyhow!(
                    "plugin {} manifest min_host is required for schema_version 1",
                    manifest.name
                )
            })?;
            let required = semver::Version::parse(min_host)
                .with_context(|| format!("invalid min_host {min_host:?}"))?;
            let host = semver::Version::parse(HOST_VERSION)
                .with_context(|| format!("invalid host version {HOST_VERSION:?}"))?;
            if required > host {
                anyhow::bail!(
                    "plugin {} manifest min_host {} is greater than host {}",
                    manifest.name,
                    min_host,
                    HOST_VERSION
                );
            }
        }
        other => {
            anyhow::bail!(
                "plugin {} manifest schema_version {} is not supported (expected 0 or 1)",
                manifest.name,
                other
            );
        }
    }
    Ok(())
}

fn validate_plugin_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        anyhow::bail!("invalid plugin name {name:?}");
    }
    Ok(())
}

fn validate_ui_entry(entry: &str) -> Result<()> {
    let path = Path::new(entry);
    if entry.is_empty()
        || path.is_absolute()
        || !entry
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        anyhow::bail!("UI plugin entry must be a safe relative path: {entry:?}");
    }
    Ok(())
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .is_ok_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

/// The host API is local by default. Binding to a non-loopback address (for
/// example `0.0.0.0`) exposes every plugin route to the network and therefore
/// requires an explicit opt-in.
fn ensure_loopback_bind(addr: &str, allow_non_loopback: bool) -> Result<()> {
    if allow_non_loopback {
        return Ok(());
    }
    let parsed = if let Ok(parsed) = addr.parse::<SocketAddr>() {
        parsed
    } else if let Some(port) = addr
        .strip_prefix("localhost:")
        .and_then(|port| port.parse::<u16>().ok())
    {
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    } else {
        anyhow::bail!("invalid bind address {addr:?}; expected host:port");
    };
    if !parsed.ip().is_loopback() {
        anyhow::bail!(
            "refusing to bind non-loopback address {addr}; pass --allow-non-loopback or set OWASIDEA_ALLOW_NON_LOOPBACK=1"
        );
    }
    Ok(())
}

pub async fn run(manifests_dir: PathBuf, addr: String) -> Result<()> {
    // Plain logs by default; OTLP export only when the `otel` feature is on and
    // OTEL_EXPORTER_OTLP_ENDPOINT is set (RL-05).
    let telemetry = otel::init_tracing();

    let loaded = load_manifests(&manifests_dir)?;
    println!("loading {} plugin manifests", loaded.len());
    if loaded.is_empty() {
        return Err(anyhow!("no manifests found in {manifests_dir:?}"));
    }

    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    // Enable fuel-based execution metering (manifest resources.max_fuel).
    config.consume_fuel(true);
    // Compiling a Go/JS/C# component takes ~0.3-1.2 s. Persist the compiled
    // artifact so restarts and `owasidea dev` reloads do not pay it again.
    config.cache(compilation_cache());
    let engine = Engine::new(&config)?;
    println!("wasmtime engine ready");

    let mut plugins = HashMap::new();
    let mut ui_plugins = Vec::new();
    let mut seen_plugins = HashSet::new();

    // Component compilation is CPU-bound and dominates startup. Compile every
    // headless plugin once, concurrently, and reuse the result for all calls.
    let startup_started = std::time::Instant::now();
    let mut compiled = HashMap::new();
    {
        let mut pending = Vec::new();
        for (manifest, manifest_value, manifest_path) in &loaded {
            if manifest.runtime.as_deref() == Some("js-wasm") {
                continue;
            }
            let component_path = manifest_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&manifest.component);
            if !component_path.exists() {
                return Err(anyhow!(
                    "component not found at {:?} for plugin {}",
                    component_path,
                    manifest.name
                ));
            }
            pending.push((
                manifest.name.clone(),
                manifest.clone(),
                manifest_value.clone(),
                component_path,
            ));
        }
        // Compiling every component at once can exhaust a 2-core CI runner;
        // cap the batch to the CPUs we actually have (and at most 4).
        let concurrency = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(2)
            .clamp(1, 4);
        for chunk in pending.chunks(concurrency) {
            let mut jobs = Vec::new();
            for (name, manifest, manifest_value, component_path) in chunk {
                let engine = engine.clone();
                let name = name.clone();
                let manifest = manifest.clone();
                let manifest_value = manifest_value.clone();
                let component_path = component_path.clone();
                println!("compiling component {name}");
                jobs.push((
                    name,
                    tokio::task::spawn_blocking(move || {
                        verify_and_compile_component(
                            &engine,
                            &manifest,
                            &manifest_value,
                            &component_path,
                        )
                    }),
                ));
            }
            for (name, job) in jobs {
                let component = job
                    .await
                    .with_context(|| format!("component compilation task failed for {name}"))??;
                println!("compiled component {name}");
                compiled.insert(name, component);
            }
        }
    }

    for (manifest, manifest_value, manifest_path) in loaded {
        validate_plugin_name(&manifest.name)?;
        if !seen_plugins.insert(manifest.name.clone()) {
            anyhow::bail!("duplicate plugin manifest for {}", manifest.name);
        }
        if manifest.runtime.as_deref() == Some("js-wasm") {
            let ui = manifest.metadata.get("ui").ok_or_else(|| {
                anyhow!("js-wasm plugin {} is missing metadata.ui", manifest.name)
            })?;
            let entry = ui
                .get("entry")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "js-wasm plugin {} is missing metadata.ui.entry",
                        manifest.name
                    )
                })?;
            validate_ui_entry(entry)?;
            let files: std::collections::BTreeMap<String, String> =
                serde_json::from_value(ui.get("files").cloned().unwrap_or_default())
                    .with_context(|| format!("invalid UI file list for {}", manifest.name))?;
            if files.is_empty() || !files.contains_key(entry) {
                anyhow::bail!(
                    "UI plugin {} must list entry in metadata.ui.files",
                    manifest.name
                );
            }
            for (path, digest) in &files {
                validate_ui_entry(path)?;
                if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    anyhow::bail!("invalid UI asset digest for {}: {path}", manifest.name);
                }
            }
            let sha256 = ui
                .get("sha256")
                .and_then(serde_json::Value::as_str)
                .filter(|digest| {
                    digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
                })
                .ok_or_else(|| {
                    anyhow!(
                        "UI plugin {} is missing a valid bundle digest",
                        manifest.name
                    )
                })?
                .to_string();
            verify_manifest_signature(&manifest_value, &[])?;
            ui_plugins.push(UiPluginInfo {
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                language: manifest.language.clone(),
                entry: format!("plugins/{}/{}", manifest.name, entry),
                sha256,
                files,
                bridge: ui
                    .get("bridge")
                    .and_then(serde_json::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                timeout_ms: ui_bridge_timeout_ms(ui, &manifest_value),
            });
            tracing::info!(
                "skipping js-wasm plugin {} (served by the webview UI, not Wasmtime)",
                manifest.name
            );
            continue;
        }

        let component_path = manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&manifest.component);

        if !component_path.exists() {
            return Err(anyhow!(
                "component not found at {:?} for plugin {}",
                component_path,
                manifest.name
            ));
        }

        println!("preparing component {}", manifest.name);
        let component = compiled.remove(&manifest.name).ok_or_else(|| {
            anyhow!(
                "component for plugin {} was not compiled at startup",
                manifest.name
            )
        })?;
        let runtime = PluginRuntime {
            manifest: manifest.clone(),
            manifest_path: manifest_path.clone(),
            component,
            kv: Arc::new(std::sync::Mutex::new(open_kv_db(
                manifest
                    .permissions
                    .kv
                    .as_ref()
                    .map(|permission| permission.namespace.as_str())
                    .unwrap_or(&manifest.name),
            )?)),
            logs: Arc::new(Mutex::new(LogBuffer::new(&manifest.name))),
            instance_slots: instance_slots(&manifest)?,
            http_pre: None,
        };

        let mut runtime = runtime;

        runtime.http_pre = prepare_http_plugin(&engine, &runtime)
            .with_context(|| format!("HTTP preparation failed for {}", manifest.name))?;
        // Instantiate every WASI component at startup so bad imports/policies
        // fail fast, including executable plugins served by the embedded host.
        instantiate(&engine, &runtime)
            .await
            .with_context(|| format!("startup smoke test failed for {}", manifest.name))?;
        println!("instantiated component {}", manifest.name);

        plugins.insert(manifest.name.clone(), std::sync::Arc::new(runtime));
    }

    println!(
        "loaded {} plugins in {} ms",
        plugins.len(),
        startup_started.elapsed().as_millis()
    );

    let plugin_metrics: Arc<plugin_metrics::PluginMetricsMap> = Arc::new(std::sync::RwLock::new(
        plugins
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    Arc::new(plugin_metrics::PluginMetrics::default()),
                )
            })
            .collect(),
    ));
    let plugins = Arc::new(std::sync::RwLock::new(plugins));
    let requests = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let errors = Arc::new(std::sync::atomic::AtomicU64::new(0));

    #[cfg(feature = "streaming")]
    let jobs = jobs::JobManager::new(Arc::new(jobs::HostJobExecutor::new(
        engine.clone(),
        plugins.clone(),
        plugin_metrics.clone(),
        requests.clone(),
    )));

    let state = AppState {
        engine,
        plugins,
        ui_plugins: Arc::new(ui_plugins),
        tailcat_session: Arc::new(Mutex::new(None)),
        services: Arc::new(Mutex::new(HashMap::new())),
        service_health: Arc::new(Mutex::new(HashMap::new())),
        requests,
        errors,
        plugin_metrics,
        dev_mode: std::env::var("OWASIDEA_DEV_MODE")
            .is_ok_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")),
        dev_hot_reload: std::env::var("OWASIDEA_DEV_HOT_RELOAD")
            .is_ok_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")),
        dev_watch_paths: std::env::var("OWASIDEA_DEV_WATCH_PATHS")
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
        dev_last_reload: Arc::new(std::sync::RwLock::new(std::collections::BTreeMap::new())),
        manifests_dir: manifests_dir.clone(),
        #[cfg(feature = "streaming")]
        jobs,
    };

    let app = build_router(state)?;

    let allow_non_loopback = env_flag("OWASIDEA_ALLOW_NON_LOOPBACK");
    ensure_loopback_bind(&addr, allow_non_loopback)?;
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local_addr = listener.local_addr()?;
    if !allow_non_loopback && !local_addr.ip().is_loopback() {
        anyhow::bail!(
            "refusing to serve on non-loopback address {local_addr}; pass --allow-non-loopback or set OWASIDEA_ALLOW_NON_LOOPBACK=1"
        );
    }
    tracing::info!("listening on http://{}", local_addr);
    println!("host listening on http://{}", local_addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    // Flush buffered spans before the process exits.
    if let Some(telemetry) = telemetry {
        telemetry.shutdown();
    }
    Ok(())
}

async fn count_requests(
    State(state): State<AppState>,
    req: axum::http::Request<Body>,
    next: middleware::Next,
) -> Response {
    state
        .requests
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let reload_plugin = req
        .uri()
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let resp = next.run(req).await;
    if resp.status().is_client_error() || resp.status().is_server_error() {
        state
            .errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if state.dev_mode
        && reload_plugin.len() == 3
        && reload_plugin[0] == "plugin"
        && reload_plugin[2] == "reload"
    {
        if let Ok(mut records) = state.dev_last_reload.write() {
            records.insert(
                reload_plugin[1].to_string(),
                ReloadStatus {
                    ok: resp.status().is_success(),
                    http_status: resp.status().as_u16(),
                    timestamp_unix: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                },
            );
        }
    }
    resp
}

fn build_router(state: AppState) -> Result<Router> {
    let tailcat_routes = tailcat_routes_enabled(&state);
    build_router_inner(state, tailcat_routes)
}

/// Tailcat is an optional example integration, not part of the generic host
/// surface. Its hardcoded routes are registered only when the plugin is loaded
/// or `OWASIDEA_ENABLE_TAILCAT=1` is set; otherwise they 404 like any unknown
/// route (see docs/API.md).
fn tailcat_routes_enabled(state: &AppState) -> bool {
    state.plugin("tailcat-wasi").is_some() || env_flag("OWASIDEA_ENABLE_TAILCAT")
}

fn build_router_inner(state: AppState, tailcat_routes: bool) -> Result<Router> {
    let mut app = Router::new()
        .route("/health", get(health))
        .route("/overview", get(overview))
        .route("/metrics", get(metrics))
        .route("/dev/status", get(dev_status))
        .route("/plugins", get(list_plugins))
        .route("/ui-plugins", get(list_ui_plugins))
        .route("/plugin/{name}/manifest", get(plugin_manifest))
        .route("/permissions/diff", get(permissions_diff))
        .route("/plugin/{name}/logs", get(plugin_logs))
        .route("/plugin/{name}/logs/stream", get(plugin_logs_stream))
        .route("/plugin/{name}/reload", post(reload))
        .route("/plugin/{name}/greet", post(greet))
        .route("/plugin/{name}/complete", post(complete))
        .route("/plugin/{name}/call", post(call_export))
        .route("/plugin/{name}/service/start", post(service_start_generic))
        .route("/plugin/{name}/service/stop", post(service_stop_generic))
        .route("/plugin/{name}/service/status", get(service_status_generic))
        .route(
            "/plugin/{name}/service/restart",
            post(service_restart_generic),
        )
        .route("/plugin/{name}/bind-udp", post(bind_udp))
        .route("/plugin/{name}/parse-address", post(parse_address))
        .route("/plugin/{name}/dial-tcp", post(dial_tcp))
        .route("/plugin/{name}/start-server", post(start_server))
        .route("/plugin/{name}/tls-probe", post(tls_probe))
        .route("/plugin/{name}/dial-echo", post(dial_echo))
        .route("/plugin/{name}/echo-udp", post(echo_udp))
        .route("/app/{name}/{*path}", axum::routing::any(proxy_app))
        .route("/app/{name}/", axum::routing::any(proxy_app))
        .route("/app/{name}", axum::routing::any(proxy_app));

    if tailcat_routes {
        app = app
            .route("/plugin/tailcat-wasi/start-service", post(start_service))
            .route("/plugin/tailcat-wasi/stop-service", post(stop_service))
            .route("/plugin/tailcat-wasi/service-status", get(service_status))
            .route("/plugin/tailcat-wasi/send-text", post(send_text))
            .route("/plugin/tailcat-wasi/drain", get(drain_tailcat));
    }

    // Experimental long-call jobs (ADR-0005). Registered only with the feature.
    #[cfg(feature = "streaming")]
    let app = app.merge(
        Router::new()
            .route("/plugin/{name}/call/jobs", post(jobs::create_job))
            .route("/jobs/{id}", get(jobs::get_job))
            .route("/jobs/{id}/events", get(jobs::job_events))
            .route("/jobs/{id}/cancel", post(jobs::cancel_job))
            .layer(middleware::from_fn(jobs::no_store)),
    );

    // W3C traceparent propagation: only compiled with the `otel` feature, and a
    // passthrough at runtime unless OTLP export is configured.
    #[cfg(feature = "otel")]
    let app = app.layer(middleware::from_fn(otel::trace_context));

    Ok(app
        .layer(cors_layer()?)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            count_requests,
        ))
        .with_state(state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_deserializes() {
        let raw = r#"{"name":"x","version":"0.1.0","language":"rust","abi":"wasi-0.2","runtime":"wasi-component","type":"library","component":"x.wasm","exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},"permissions":{},"state":{},"composition":{},"metadata":{}}"#;
        let m: Manifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.name, "x");
    }

    #[test]
    fn load_manifests_ignores_non_manifest_json() {
        const VALID: &str = r#"{"name":"x","version":"0.1.0","language":"rust","abi":"wasi-0.2","runtime":"wasi-component","type":"library","component":"x.wasm","exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},"permissions":{},"state":{},"composition":{},"metadata":{}}"#;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.manifest.json"), VALID).unwrap();
        std::fs::write(
            dir.path().join("permissions.lock.json"),
            r#"{"plugins":{"x":{"host":[]}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("permissions.diff.json"),
            r#"{"widened":[]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.json"), r#"{"hello":"world"}"#).unwrap();
        std::fs::write(dir.path().join("README.md"), "not json").unwrap();

        let loaded = load_manifests(dir.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0.name, "x");
        assert_eq!(loaded[0].2.file_name().unwrap(), "x.manifest.json");
    }

    #[test]
    fn load_manifests_still_rejects_broken_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.manifest.json"), r#"{"name":42}"#).unwrap();
        let error = load_manifests(dir.path()).unwrap_err().to_string();
        assert!(error.contains("invalid manifest"), "{error}");
    }

    #[test]
    fn socket_policy_denies_undeclared_destinations_and_bind_addresses() {
        let outbound = HashSet::from(["203.0.113.10".parse::<IpAddr>().unwrap()]);
        let inbound = HashSet::from(["127.0.0.1".parse::<IpAddr>().unwrap()]);
        assert!(socket_address_allowed(
            "203.0.113.10:443".parse().unwrap(),
            SocketAddrUse::TcpConnect,
            &outbound,
            &inbound,
        ));
        assert!(!socket_address_allowed(
            "203.0.113.11:443".parse().unwrap(),
            SocketAddrUse::TcpConnect,
            &outbound,
            &inbound,
        ));
        assert!(socket_address_allowed(
            "127.0.0.1:8080".parse().unwrap(),
            SocketAddrUse::TcpBind,
            &outbound,
            &inbound,
        ));
        assert!(!socket_address_allowed(
            "0.0.0.0:8080".parse().unwrap(),
            SocketAddrUse::TcpBind,
            &outbound,
            &inbound,
        ));
    }

    #[test]
    fn plugin_log_buffer_is_bounded_and_truncates_messages() {
        let mut logs = LogBuffer::new("notes");
        for id in 0..=MAX_LOG_ENTRIES {
            logs.push("info", format!("log-{id}"), 0, None);
        }
        assert_eq!(logs.entries.len(), MAX_LOG_ENTRIES);
        assert_eq!(logs.entries.front().unwrap().0, 2);
        assert_eq!(logs.entries.back().unwrap().0, (MAX_LOG_ENTRIES + 1) as u64);

        logs.push("info", "x".repeat(MAX_LOG_CHARS + 10), 0, None);
        assert_eq!(
            logs.entries.back().unwrap().1.message.chars().count(),
            MAX_LOG_CHARS
        );
    }

    #[test]
    fn log_entry_serializes_structured_fields() {
        let mut logs = LogBuffer::new("notes");
        logs.push(
            "warn",
            "hello".to_string(),
            42,
            Some(serde_json::json!({"k": "v"})),
        );
        let entry = logs.entries.back().unwrap().1.clone();
        assert_eq!(entry.level, "warn");
        assert_eq!(entry.plugin, "notes");
        assert_eq!(entry.request_id, 42);
        assert_eq!(entry.message, "hello");
        let v = serde_json::to_value(&entry).unwrap();
        assert!(v.get("ts").is_some());
        assert_eq!(v["level"], "warn");
        assert_eq!(v["request_id"], 42);
        assert_eq!(v["fields"]["k"], "v");
    }

    #[test]
    fn plugin_signature_payload_covers_manifest_permissions_and_component() {
        let raw = r#"{"name":"x","version":"0.1.0","language":"rust","abi":"wasi-0.2","runtime":"wasi-component","type":"library","component":"x.wasm","exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},"permissions":{"network":{"outbound":[],"inbound":[]}},"state":{},"composition":{},"metadata":{"sha256":"abc"}}"#;
        let mut manifest: serde_json::Value = serde_json::from_str(raw).unwrap();
        let original = signed_manifest_payload(&manifest, b"component bytes").unwrap();
        manifest["permissions"]["network"]["outbound"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("example.com"));
        let changed_policy = signed_manifest_payload(&manifest, b"component bytes").unwrap();
        let changed_component = signed_manifest_payload(&manifest, b"changed component").unwrap();
        assert_ne!(original, changed_policy);
        assert_ne!(changed_policy, changed_component);
    }

    #[test]
    fn ed25519_signature_authenticates_manifest_and_component() {
        use base64::Engine;
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let raw = r#"{"name":"signed","version":"1.0.0","language":"rust","abi":"wasi-0.2","runtime":"wasi-component","type":"library","component":"signed.wasm","exports":[],"imports":{"wasi":[],"host":[],"plugins":[]},"permissions":{"filesystem":[],"http":{"outbound":[],"inbound":false},"network":{"outbound":[],"inbound":[]},"kv":null,"resources":{"max_memory_mb":64,"max_fuel":1000000,"timeout_ms":5000,"max_instances":1}},"state":{"persistent":false,"storage":"none","root":null},"composition":{"strategy":"build-time","entry":false},"metadata":{"sha256":"hash","signature_format":"owasidea-manifest-v1"}}"#;
        let mut manifest: serde_json::Value = serde_json::from_str(raw).unwrap();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let payload = signed_manifest_payload(&manifest, b"artifact").unwrap();
        let signature = pair.sign(&payload);
        manifest["metadata"]["signature"] =
            serde_json::json!(base64::engine::general_purpose::STANDARD.encode(signature.as_ref()));
        assert!(verify_signature_with_key(
            &manifest,
            b"artifact",
            pair.public_key().as_ref(),
            signature.as_ref(),
            "signed",
        )
        .is_ok());

        manifest["permissions"]["network"]["outbound"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("attacker.example"));
        assert!(verify_signature_with_key(
            &manifest,
            b"artifact",
            pair.public_key().as_ref(),
            signature.as_ref(),
            "signed",
        )
        .is_err());
    }

    fn test_app_state(plugin_metrics: plugin_metrics::PluginMetricsMap) -> AppState {
        AppState {
            engine: Engine::default(),
            plugins: Arc::new(std::sync::RwLock::new(HashMap::new())),
            ui_plugins: Arc::new(Vec::new()),
            tailcat_session: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(HashMap::new())),
            service_health: Arc::new(Mutex::new(HashMap::new())),
            requests: Arc::new(std::sync::atomic::AtomicU64::new(5)),
            errors: Arc::new(std::sync::atomic::AtomicU64::new(2)),
            plugin_metrics: Arc::new(plugin_metrics),
            dev_mode: false,
            dev_hot_reload: false,
            dev_watch_paths: Vec::new(),
            dev_last_reload: Arc::new(std::sync::RwLock::new(std::collections::BTreeMap::new())),
            manifests_dir: PathBuf::from("."),
            #[cfg(feature = "streaming")]
            jobs: jobs::test_manager(),
        }
    }

    #[cfg(not(feature = "streaming"))]
    #[tokio::test]
    async fn job_routes_are_absent_without_streaming_feature() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = test_app_state(std::sync::RwLock::new(HashMap::new()));
        let router = build_router(state).expect("router");

        let call = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/plugin/notes/call")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"function":"greet","args":["x"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(call.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(call.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("unknown plugin"));

        for (method, uri) in [
            ("POST", "/plugin/notes/call/jobs"),
            ("GET", "/jobs/abc"),
            ("GET", "/jobs/abc/events"),
            ("POST", "/jobs/abc/cancel"),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{method} {uri} must not be routed without the streaming feature"
            );
        }
    }

    #[test]
    fn non_loopback_bind_requires_explicit_opt_in() {
        assert!(ensure_loopback_bind("127.0.0.1:8787", false).is_ok());
        assert!(ensure_loopback_bind("[::1]:8787", false).is_ok());
        assert!(ensure_loopback_bind("localhost:8787", false).is_ok());
        assert!(ensure_loopback_bind("0.0.0.0:8787", false).is_err());
        assert!(ensure_loopback_bind("[::]:8787", false).is_err());
        assert!(ensure_loopback_bind("0.0.0.0:8787", true).is_ok());
    }

    #[test]
    fn service_address_accepts_bare_host_port_only() {
        assert_eq!(
            service_address("127.0.0.1:9000"),
            Some("127.0.0.1:9000".to_string())
        );
        assert_eq!(
            service_address("[::1]:9000"),
            Some("[::1]:9000".to_string())
        );
        assert_eq!(service_address("heartbeat service started"), None);
        assert_eq!(service_address("started on 127.0.0.1:9000"), None);
        assert_eq!(service_address(""), None);
    }

    #[test]
    fn service_status_exposes_failure_reason_and_restarts() {
        let health = ServiceHealth {
            last_error: Some("no start-service export".to_string()),
            restarts: 2,
        };
        let value = serde_json::to_value(ServiceStatusRes::new(false, None, &health)).unwrap();
        assert_eq!(value["running"], false);
        assert_eq!(value["address"], serde_json::Value::Null);
        assert_eq!(value["last_error"], "no start-service export");
        assert_eq!(value["restarts"], 2);
    }

    #[test]
    fn ui_bridge_timeout_reads_manifest_and_defaults_to_none() {
        let ui = serde_json::json!({ "entry": "index.html" });
        let manifest =
            serde_json::json!({ "permissions": { "resources": { "timeout_ms": 1500 } } });
        assert_eq!(ui_bridge_timeout_ms(&ui, &manifest), Some(1500));

        let ui_with_timeout = serde_json::json!({ "timeout_ms": 2500 });
        assert_eq!(
            ui_bridge_timeout_ms(&ui_with_timeout, &manifest),
            Some(2500)
        );

        let no_timeout =
            serde_json::json!({ "permissions": { "resources": { "timeout_ms": null } } });
        assert_eq!(ui_bridge_timeout_ms(&ui, &no_timeout), None);
    }

    /// Minimal empty component binary (component-model magic + version).
    const EMPTY_COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";

    /// Guest export that blocks in a WASI timer instead of returning: the
    /// in-process stand-in for a socket call that never resolves.
    const SLEEPY_DIAL_TCP_WAT: &str = r#"
(component
  (import "wasi:io/poll@0.2.0" (instance $poll
    (export "pollable" (type $pollable0 (sub resource)))
    (export "[method]pollable.block" (func $block (param "self" (borrow $pollable0))))
  ))
  (alias export $poll "pollable" (type $pollable))
  (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock
    (export "subscribe-duration" (func $subscribe (param "when" u64) (result (own $pollable))))
  ))
  (alias export $poll "[method]pollable.block" (func $block_fn))
  (alias export $clock "subscribe-duration" (func $subscribe_fn))
  (core func $block_lowered (canon lower (func $block_fn)))
  (core func $subscribe_lowered (canon lower (func $subscribe_fn)))
  (core module $m
    (import "" "subscribe" (func $subscribe_core (param i64) (result i32)))
    (import "" "block" (func $block_core (param i32)))
    (memory (export "memory") 1)
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 0)
    (func (export "dial-tcp") (param i32 i32 i32) (result i32)
      i64.const 2000000000
      call $subscribe_core
      call $block_core
      i32.const 0))
  (core instance $i (instantiate $m
    (with "" (instance
      (export "subscribe" (func $subscribe_lowered))
      (export "block" (func $block_lowered))))))
  (func (export "dial-tcp") (param "host" string) (param "port" u16)
    (result (result string (error string)))
    (canon lift (core func $i "dial-tcp")
      (memory (core memory $i "memory"))
      (realloc (core func $i "cabi_realloc"))))
)
"#;

    fn poc_test_plugin(
        engine: &Engine,
        timeout_ms: Option<u64>,
        wat: Option<&str>,
    ) -> PluginRuntime {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "name": "poc-spin",
            "version": "0.1.0",
            "language": "rust",
            "abi": "wasi-0.2",
            "runtime": "wasi-component",
            "type": "library",
            "component": "poc-spin.wasm",
            "exports": [],
            "imports": { "wasi": [], "host": [], "plugins": [] },
            "permissions": { "resources": { "timeout_ms": timeout_ms } },
            "state": {},
            "composition": {},
            "metadata": {}
        }))
        .unwrap();
        let component = match wat {
            Some(wat) => Component::new(engine, wat).expect("compile WAT component"),
            None => Component::from_binary(engine, EMPTY_COMPONENT).unwrap(),
        };
        PluginRuntime {
            manifest,
            manifest_path: PathBuf::from("poc-spin.json"),
            component,
            kv: Arc::new(std::sync::Mutex::new(
                rusqlite::Connection::open_in_memory().unwrap(),
            )),
            logs: Arc::new(Mutex::new(LogBuffer::new("poc-spin"))),
            instance_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            http_pre: None,
        }
    }

    #[test]
    fn poc_route_timeout_defaults_to_ten_seconds() {
        let engine = Engine::default();
        assert_eq!(
            poc_timeout_ms(&poc_test_plugin(&engine, None, None)),
            DEFAULT_POC_TIMEOUT_MS
        );
        assert_eq!(DEFAULT_POC_TIMEOUT_MS, 10_000);
        assert_eq!(
            poc_timeout_ms(&poc_test_plugin(&engine, Some(250), None)),
            250
        );
    }

    /// End-to-end: a POC route whose export never returns must answer 504 after
    /// the manifest timeout and give the `max_instances` slot back.
    #[tokio::test]
    async fn poc_route_timeout_answers_504_and_releases_the_instance_slot() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("engine");
        let plugin = Arc::new(poc_test_plugin(
            &engine,
            Some(50),
            Some(SLEEPY_DIAL_TCP_WAT),
        ));
        let name = plugin.manifest.name.clone();
        let mut state = test_app_state(std::sync::RwLock::new(HashMap::new()));
        state.engine = engine;
        state.plugins = Arc::new(std::sync::RwLock::new(HashMap::from([(
            name.clone(),
            plugin.clone(),
        )])));
        let router = build_router(state).expect("router");

        let started = std::time::Instant::now();
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/plugin/{name}/dial-tcp"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"host":"example.com","port":443}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("dial-tcp timed out after 50 ms"),
            "unexpected body: {}",
            String::from_utf8_lossy(&body)
        );
        // The guest asked for 2 s; the 50 ms manifest timeout must cut it short.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "POC route timeout did not interrupt the guest: {:?}",
            started.elapsed()
        );
        assert_eq!(
            plugin.instance_slots.available_permits(),
            1,
            "timed-out call must release its instance slot"
        );
    }

    #[test]
    fn proxied_response_headers_are_sanitized() {
        let response = axum::http::Response::builder()
            .status(200)
            .header("content-type", "text/html; charset=utf-8")
            .header("set-cookie", "session=guest")
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-credentials", "true")
            .header("x-guest", "1")
            .header("cache-control", "no-store")
            .body(())
            .unwrap();
        let (mut parts, _) = response.into_parts();
        sanitize_proxied_response(&mut parts);

        assert_eq!(
            parts.headers.get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(parts.headers.get("cache-control").unwrap(), "no-store");
        assert_eq!(
            parts.headers.get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert_eq!(
            parts.headers.get("content-security-policy").unwrap(),
            "sandbox"
        );
        for dropped in [
            "set-cookie",
            "access-control-allow-origin",
            "access-control-allow-credentials",
            "x-guest",
        ] {
            assert!(
                parts.headers.get(dropped).is_none(),
                "{dropped} must not be forwarded"
            );
        }
    }

    #[test]
    fn proxied_response_keeps_inert_content_types_only() {
        let plain = safe_content_type(Some(&HeaderValue::from_static("application/json")));
        assert_eq!(plain, "application/json");
        let js = safe_content_type(Some(&HeaderValue::from_static("application/javascript")));
        assert_eq!(js, "text/plain; charset=utf-8");
        let svg = safe_content_type(Some(&HeaderValue::from_static("image/svg+xml")));
        assert_eq!(svg, "text/plain; charset=utf-8");
        let missing = safe_content_type(None);
        assert_eq!(missing, "text/plain; charset=utf-8");
    }

    #[tokio::test]
    async fn tailcat_routes_are_absent_unless_enabled() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let state = test_app_state(std::sync::RwLock::new(HashMap::new()));
        let status_request = || {
            Request::builder()
                .uri("/plugin/tailcat-wasi/service-status")
                .body(Body::empty())
                .unwrap()
        };

        let disabled = build_router_inner(state.clone(), false).unwrap();
        let response = disabled.oneshot(status_request()).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "optional tailcat routes must 404 when the plugin is absent"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&body).contains("tailcat"),
            "disabled routes must not disclose the integration"
        );

        let enabled = build_router_inner(state, true).unwrap();
        let response = enabled.oneshot(status_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_per_plugin_labels() {
        let plugin_metrics = std::sync::RwLock::new(HashMap::from([(
            "notes".to_string(),
            Arc::new(plugin_metrics::PluginMetrics::default()),
        )]));
        let state = test_app_state(plugin_metrics);
        record_plugin_metric(&state, "notes", true, 12, 400);
        record_plugin_metric(&state, "notes", false, 3, 100);
        record_plugin_metric(&state, "not-loaded", true, 1, 1);

        let body = metrics(State(state)).await;
        assert!(body.contains("owasidea_requests_total 5"));
        assert!(body.contains("owasidea_errors_total 2"));
        assert!(body.contains("owasidea_plugin_requests_total{plugin=\"notes\"} 2"));
        assert!(body.contains("owasidea_plugin_errors_total{plugin=\"notes\"} 1"));
        assert!(body.contains("owasidea_plugin_latency_ms_sum{plugin=\"notes\"} 15"));
        assert!(body.contains("owasidea_plugin_latency_ms_count{plugin=\"notes\"} 2"));
        assert!(body.contains("owasidea_plugin_fuel_total{plugin=\"notes\"} 500"));
        assert!(!body.contains("not-loaded"));
    }
}
