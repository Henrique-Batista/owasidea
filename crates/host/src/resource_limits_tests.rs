//! Resource-limit coverage (FIX-01).
//!
//! What is exercised here, and how honest each test is:
//!
//! | Test | Kind | Evidence |
//! |---|---|---|
//! | `instance_slots_are_derived_from_manifest` | config-only | `instance_slots()` reads `max_instances`, rejects `0`, defaults to `1`. |
//! | `store_applies_manifest_fuel_limit` | config-only | `create_store()` sets fuel to `max_fuel`. |
//! | `fuel_exhaustion_traps_cpu_bound_export` | end-to-end (router + real Wasmtime component) | guest loops forever; `max_fuel` traps the call. |
//! | `memory_limit_rejects_component_exceeding_max_memory_mb` | end-to-end | component declares a 64-page memory; `max_memory_mb = 1` refuses instantiation, `64` succeeds. |
//! | `max_instances_queues_second_call_until_slot_frees` | end-to-end | one permit held; second call waits and then succeeds (queue, not reject). |
//! | `max_instances_wait_times_out_with_manifest_timeout` | runtime primitive | one permit held; `create_store()` fails after `timeout_ms` waiting for a slot (the HTTP layer surfaces the same wait as `504`). |
//! | `timeout_ms_cancels_yielding_guest_call` | end-to-end | guest blocks in a WASI timer (yields to the executor); `/call` returns 504 after `timeout_ms`. |
//!
//! Components are tiny WAT fixtures compiled by `Component::new`; no downloaded
//! artifact and no build toolchain is required. The HTTP path is the real
//! `build_router` + axum handlers, in-process (no separate host process).
//!
//! Known limitation, not masked by these tests: `timeout_ms` is a
//! `tokio::time::timeout` around the call future, so it can only interrupt a
//! guest that yields (I/O, timer, async import). A purely CPU-bound guest is
//! not preempted by that timeout; `max_fuel` is the backstop that stops it
//! (`fuel_exhaustion_traps_cpu_bound_export`). Epoch interruption is not
//! configured in the engine today.

use super::*;

use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use axum::body::Body;
use tower::ServiceExt;
use wasmtime::component::Component;

/// Infinite loop: only fuel can stop it.
const SPIN_WAT: &str = r#"
(component
  (core module $m
    (func (export "spin") (result i32)
      (local $n i32)
      (loop $l
        local.get $n
        i32.const 1
        i32.add
        local.set $n
        br $l)
      local.get $n))
  (core instance $i (instantiate $m))
  (func (export "spin") (result u32) (canon lift (core func $i "spin")))
)
"#;

/// Returns 42 without touching memory: used for concurrency/queue tests.
const ECHO_WAT: &str = r#"
(component
  (core module $m
    (func (export "value") (result i32) i32.const 42))
  (core instance $i (instantiate $m))
  (func (export "value") (result u32) (canon lift (core func $i "value")))
)
"#;

/// Declares a 64-page (4 MiB) linear memory at instantiation time.
const MEMORY_WAT: &str = r#"
(component
  (core module $m
    (memory (export "mem") 64)
    (func (export "value") (result i32) i32.const 7))
  (core instance $i (instantiate $m))
  (func (export "value") (result u32) (canon lift (core func $i "value")))
)
"#;

/// Blocks in a WASI monotonic-clock timer, which yields to the Tokio executor.
const SLEEPY_WAT: &str = r#"
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
    (func (export "sleepy") (result i32)
      i64.const 2000000000
      call $subscribe_core
      call $block_core
      i32.const 7))
  (core instance $i (instantiate $m
    (with "" (instance
      (export "subscribe" (func $subscribe_lowered))
      (export "block" (func $block_lowered))))))
  (func (export "sleepy") (result u32) (canon lift (core func $i "sleepy")))
)
"#;

fn test_engine() -> Engine {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    Engine::new(&config).expect("engine")
}

