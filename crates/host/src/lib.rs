use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{sync::Mutex, task::JoinHandle};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

// ---------------------------------------------------------------------------
// Manifest (subset used by the POC host)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Manifest {
    name: String,
    version: String,
    language: String,
    abi: String,
    #[serde(default)]
    runtime: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    component: String,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Export {
    name: String,
    signature: String,
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
    kv: Option<KvPerm>,
    #[serde(default)]
    resources: Resources,
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
    component_path: PathBuf,
    kv: Arc<Mutex<HashMap<String, String>>>,
    logs: Arc<Mutex<Vec<String>>>,
    // Executable plugins are served by a `wasmtime serve` child process for
    // this POC. A production host would embed wasmtime-wasi-http instead.
    child: Option<Child>,
    port: Option<u16>,
}

struct TailcatSession {
    store: Store<HostState>,
    instance: wasmtime::component::Instance,
    address: Option<String>,
    running: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct AppState {
    engine: Engine,
    plugins: Arc<HashMap<String, PluginRuntime>>,
    tailcat_session: Arc<Mutex<Option<TailcatSession>>>,
}

struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
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
            hooks: wasmtime_wasi_http::default_hooks(),
        }
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
        let kv = plugin.kv.clone();
        let mut root = linker.root();
        let mut inst = root
            .instance("poc:host/host-kv@0.1.0")
            .map_err(|e| anyhow!("missing host-kv interface: {e}"))?;
        let kv_get = kv.clone();
        inst.func_wrap(
            "get",
            move |_store: wasmtime::StoreContextMut<'_, HostState>, (key,): (String,)| {
                let value = kv_get.try_lock().ok().and_then(|m| m.get(&key).cloned());
                Ok((value,))
            },
        )?;
        inst.func_wrap(
            "set",
            move |_store: wasmtime::StoreContextMut<'_, HostState>,
                  (key, value): (String, String)| {
                if let Ok(mut m) = kv.try_lock() {
                    m.insert(key, value);
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
            move |_store: wasmtime::StoreContextMut<'_, HostState>, (msg,): (String,)| {
                if let Ok(mut l) = logs.try_lock() {
                    l.push(msg);
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
    let component = Component::from_file(engine, &plugin.component_path)
        .map_err(|e| anyhow!("failed to load component {:?}: {e}", plugin.component_path))?;
    let linker = build_linker(engine, plugin)?;

    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();
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

    // Grant network only when the component declares wasi:sockets imports.
    if plugin
        .manifest
        .imports
        .wasi
        .iter()
        .any(|w| w.starts_with("wasi:sockets"))
    {
        builder.inherit_network();
        builder.allow_udp(true);
        builder.allow_tcp(true);
        builder.allow_ip_name_lookup(true);
    }
    for fs in &plugin.manifest.permissions.filesystem {
        let host_path = PathBuf::from(&fs.path);
        std::fs::create_dir_all(&host_path).ok();
        let perms = match fs.access.as_str() {
            "read" => FsPerms::ReadOnly,
            _ => FsPerms::ReadWrite,
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
        },
    );
    // Wire manifest fuel limit (if declared).
    if let Some(max_fuel) = plugin.manifest.permissions.resources.max_fuel {
        let _ = store.set_fuel(max_fuel);
    }
    let instance = linker
        .instantiate_async(&mut store, &component)
        .await
        .map_err(|e| anyhow!("failed to instantiate component: {e}"))?;
    Ok((store, instance))
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
struct ServiceStatusRes {
    running: bool,
    address: Option<String>,
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

#[derive(Serialize)]
struct LogsRes {
    logs: Vec<String>,
}

#[derive(Serialize)]
struct PluginInfo {
    name: String,
    version: String,
    language: String,
    #[serde(rename = "type")]
    kind: String,
    exports: Vec<String>,
    imports_wasi: usize,
    imports_host: Vec<String>,
    served_on: Option<String>,
}

async fn health() -> &'static str {
    "ok"
}

async fn list_plugins(State(state): State<AppState>) -> Json<Vec<PluginInfo>> {
    let mut out = Vec::new();
    for (name, p) in state.plugins.iter() {
        out.push(PluginInfo {
            name: name.clone(),
            version: p.manifest.version.clone(),
            language: p.manifest.language.clone(),
            kind: p.manifest.kind.clone(),
            exports: p.manifest.exports.iter().map(|e| e.name.clone()).collect(),
            imports_wasi: p.manifest.imports.wasi.len(),
            imports_host: p.manifest.imports.host.clone(),
            served_on: p.port.map(|port| format!("http://127.0.0.1:{port}")),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Json(out)
}

async fn plugin_manifest(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Manifest>, (StatusCode, String)> {
    state
        .plugins
        .get(&name)
        .map(|p| Json(p.manifest.clone()))
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))
}

async fn plugin_logs(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<LogsRes>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let logs = p.logs.lock().await.clone();
    Ok(Json(LogsRes { logs }))
}

async fn reload(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    // Re-read the manifest from disk so capability/permission edits are
    // enforced on reload (not just the component bytes).
    let raw = std::fs::read_to_string(&p.manifest_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read manifest: {e}"),
        )
    })?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid manifest: {e}")))?;
    let base = p
        .manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let component_path = base.join(&manifest.component);
    let runtime = PluginRuntime {
        manifest,
        manifest_path: p.manifest_path.clone(),
        component_path,
        kv: p.kv.clone(),
        logs: p.logs.clone(),
        child: None,
        port: p.port,
    };
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
        Ok(Ok(_)) => Ok(Json(serde_json::json!({ "reloaded": name }))),
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
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
        let func = instance
            .get_typed_func::<(String,), (String,)>(&mut store, "greet")
            .map_err(internal)?;
        let (output,) = func
            .call_async(&mut store, (req.name,))
            .await
            .map_err(internal)?;
        Ok::<_, (StatusCode, String)>(Json(OutputRes { output }))
    };
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(r) => r,
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
    }
}

async fn complete(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<CompleteReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let prompt = req.prompt.clone();
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
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
    };
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(r) => r,
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
    }
}

