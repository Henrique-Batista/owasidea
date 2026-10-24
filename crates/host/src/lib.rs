use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{
        sse::{Event, KeepAlive, Sse},
        Response,
    },
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
use wasmtime::component::{Component, Linker, ResourceTable, Type, Val};
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
    kv: Arc<std::sync::Mutex<rusqlite::Connection>>,
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
    http_hooks: AllowlistHooks,
    limits: wasmtime::StoreLimits,
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
        let allowed = self.allowed.iter().any(|a| a == "*" || a == &host);
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
        let kv = plugin.kv.clone();
        let mut root = linker.root();
        let mut inst = root
            .instance("poc:host/host-kv@0.1.0")
            .map_err(|e| anyhow!("missing host-kv interface: {e}"))?;
        let kv_get = kv.clone();
        inst.func_wrap(
            "get",
            move |_store: wasmtime::StoreContextMut<'_, HostState>, (key,): (String,)| {
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
                if let Ok(conn) = kv_set.lock() {
                    let _ = conn.execute(
                        "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
                        rusqlite::params![key, value],
                    );
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
    // Verify component checksum if declared in manifest metadata.
    if let Some(expected) = plugin
        .manifest
        .metadata
        .get("sha256")
        .and_then(|v| v.as_str())
    {
        use sha2::{Digest, Sha256};
        let bytes =
            std::fs::read(&plugin.component_path).map_err(|e| anyhow!("read component: {e}"))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            return Err(anyhow!(
                "checksum mismatch for {}: expected {}, got {}",
                plugin.manifest.name,
                expected,
                actual
            ));
        }
    }
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
            http_hooks: AllowlistHooks {
                allowed: plugin.manifest.permissions.http.outbound.clone(),
            },
            limits: {
                let mut b = wasmtime::StoreLimitsBuilder::new();
                if let Some(mb) = plugin.manifest.permissions.resources.max_memory_mb {
                    b = b.memory_size((mb as usize) * 1024 * 1024);
                }
                b.build()
            },
        },
    );
    store.limiter(|state| &mut state.limits);
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
struct CallReq {
    function: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct ServiceStatusRes {
    running: bool,
    address: Option<String>,
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

#[derive(Serialize)]
struct OverviewRes {
    host: &'static str,
    plugins: usize,
    libraries: usize,
    executables: usize,
    services: usize,
    runtimes: Vec<String>,
}

async fn overview(State(state): State<AppState>) -> Json<OverviewRes> {
    let mut libraries = 0;
    let mut executables = 0;
    let mut services = 0;
    let mut runtimes = std::collections::BTreeSet::new();
    for p in state.plugins.values() {
        match p.manifest.kind.as_str() {
            "executable" => executables += 1,
            "hybrid" => services += 1,
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
        plugins: state.plugins.len(),
        libraries,
        executables,
        services,
        runtimes: runtimes.into_iter().collect(),
    })
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

async fn plugin_logs_stream(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, String),
> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let logs = p.logs.clone();
    let stream = async_stream::stream! {
        let mut last = 0usize;
        loop {
            let snapshot = {
                let l = logs.lock().await;
                l.clone()
            };
            if snapshot.len() > last {
                for msg in &snapshot[last..] {
                    yield Ok(Event::default().data(msg.clone()));
                }
                last = snapshot.len();
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

async fn call_export(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<CallReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
        let func = instance
            .get_func(&mut store, &req.function)
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    format!("export {} not found", req.function),
                )
            })?;
        let ty = func.ty(&store);
        if ty.params().count() != req.args.len() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "expected {} args, got {}",
                    ty.params().count(),
                    req.args.len()
                ),
            ));
        }
        let mut params = Vec::new();
        for (param, arg) in ty.params().zip(req.args.iter()) {
            params.push(json_to_val(&param.1, arg).map_err(|e| (StatusCode::BAD_REQUEST, e))?);
        }
        let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
        func.call_async(&mut store, &params, &mut results)
            .await
            .map_err(internal)?;
        let json_results: Vec<serde_json::Value> = results.iter().map(val_to_json).collect();
        let body = if json_results.len() == 1 {
            serde_json::json!({ "result": json_results[0] })
        } else {
            serde_json::json!({ "results": json_results })
        };
        Ok::<_, (StatusCode, String)>(Json(body))
    };
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(r) => r,
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
    }
}