fn test_manifest(name: &str, resources: serde_json::Value) -> Manifest {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "language": "rust",
        "abi": "wasi-0.2",
        "runtime": "wasi-component",
        "type": "library",
        "component": "test.wasm",
        "exports": [{"name": "value", "signature": "func() -> u32"}],
        "imports": {"wasi": [], "host": [], "plugins": []},
        "permissions": {"resources": resources},
        "state": {},
        "composition": {},
        "metadata": {}
    }))
    .expect("test manifest")
}

fn test_plugin(
    engine: &Engine,
    name: &str,
    wat: &str,
    resources: serde_json::Value,
) -> Arc<PluginRuntime> {
    let manifest = test_manifest(name, resources);
    Arc::new(PluginRuntime {
        instance_slots: instance_slots(&manifest).expect("instance slots"),
        manifest,
        manifest_path: PathBuf::from("test.manifest.json"),
        component: Component::new(engine, wat).expect("compile WAT component"),
        kv: Arc::new(std::sync::Mutex::new(
            rusqlite::Connection::open_in_memory().expect("in-memory kv"),
        )),
        logs: Arc::new(Mutex::new(LogBuffer::new(name))),
        http_pre: None,
    })
}

fn test_state(engine: Engine, plugin: Arc<PluginRuntime>) -> AppState {
    let name = plugin.manifest.name.clone();
    AppState {
        engine,
        plugins: Arc::new(std::sync::RwLock::new(HashMap::from([(name, plugin)]))),
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
        manifests_dir: PathBuf::from("."),
        #[cfg(feature = "streaming")]
        jobs: jobs::test_manager(),
    }
}

async fn call(router: &Router, plugin: &str, function: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/plugin/{plugin}/call"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"function": function, "args": []}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("router response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// ---------------------------------------------------------------------------
// Config-only coverage: the manifest values reach the runtime primitives.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn instance_slots_are_derived_from_manifest() {
    // Unset -> one instance (the historical default).
    let slots = instance_slots(&test_manifest("p", serde_json::json!({}))).unwrap();
    assert_eq!(slots.available_permits(), 1);

    let slots =
        instance_slots(&test_manifest("p", serde_json::json!({"max_instances": 3}))).unwrap();
    assert_eq!(slots.available_permits(), 3);

    // Zero instances is a manifest error, not a silently disabled plugin.
    assert!(instance_slots(&test_manifest("p", serde_json::json!({"max_instances": 0}))).is_err());
}

#[tokio::test]
async fn store_applies_manifest_fuel_limit() {
    let engine = test_engine();
    let plugin = test_plugin(
        &engine,
        "fueled",
        ECHO_WAT,
        serde_json::json!({"max_fuel": 12_345}),
    );
    let store = create_store(&engine, &plugin).await.expect("store");
    assert_eq!(store.get_fuel().unwrap(), 12_345);
}