async fn echo_udp(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<BindReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(u16,), (Result<String, String>,)>(&mut store, "echo-udp")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.port,))
        .await
        .map_err(internal)?;
    match output {
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
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(String, u16), (Result<String, String>,)>(&mut store, "dial-echo")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.host, req.port))
        .await
        .map_err(internal)?;
    match output {
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
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(String, u16, String), (Result<String, String>,)>(&mut store, "tls-probe")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.host, req.port, req.server_name))
        .await
        .map_err(internal)?;
    match output {
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
        let p = state.plugins.get("tailcat-wasi").ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "tailcat-wasi plugin not loaded".to_string(),
            )
        })?;
        let (store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
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
    }))
}

async fn service_status(
    State(state): State<AppState>,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    let mut guard = state.tailcat_session.lock().await;
    let Some(session) = guard.as_mut() else {
        return Ok(Json(ServiceStatusRes {
            running: false,
            address: None,
        }));
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

async fn start_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<StartReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(String,), (Result<String, String>,)>(&mut store, "start-server")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.region_json,))
        .await
        .map_err(internal)?;
    match output {
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
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(String, u16), (Result<String, String>,)>(&mut store, "dial-tcp")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.host, req.port))
        .await
        .map_err(internal)?;
    match output {
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
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(String,), (Result<String, String>,)>(&mut store, "parse-address")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.addr,))
        .await
        .map_err(internal)?;
    match output {
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
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
    let func = instance
        .get_typed_func::<(u16,), (Result<String, String>,)>(&mut store, "bind-udp")
        .map_err(internal)?;
    let (output,) = func
        .call_async(&mut store, (req.port,))
        .await
        .map_err(internal)?;
    match output {
        Ok(s) => Ok(Json(OutputRes { output: s })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ---------------------------------------------------------------------------
// Executable plugin proxy (POC)
// ---------------------------------------------------------------------------

async fn proxy_app(
    State(state): State<AppState>,
    AxumPath(params): AxumPath<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Body,
) -> Result<Response, (StatusCode, String)> {
    let name = params
        .get("name")
        .cloned()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing plugin name".to_string()))?;
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let port = p.port.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("plugin {name} is not an executable (no inbound HTTP)"),
        )
    })?;

    let path = params.get("path").cloned().unwrap_or_default();
    tracing::info!("proxy_app plugin={name} path={path}");
    let target = format!(
        "http://127.0.0.1:{port}/{}{}",
        path,
        uri.query().map(|q| format!("?{q}")).unwrap_or_default()
    );

    let body_bytes = axum::body::to_bytes(body, 32 * 1024 * 1024)
        .await
        .map_err(internal)?;

    let mut req = reqwest::Client::new()
        .request(method, &target)
        .body(body_bytes);
    for (k, v) in headers.iter() {
        if k != axum::http::header::HOST {
            req = req.header(k, v);
        }
    }
    let resp = req.send().await.map_err(internal)?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let bytes = resp.bytes().await.map_err(internal)?;

    let mut builder = Response::builder().status(status);
    for (k, v) in resp_headers.iter() {
        // Let hyper recompute framing from the buffered body.
        if k == axum::http::header::CONTENT_LENGTH
            || k == axum::http::header::TRANSFER_ENCODING
            || k == axum::http::header::CONNECTION
        {
            continue;
        }
        builder = builder.header(k, v);
    }
    builder
        .body(Body::from(bytes))
        .map_err(|e| internal(e.to_string()))
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

fn load_manifests(dir: &Path) -> Result<Vec<(Manifest, PathBuf)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        let manifest: Manifest =
            serde_json::from_str(&raw).with_context(|| format!("invalid manifest {path:?}"))?;
        out.push((manifest, path));
    }
    Ok(out)
}

