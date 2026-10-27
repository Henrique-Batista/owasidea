//! Experimental long-call jobs (RL-06, ADR-0005).
//!
//! This module is compiled only with the `streaming` feature. It implements the
//! minimal prototype from ADR-0005: asynchronous jobs, an SSE event stream, and
//! cooperative cancellation. `/plugin/{name}/call` is untouched.
//!
//! The prototype deliberately keeps the execution contract small:
//!
//! - the host owns job lifecycle, limits, and the event ring buffer;
//! - an executor runs one call and may emit progress events;
//! - cancellation drops the executor future (Wasmtime has no guest preemption
//!   here) and marks the job `cancelled`;
//! - guest-driven progress through a WIT import is specified by the ADR but not
//!   wired in this prototype.

use super::{
    invoke_export, plugin_metrics, PluginRuntime, DEFAULT_CALL_TIMEOUT_MS, MAX_CALL_TIMEOUT_MS,
};
use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, Notify};
use wasmtime::Engine;

/// Maximum number of jobs running or queued at once.
pub(crate) const MAX_ACTIVE_JOBS: usize = 8;
/// Maximum number of finished jobs kept for later status/SSE reads.
pub(crate) const MAX_RETAINED_JOBS: usize = 64;
/// Maximum events kept per job (oldest events are dropped first).
pub(crate) const MAX_EVENTS_PER_JOB: usize = 256;
/// Maximum serialized `args` payload accepted by a job.
pub(crate) const MAX_ARGS_BYTES: usize = 64 * 1024;
/// Maximum serialized size of one event's `data`; larger payloads are replaced.
pub(crate) const MAX_EVENT_BYTES: usize = 8 * 1024;
/// How often the host emits a heartbeat progress event while a call runs.
pub(crate) const PROGRESS_HEARTBEAT_MS: u64 = 500;

pub(crate) type JobRunFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct JobEvent {
    pub seq: u64,
    pub ts: u64,
    pub event: String,
    pub data: Value,
}

pub(crate) struct JobRecord {
    pub id: String,
    pub plugin: String,
    pub function: String,
    pub state: JobState,
    pub created_unix_ms: u64,
    pub started_unix_ms: Option<u64>,
    pub finished_unix_ms: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub events: VecDeque<JobEvent>,
    next_seq: u64,
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
}