// ---------------------------------------------------------------------------
// End-to-end coverage through the real HTTP handlers.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fuel_exhaustion_traps_cpu_bound_export() {
    let engine = test_engine();
    let plugin = test_plugin(
        &engine,
        "spinner",
        SPIN_WAT,
        serde_json::json!({"max_fuel": 100_000, "timeout_ms": 30_000}),
    );
    let router = build_router(test_state(engine.clone(), plugin.clone())).expect("router");

    let started = Instant::now();
    let (status, body) = call(&router, "spinner", "spin").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {body}");
    // The HTTP body only carries the outer trap context, so assert the root
    // cause on the same path the handler uses (instantiate + call_async).
    let (mut store, instance) = instantiate(&engine, &plugin).await.expect("instantiate");
    let func = instance
        .get_typed_func::<(), (u32,)>(&mut store, "spin")
        .expect("spin export");
    let trap = func
        .call_async(&mut store, ())
        .await
        .expect_err("fuel trap");
    let chain = format!("{trap:#}").to_ascii_lowercase();
    assert!(chain.contains("fuel"), "expected a fuel trap, got: {chain}");
    // The trap must happen well before the 30 s wall-clock timeout: fuel, not
    // time, stopped the guest.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "fuel limit did not stop the guest promptly: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn memory_limit_rejects_component_exceeding_max_memory_mb() {
    let engine = test_engine();

    // 1 MiB limit vs a component whose memory needs 4 MiB (64 pages).
    let denied = test_plugin(
        &engine,
        "memory",
        MEMORY_WAT,
        serde_json::json!({"max_memory_mb": 1}),
    );
    let router = build_router(test_state(engine.clone(), denied)).expect("router");
    let (status, body) = call(&router, "memory", "value").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {body}");
    assert!(
        body.to_ascii_lowercase().contains("memory"),
        "expected a memory limit error, got: {body}"
    );

    // Control: the same component loads when the manifest allows enough memory,
    // so the rejection above is the limit and not a broken fixture.
    let allowed = test_plugin(
        &engine,
        "memory",
        MEMORY_WAT,
        serde_json::json!({"max_memory_mb": 64}),
    );
    let router = build_router(test_state(engine, allowed)).expect("router");
    let (status, body) = call(&router, "memory", "value").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["result"],
        7
    );
}

#[tokio::test]
async fn max_instances_queues_second_call_until_slot_frees() {
    let engine = test_engine();
    let plugin = test_plugin(
        &engine,
        "single",
        ECHO_WAT,
        serde_json::json!({"max_instances": 1, "timeout_ms": 10_000}),
    );
    // Hold the only instance slot so the call below has to wait.
    let held = plugin.instance_slots.clone().acquire_owned().await.unwrap();
    let router = build_router(test_state(engine, plugin)).expect("router");

    let pending = tokio::spawn({
        let router = router.clone();
        async move { call(&router, "single", "value").await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !pending.is_finished(),
        "call must wait for an instance slot instead of running above max_instances"
    );

    drop(held);
    let (status, body) = pending.await.expect("call task");
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["result"],
        42
    );
}

#[tokio::test]
async fn max_instances_wait_times_out_with_manifest_timeout() {
    let engine = test_engine();
    let plugin = test_plugin(
        &engine,
        "single",
        ECHO_WAT,
        serde_json::json!({"max_instances": 1, "timeout_ms": 100}),
    );
    // Hold the only slot so `create_store` has to wait for it.
    let _held = plugin.instance_slots.clone().acquire_owned().await.unwrap();

    let started = Instant::now();
    let error = match create_store(&engine, &plugin).await {
        Ok(_) => panic!("slot wait must time out"),
        Err(error) => error,
    };
    let elapsed = started.elapsed();

    assert!(
        error.to_string().contains("instance limit wait timed out"),
        "expected the slot-wait timeout, got: {error:#}"
    );
    assert!(
        elapsed >= Duration::from_millis(90),
        "timeout_ms was not applied: returned after {elapsed:?}"
    );
}

#[tokio::test]
async fn timeout_ms_cancels_yielding_guest_call() {
    let engine = test_engine();
    let plugin = test_plugin(
        &engine,
        "sleepy",
        SLEEPY_WAT,
        serde_json::json!({"timeout_ms": 50, "max_fuel": 1_000_000_000}),
    );
    let router = build_router(test_state(engine, plugin)).expect("router");

    let started = Instant::now();
    let (status, body) = call(&router, "sleepy", "sleepy").await;
    let elapsed = started.elapsed();

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "body: {body}");
    assert!(
        body.contains("request timed out"),
        "expected the call timeout, got: {body}"
    );
    // The guest asked for 2 s; the 50 ms manifest timeout must cut it short.
    assert!(
        elapsed < Duration::from_secs(1),
        "timeout_ms did not interrupt the guest call: {elapsed:?}"
    );
}