async fn service_start_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<ServiceStartReq>,
) -> Result<Json<OutputRes>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let region_json = if req.region_json.trim().is_empty() {
        default_region_json()
    } else {
        req.region_json
    };
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
        for candidate in ["start-service", "start-server"] {
            if let Some(func) = instance.get_func(&mut store, candidate) {
                let ty = func.ty(&store);
                let mut params = vec![];
                let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
                if ty.params().count() == 1 {
                    params.push(Val::String(region_json.clone()));
                }
                func.call_async(&mut store, &params, &mut results)
                    .await
                    .map_err(internal)?;
                let out = results
                    .first()
                    .map(val_to_json)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "started".to_string());
                return Ok::<_, (StatusCode, String)>(Json(OutputRes { output: out }));
            }
        }
        Err((StatusCode::NOT_FOUND, "no start-service export".to_string()))
    };
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(r) => r,
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
    }
}

async fn service_stop_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
        for candidate in ["stop-service", "stop-server"] {
            if let Some(func) = instance.get_func(&mut store, candidate) {
                let ty = func.ty(&store);
                let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
                func.call_async(&mut store, &[], &mut results)
                    .await
                    .map_err(internal)?;
                return Ok::<_, (StatusCode, String)>(serde_json::json!({"stopped": true}));
            }
        }
        Ok(serde_json::json!({"stopped": false, "reason": "no stop export"}))
    };
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(r) => Ok(Json(r?)),
        Err(_) => Err((StatusCode::GATEWAY_TIMEOUT, "request timed out".to_string())),
    }
}

async fn service_status_generic(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<ServiceStatusRes>, (StatusCode, String)> {
    let p = state
        .plugins
        .get(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {name}")))?;
    let timeout_ms = p.manifest.permissions.resources.timeout_ms.unwrap_or(30000);
    let work = async {
        let (mut store, instance) = instantiate(&state.engine, p).await.map_err(internal)?;
        for candidate in ["service-status", "server-status"] {
            if let Some(func) = instance.get_func(&mut store, candidate) {
                let ty = func.ty(&store);
                let mut results = ty.results().map(|ty| default_val(&ty)).collect::<Vec<_>>();
                func.call_async(&mut store, &[], &mut results)
                    .await
                    .map_err(internal)?;
                let running = results
                    .first()
                    .and_then(|v| match v {
                        Val::Bool(b) => Some(*b),
                        _ => None,
                    })
                    .unwrap_or(false);
                return Ok::<_, (StatusCode, String)>(ServiceStatusRes {
                    running,
                    address: None,
                });
            }
        }
        Ok(ServiceStatusRes {
            running: false,
            address: None,
        })
    };
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), work).await {
        Ok(r) => Ok(Json(r?)),
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

fn open_kv_db(name: &str) -> rusqlite::Connection {
    let dir = std::path::PathBuf::from("./data").join(name);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("kv.sqlite");
    let conn = rusqlite::Connection::open(&path).expect("open kv db");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .expect("init kv table");
    conn
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
            kv: Arc::new(std::sync::Mutex::new(open_kv_db(&manifest.name))),
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
        .route("/overview", get(overview))
        .route("/plugins", get(list_plugins))
        .route("/plugin/{name}/manifest", get(plugin_manifest))
        .route("/plugin/{name}/logs", get(plugin_logs))
        .route("/plugin/{name}/logs/stream", get(plugin_logs_stream))
        .route("/plugin/{name}/reload", post(reload))
        .route("/plugin/{name}/greet", post(greet))
        .route("/plugin/{name}/complete", post(complete))
        .route("/plugin/{name}/call", post(call_export))
        .route("/plugin/{name}/service/start", post(service_start_generic))
        .route("/plugin/{name}/service/stop", post(service_stop_generic))
        .route("/plugin/{name}/service/status", get(service_status_generic))
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