impl JobRecord {
    fn new(id: String, plugin: String, function: String) -> Self {
        Self {
            id,
            plugin,
            function,
            state: JobState::Queued,
            created_unix_ms: now_ms(),
            started_unix_ms: None,
            finished_unix_ms: None,
            result: None,
            error: None,
            events: VecDeque::new(),
            next_seq: 1,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct JobView {
    pub id: String,
    pub plugin: String,
    pub function: String,
    pub state: JobState,
    pub created_unix_ms: u64,
    pub started_unix_ms: Option<u64>,
    pub finished_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub events: Vec<JobEvent>,
}

impl JobView {
    fn from_record(record: &JobRecord) -> Self {
        Self {
            id: record.id.clone(),
            plugin: record.plugin.clone(),
            function: record.function.clone(),
            state: record.state,
            created_unix_ms: record.created_unix_ms,
            started_unix_ms: record.started_unix_ms,
            finished_unix_ms: record.finished_unix_ms,
            result: record.result.clone(),
            error: record.error.clone(),
            events: record.events.iter().cloned().collect(),
        }
    }
}

/// A lightweight handle passed to executors. It lets a long-running call emit
/// progress and observe cancellation without owning the job store.
#[derive(Clone)]
pub(crate) struct JobHandle {
    job: Arc<Mutex<JobRecord>>,
    cancel: Arc<AtomicBool>,
}

impl JobHandle {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Append a progress event. Ignored once the job reached a terminal state.
    pub(crate) async fn emit(&self, event: &str, data: Value) {
        let mut record = self.job.lock().await;
        if record.state.is_terminal() {
            return;
        }
        push_event(&mut record, event, data);
    }
}

/// Execution backend for jobs. The prototype ships one real executor
/// (`HostJobExecutor`) and uses fake executors in tests.
pub(crate) trait JobExecutor: Send + Sync {
    /// Validate the request before a job is accepted and return the effective
    /// timeout in milliseconds.
    fn validate(&self, plugin: &str, function: &str) -> Result<u64, (StatusCode, String)>;

    /// Run the call. The returned future is dropped when the job is cancelled
    /// or times out.
    fn run(
        &self,
        plugin: String,
        function: String,
        args: Vec<Value>,
        job: JobHandle,
    ) -> JobRunFuture;
}

#[derive(Default)]
struct JobStore {
    jobs: HashMap<String, Arc<Mutex<JobRecord>>>,
    order: VecDeque<String>,
}

#[derive(Clone)]
pub(crate) struct JobManager {
    store: Arc<Mutex<JobStore>>,
    executor: Arc<dyn JobExecutor>,
    active: Arc<AtomicUsize>,
}

impl JobManager {
    pub(crate) fn new(executor: Arc<dyn JobExecutor>) -> Self {
        Self {
            store: Arc::new(Mutex::new(JobStore::default())),
            executor,
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Accept a job and spawn its execution. Returns the job descriptor.
    pub(crate) async fn create(
        &self,
        plugin: &str,
        function: &str,
        args: Vec<Value>,
    ) -> Result<Value, (StatusCode, String)> {
        if function.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "function must not be empty".to_string(),
            ));
        }
        let args_len = serde_json::to_vec(&args)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid args: {e}")))?
            .len();
        if args_len > MAX_ARGS_BYTES {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("args exceed {MAX_ARGS_BYTES} bytes"),
            ));
        }
        let timeout_ms = self.executor.validate(plugin, function)?;
        self.reserve_slot()?;

        let id = new_job_id();
        let record = Arc::new(Mutex::new(JobRecord::new(
            id.clone(),
            plugin.to_string(),
            function.to_string(),
        )));
        {
            let mut record_guard = record.lock().await;
            push_event(
                &mut record_guard,
                "queued",
                json!({ "plugin": plugin, "function": function }),
            );
        }

        {
            let mut store = self.store.lock().await;
            prune_store(&mut store);
            store.jobs.insert(id.clone(), record.clone());
            store.order.push_back(id.clone());
        }

        tokio::spawn(run_job(
            self.executor.clone(),
            record,
            self.active.clone(),
            plugin.to_string(),
            function.to_string(),
            args,
            timeout_ms,
        ));

        Ok(json!({
            "job_id": id,
            "state": JobState::Queued,
            "status_url": format!("/jobs/{id}"),
            "events_url": format!("/jobs/{id}/events"),
            "cancel_url": format!("/jobs/{id}/cancel"),
        }))
    }

    fn reserve_slot(&self) -> Result<(), (StatusCode, String)> {
        let mut current = self.active.load(Ordering::SeqCst);
        loop {
            if current >= MAX_ACTIVE_JOBS {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("too many active jobs (limit {MAX_ACTIVE_JOBS})"),
                ));
            }
            match self.active.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) async fn view(&self, id: &str) -> Option<JobView> {
        let record = self.record(id).await?;
        let guard = record.lock().await;
        Some(JobView::from_record(&guard))
    }

    pub(crate) async fn record(&self, id: &str) -> Option<Arc<Mutex<JobRecord>>> {
        let store = self.store.lock().await;
        store.jobs.get(id).cloned()
    }

    /// Cancel a job. Idempotent: cancelling a terminal job returns its current
    /// state without changing it.
    pub(crate) async fn cancel(&self, id: &str) -> Result<JobView, (StatusCode, String)> {
        let record = self
            .record(id)
            .await
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown job {id}")))?;
        let mut guard = record.lock().await;
        if !guard.state.is_terminal() {
            guard.cancel.store(true, Ordering::SeqCst);
            guard.state = JobState::Cancelled;
            guard.error = Some("cancelled".to_string());
            guard.finished_unix_ms = Some(now_ms());
            push_event(&mut guard, "finished", json!({ "state": "cancelled" }));
            guard.cancel_notify.notify_one();
        }
        Ok(JobView::from_record(&guard))
    }
}

fn prune_store(store: &mut JobStore) {
    while store.order.len() >= MAX_RETAINED_JOBS {
        let removable = store.order.iter().find_map(|id| {
            store
                .jobs
                .get(id)
                .and_then(|record| record.try_lock().ok())
                .filter(|guard| guard.state.is_terminal())
                .map(|_| id.clone())
        });
        match removable {
            Some(id) => {
                store.order.retain(|candidate| candidate != &id);
                store.jobs.remove(&id);
            }
            None => break,
        }
    }
}