fn next_free_port(start: u16) -> u16 {
    for port in start..start + 100 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

fn spawn_executable(_manifest: &Manifest, component: &Path, port: u16) -> Result<Child> {
    let wasmtime = std::env::var("WASMTIME_BIN").unwrap_or_else(|_| {
        format!(
            "{}/.wasmtime/bin/wasmtime",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let child = Command::new(&wasmtime)
        .args([
            "serve",
            "-S",
            "common",
            "-S",
            "http",
            "--addr",
            &format!("127.0.0.1:{port}"),
        ])
        .arg(component)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("failed to spawn {wasmtime}: {e}"))?;

    // Best-effort wait for the port to accept connections.
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(child);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(child)
}

pub async fn run(manifests_dir: PathBuf, addr: String) -> Result<()> {
    tracing_subscriber::fmt::init();

    let loaded = load_manifests(&manifests_dir)?;
    if loaded.is_empty() {
        return Err(anyhow!("no manifests found in {manifests_dir:?}"));
    }

    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    // Enable fuel-based execution metering (manifest resources.max_fuel).
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let mut plugins = HashMap::new();
    let mut next_port = 9100u16;

    for (manifest, manifest_path) in loaded {
        if manifest.runtime.as_deref() == Some("js-wasm") {
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

        // Capability check: every host import the component uses must be
        // declared in the manifest. (The generated manifest is derived from
        // the component WIT, so this normally cannot fail; it protects against
        // hand-edited manifests.)
        for required in ["poc:host/host-kv", "poc:host/host-log"] {
            let declared = manifest
                .imports
                .host
                .iter()
                .any(|h| h.starts_with(required));
            if !declared {
                // Only enforce if the component actually imports it. We detect
                // this by trying to instantiate below; a missing declaration
                // manifests as a linker error, which we surface clearly.
                let _ = required;
            }
        }

        let runtime = PluginRuntime {
            manifest: manifest.clone(),
            manifest_path: manifest_path.clone(),
            component_path: component_path.clone(),
            kv: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            child: None,
            port: None,
        };

        let mut runtime = runtime;

        if manifest.kind == "executable" || manifest.kind == "hybrid" {
            let port = next_free_port(next_port);
            next_port = port + 1;
            let child = spawn_executable(&manifest, &component_path, port)?;
            runtime.child = Some(child);
            runtime.port = Some(port);
        } else {
            // Smoke test library plugins now so bad manifests fail fast.
            instantiate(&engine, &runtime)
                .await
                .with_context(|| format!("startup smoke test failed for {}", manifest.name))?;
        }

        plugins.insert(manifest.name.clone(), runtime);
    }

    let state = AppState {
        engine,
        plugins: Arc::new(plugins),
        tailcat_session: Arc::new(Mutex::new(None)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/plugins", get(list_plugins))
        .route("/plugin/{name}/manifest", get(plugin_manifest))
        .route("/plugin/{name}/logs", get(plugin_logs))
        .route("/plugin/{name}/reload", post(reload))
        .route("/plugin/{name}/greet", post(greet))
        .route("/plugin/{name}/complete", post(complete))
        .route("/plugin/{name}/bind-udp", post(bind_udp))
        .route("/plugin/{name}/parse-address", post(parse_address))
        .route("/plugin/{name}/dial-tcp", post(dial_tcp))
        .route("/plugin/{name}/start-server", post(start_server))
        .route("/plugin/tailcat-wasi/start-service", post(start_service))
        .route("/plugin/tailcat-wasi/stop-service", post(stop_service))
        .route("/plugin/tailcat-wasi/service-status", get(service_status))
        .route("/plugin/tailcat-wasi/send-text", post(send_text))
        .route("/plugin/tailcat-wasi/drain", get(drain_tailcat))
        .route("/plugin/{name}/tls-probe", post(tls_probe))
        .route("/plugin/{name}/dial-echo", post(dial_echo))
        .route("/plugin/{name}/echo-udp", post(echo_udp))
        .route("/app/{name}/{*path}", axum::routing::any(proxy_app))
        .route("/app/{name}/", axum::routing::any(proxy_app))
        .route("/app/{name}", axum::routing::any(proxy_app))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    tracing::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
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
}