async fn run_job(
    executor: Arc<dyn JobExecutor>,
    record: Arc<Mutex<JobRecord>>,
    active: Arc<AtomicUsize>,
    plugin: String,
    function: String,
    args: Vec<Value>,
    timeout_ms: u64,
) {
    let cancel = {
        let mut guard = record.lock().await;
        guard.state = JobState::Running;
        guard.started_unix_ms = Some(now_ms());
        push_event(
            &mut guard,
            "started",
            json!({ "plugin": plugin, "function": function }),
        );
        guard.cancel.clone()
    };
    let handle = JobHandle {
        job: record.clone(),
        cancel: cancel.clone(),
    };

    let heartbeat = {
        let record = record.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(PROGRESS_HEARTBEAT_MS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let mut guard = record.lock().await;
                if guard.state.is_terminal() {
                    break;
                }
                let elapsed = now_ms().saturating_sub(guard.started_unix_ms.unwrap_or_else(now_ms));
                push_event(&mut guard, "progress", json!({ "elapsed_ms": elapsed }));
            }
        })
    };

    let outcome = tokio::select! {
        result = executor.run(plugin.clone(), function.clone(), args, handle) => Outcome::Done(result),
        _ = wait_cancelled(record.clone()) => Outcome::Cancelled,
        _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => Outcome::Timeout,
    };
    heartbeat.abort();

    {
        let mut guard = record.lock().await;
        if !guard.state.is_terminal() {
            match outcome {
                Outcome::Done(Ok(result)) => {
                    guard.result = Some(result);
                    guard.state = JobState::Succeeded;
                    guard.finished_unix_ms = Some(now_ms());
                    push_event(&mut guard, "finished", json!({ "state": "succeeded" }));
                }
                Outcome::Done(Err(error)) => {
                    guard.error = Some(error);
                    guard.state = JobState::Failed;
                    guard.finished_unix_ms = Some(now_ms());
                    push_event(&mut guard, "finished", json!({ "state": "failed" }));
                }
                Outcome::Cancelled => {
                    guard.error = Some("cancelled".to_string());
                    guard.state = JobState::Cancelled;
                    guard.finished_unix_ms = Some(now_ms());
                    push_event(&mut guard, "finished", json!({ "state": "cancelled" }));
                }
                Outcome::Timeout => {
                    guard.error = Some(format!("job timed out after {timeout_ms} ms"));
                    guard.state = JobState::Failed;
                    guard.finished_unix_ms = Some(now_ms());
                    push_event(
                        &mut guard,
                        "finished",
                        json!({ "state": "failed", "reason": "timeout" }),
                    );
                }
            }
        }
    }
    active.fetch_sub(1, Ordering::SeqCst);
}

enum Outcome {
    Done(Result<Value, String>),
    Cancelled,
    Timeout,
}

async fn wait_cancelled(record: Arc<Mutex<JobRecord>>) {
    let notify = {
        let guard = record.lock().await;
        if guard.cancel.load(Ordering::SeqCst) {
            return;
        }
        guard.cancel_notify.clone()
    };
    notify.notified().await;
}

fn push_event(record: &mut JobRecord, event: &str, mut data: Value) {
    if serde_json::to_vec(&data)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
        > MAX_EVENT_BYTES
    {
        data = json!({ "truncated": true });
    }
    let seq = record.next_seq;
    record.next_seq += 1;
    record.events.push_back(JobEvent {
        seq,
        ts: now_ms(),
        event: event.to_string(),
        data,
    });
    while record.events.len() > MAX_EVENTS_PER_JOB {
        record.events.pop_front();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn new_job_id() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    if SystemRandom::new().fill(&mut bytes).is_ok() {
        let mut id = String::with_capacity(32);
        for byte in bytes {
            use std::fmt::Write;
            let _ = write!(id, "{byte:02x}");
        }
        return id;
    }
    static FALLBACK: AtomicU64 = AtomicU64::new(0);
    format!(
        "job-{}-{}",
        now_ms(),
        FALLBACK.fetch_add(1, Ordering::SeqCst)
    )
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct CreateJobReq {
    function: String,
    #[serde(default)]
    args: Vec<Value>,
}

pub(crate) async fn create_job(
    State(state): State<super::AppState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<CreateJobReq>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let body = state.jobs.create(&name, &req.function, req.args).await?;
    Ok((StatusCode::ACCEPTED, Json(body)))
}

pub(crate) async fn get_job(
    State(state): State<super::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<JobView>, (StatusCode, String)> {
    state
        .jobs
        .view(&id)
        .await
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown job {id}")))
}

pub(crate) async fn cancel_job(
    State(state): State<super::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<JobView>, (StatusCode, String)> {
    state.jobs.cancel(&id).await.map(Json)
}

pub(crate) async fn job_events(
    State(state): State<super::AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, String),
> {
    let record = state
        .jobs
        .record(&id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown job {id}")))?;
    let stream = async_stream::stream! {
        let mut last = 0u64;
        loop {
            let (snapshot, terminal) = {
                let guard = record.lock().await;
                (
                    guard
                        .events
                        .iter()
                        .filter(|event| event.seq > last)
                        .cloned()
                        .collect::<Vec<_>>(),
                    guard.state.is_terminal(),
                )
            };
            for event in snapshot {
                last = event.seq;
                let data = serde_json::to_string(&event).unwrap_or_default();
                yield Ok(Event::default().event("job").id(event.seq.to_string()).data(data));
            }
            if terminal {
                let state_name = {
                    let guard = record.lock().await;
                    guard.state
                };
                let data = json!({ "state": state_name }).to_string();
                yield Ok(Event::default().event("end").data(data));
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ---------------------------------------------------------------------------
// Real executor backed by the host runtime
// ---------------------------------------------------------------------------

pub(crate) struct HostJobExecutor {
    engine: Engine,
    plugins: Arc<std::sync::RwLock<HashMap<String, Arc<PluginRuntime>>>>,
    plugin_metrics: Arc<plugin_metrics::PluginMetricsMap>,
    requests: Arc<AtomicU64>,
}

impl HostJobExecutor {
    pub(crate) fn new(
        engine: Engine,
        plugins: Arc<std::sync::RwLock<HashMap<String, Arc<PluginRuntime>>>>,
        plugin_metrics: Arc<plugin_metrics::PluginMetricsMap>,
        requests: Arc<AtomicU64>,
    ) -> Self {
        Self {
            engine,
            plugins,
            plugin_metrics,
            requests,
        }
    }
}

impl JobExecutor for HostJobExecutor {
    fn validate(&self, plugin: &str, function: &str) -> Result<u64, (StatusCode, String)> {
        let runtime = self
            .plugins
            .read()
            .ok()
            .and_then(|plugins| plugins.get(plugin).cloned())
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown plugin {plugin}")))?;
        if !matches!(runtime.manifest.kind.as_str(), "library" | "hybrid") {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "plugin {plugin} is a {} plugin; jobs only run library exports",
                    runtime.manifest.kind
                ),
            ));
        }
        if function.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "function must not be empty".to_string(),
            ));
        }
        let timeout = runtime
            .manifest
            .permissions
            .resources
            .timeout_ms
            .unwrap_or(DEFAULT_CALL_TIMEOUT_MS)
            .clamp(1, MAX_CALL_TIMEOUT_MS);
        Ok(timeout)
    }

    fn run(
        &self,
        plugin: String,
        function: String,
        args: Vec<Value>,
        job: JobHandle,
    ) -> JobRunFuture {
        let runtime = self
            .plugins
            .read()
            .ok()
            .and_then(|plugins| plugins.get(&plugin).cloned());
        let engine = self.engine.clone();
        let metrics = self.plugin_metrics.clone();
        let requests = self.requests.clone();
        Box::pin(async move {
            if job.is_cancelled() {
                return Err("cancelled".to_string());
            }
            job.emit("progress", json!({ "phase": "invoking" })).await;
            let runtime = runtime.ok_or_else(|| format!("unknown plugin {plugin}"))?;
            let request_id = requests.fetch_add(1, Ordering::Relaxed) + 1;
            let started = Instant::now();
            match invoke_export(&engine, &runtime, &function, &args, request_id).await {
                Ok((body, fuel)) => {
                    plugin_metrics::record(
                        &metrics,
                        &plugin,
                        true,
                        started.elapsed().as_millis() as u64,
                        fuel,
                    );
                    Ok(body)
                }
                Err((_, message)) => {
                    plugin_metrics::record(
                        &metrics,
                        &plugin,
                        false,
                        started.elapsed().as_millis() as u64,
                        0,
                    );
                    Err(message)
                }
            }
        })
    }
}

#[cfg(test)]
pub(crate) fn test_manager() -> JobManager {
    JobManager::new(Arc::new(TestExecutor))
}

#[cfg(test)]
struct TestExecutor;

#[cfg(test)]
impl JobExecutor for TestExecutor {
    fn validate(&self, _plugin: &str, _function: &str) -> Result<u64, (StatusCode, String)> {
        Ok(DEFAULT_CALL_TIMEOUT_MS)
    }

    fn run(
        &self,
        _plugin: String,
        _function: String,
        args: Vec<Value>,
        _job: JobHandle,
    ) -> JobRunFuture {
        Box::pin(async move { Ok(json!({ "args": args })) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    enum FakeMode {
        ProgressThenDone,
        WaitCancel,
        Sleep(u64),
        EmitMany(usize),
    }

    struct FakeExecutor {
        mode: FakeMode,
        timeout_ms: u64,
    }

    impl JobExecutor for FakeExecutor {
        fn validate(&self, _plugin: &str, _function: &str) -> Result<u64, (StatusCode, String)> {
            Ok(self.timeout_ms)
        }

        fn run(
            &self,
            _plugin: String,
            _function: String,
            args: Vec<Value>,
            job: JobHandle,
        ) -> JobRunFuture {
            let mode = self.mode.clone();
            Box::pin(async move {
                match mode {
                    FakeMode::ProgressThenDone => {
                        job.emit("progress", json!({ "percent": 25 })).await;
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        job.emit("progress", json!({ "percent": 75 })).await;
                        Ok(json!({ "echo": args }))
                    }
                    FakeMode::WaitCancel => loop {
                        if job.is_cancelled() {
                            return Err("cancelled".to_string());
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    },
                    FakeMode::Sleep(ms) => {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        Ok(json!("late"))
                    }
                    FakeMode::EmitMany(count) => {
                        for index in 0..count {
                            job.emit("progress", json!({ "index": index })).await;
                        }
                        Ok(json!("done"))
                    }
                }
            })
        }
    }

    fn test_state(executor: Arc<dyn JobExecutor>) -> crate::AppState {
        crate::AppState {
            engine: Engine::default(),
            plugins: Arc::new(std::sync::RwLock::new(HashMap::new())),
            ui_plugins: Arc::new(Vec::new()),
            tailcat_session: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(HashMap::new())),
            service_health: Arc::new(Mutex::new(HashMap::new())),
            requests: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
            plugin_metrics: Arc::new(std::sync::RwLock::new(HashMap::new())),
            dev_mode: false,
            dev_hot_reload: false,
            dev_watch_paths: Vec::new(),
            dev_last_reload: Arc::new(std::sync::RwLock::new(std::collections::BTreeMap::new())),
            manifests_dir: std::path::PathBuf::from("."),
            jobs: JobManager::new(executor),
        }
    }

    async fn request(
        router: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let mut builder = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(value) => {
                builder = builder.header("content-type", "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = router
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn create_job(router: &axum::Router, function: &str, args: Value) -> String {
        let (status, body) = request(
            router,
            "POST",
            "/plugin/example/call/jobs",
            Some(json!({ "function": function, "args": args })),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "create failed: {body}");
        let value: Value = serde_json::from_str(&body).unwrap();
        value["job_id"].as_str().unwrap().to_string()
    }

    async fn wait_terminal(router: &axum::Router, id: &str) -> Value {
        for _ in 0..100 {
            let (status, body) = request(router, "GET", &format!("/jobs/{id}"), None).await;
            assert_eq!(status, StatusCode::OK, "get failed: {body}");
            let value: Value = serde_json::from_str(&body).unwrap();
            if matches!(
                value["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {id} did not reach a terminal state");
    }

    #[tokio::test]
    async fn job_create_get_cancel_is_idempotent() {
        let executor = Arc::new(FakeExecutor {
            mode: FakeMode::WaitCancel,
            timeout_ms: 5_000,
        });
        let router = crate::build_router(test_state(executor)).unwrap();

        let id = create_job(&router, "slow-sum", json!([])).await;
        let (status, body) = request(&router, "GET", &format!("/jobs/{id}"), None).await;
        assert_eq!(status, StatusCode::OK);
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["plugin"], "example");
        assert_eq!(value["function"], "slow-sum");
        assert!(matches!(
            value["state"].as_str(),
            Some("queued" | "running")
        ));

        let (status, body) = request(&router, "POST", &format!("/jobs/{id}/cancel"), None).await;
        assert_eq!(status, StatusCode::OK);
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["state"], "cancelled");
        assert_eq!(value["error"], "cancelled");
        assert!(value["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "finished"));

        let (status, body) = request(&router, "POST", &format!("/jobs/{id}/cancel"), None).await;
        assert_eq!(status, StatusCode::OK);
        let again: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(again["state"], "cancelled");

        let (status, _) = request(&router, "GET", "/jobs/does-not-exist", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = request(&router, "POST", "/jobs/does-not-exist/cancel", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn job_events_stream_progress_and_end() {
        let executor = Arc::new(FakeExecutor {
            mode: FakeMode::ProgressThenDone,
            timeout_ms: 5_000,
        });
        let router = crate::build_router(test_state(executor)).unwrap();
        let id = create_job(&router, "slow-sum", json!([10])).await;
        let value = wait_terminal(&router, &id).await;
        assert_eq!(value["state"], "succeeded");
        assert_eq!(value["result"]["echo"][0], 10);

        let (status, body) = tokio::time::timeout(
            Duration::from_secs(2),
            request(&router, "GET", &format!("/jobs/{id}/events"), None),
        )
        .await
        .expect("SSE stream should end after the terminal event");
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("event: job"), "missing job events: {body}");
        assert!(body.contains("\"percent\""), "missing progress: {body}");
        assert!(body.contains("event: end"), "missing end event: {body}");
        assert!(body.contains("succeeded"), "missing terminal state: {body}");
    }

    #[tokio::test]
    async fn job_args_payload_limit_is_enforced() {
        let executor = Arc::new(FakeExecutor {
            mode: FakeMode::ProgressThenDone,
            timeout_ms: 5_000,
        });
        let router = crate::build_router(test_state(executor)).unwrap();
        let oversized = "x".repeat(MAX_ARGS_BYTES + 1);
        let (status, body) = request(
            &router,
            "POST",
            "/plugin/example/call/jobs",
            Some(json!({ "function": "slow-sum", "args": [oversized] })),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body.contains("args exceed"), "unexpected body: {body}");
    }

    #[tokio::test]
    async fn job_concurrency_and_timeout_limits_are_enforced() {
        let executor = Arc::new(FakeExecutor {
            mode: FakeMode::WaitCancel,
            timeout_ms: 5_000,
        });
        let router = crate::build_router(test_state(executor)).unwrap();
        let mut ids = Vec::new();
        for _ in 0..MAX_ACTIVE_JOBS {
            ids.push(create_job(&router, "slow-sum", json!([])).await);
        }
        let (status, body) = request(
            &router,
            "POST",
            "/plugin/example/call/jobs",
            Some(json!({ "function": "slow-sum", "args": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(body.contains("too many active jobs"), "unexpected: {body}");
        for id in ids {
            let (status, _) = request(&router, "POST", &format!("/jobs/{id}/cancel"), None).await;
            assert_eq!(status, StatusCode::OK);
        }

        let timeout_executor = Arc::new(FakeExecutor {
            mode: FakeMode::Sleep(2_000),
            timeout_ms: 50,
        });
        let router = crate::build_router(test_state(timeout_executor)).unwrap();
        let id = create_job(&router, "slow-sum", json!([])).await;
        let value = wait_terminal(&router, &id).await;
        assert_eq!(value["state"], "failed");
        assert!(
            value["error"].as_str().unwrap().contains("timed out"),
            "unexpected error: {value}"
        );
    }

    #[tokio::test]
    async fn job_event_ring_buffer_is_bounded() {
        let executor = Arc::new(FakeExecutor {
            mode: FakeMode::EmitMany(MAX_EVENTS_PER_JOB + 40),
            timeout_ms: 5_000,
        });
        let router = crate::build_router(test_state(executor)).unwrap();
        let id = create_job(&router, "slow-sum", json!([])).await;
        let value = wait_terminal(&router, &id).await;
        assert_eq!(value["state"], "succeeded");
        assert_eq!(
            value["events"].as_array().unwrap().len(),
            MAX_EVENTS_PER_JOB
        );
    }
}
