//! Optional OpenTelemetry tracing (RL-05).
//!
//! The host exports OTLP traces only when **both** hold:
//!
//! 1. the crate was built with `--features otel`, and
//! 2. `OTEL_EXPORTER_OTLP_ENDPOINT` (or the traces-specific variant) is set.
//!
//! Otherwise nothing changes: the plain `tracing_subscriber::fmt` subscriber is
//! installed, [`call_span`] returns a disabled span, and no exporter, runtime or
//! background task exists. The active flag is a single atomic load on the call
//! path, so the disabled cost is negligible.
//!
//! What is exported:
//!
//! - `http.request` — one span per inbound host HTTP request, with the W3C
//!   `traceparent` header honoured as parent context (context propagation).
//! - `plugin.call` — one span per plugin invocation, with `plugin`,
//!   `function`/`operation`, `call.kind` and `status` attributes, plus the
//!   OpenTelemetry `otel.status_code` used by backends.
//!
//! Service lifecycle calls (`start`, `stop`, `restart`) and executable
//! `wasi:http` plugins use the same `plugin.call` span with a different
//! `call.kind`.

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "otel")]
use axum::{
    body::Body,
    http::{HeaderMap, Request},
    middleware::Next,
    response::Response,
};
#[cfg(feature = "otel")]
use tracing::Instrument;

/// Environment variable that enables OTLP export (standard OpenTelemetry name).
pub(crate) const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
/// Traces-specific override; takes precedence over [`OTLP_ENDPOINT_ENV`].
pub(crate) const OTLP_TRACES_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";
/// `service.name` resource attribute reported to the collector.
#[cfg_attr(not(feature = "otel"), allow(dead_code))]
pub(crate) const SERVICE_NAME: &str = "owasidea-host";

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether OTLP export is actually running in this process.
pub(crate) fn enabled() -> bool {
    cfg!(feature = "otel") && ACTIVE.load(Ordering::Relaxed)
}

/// Resolve the OTLP endpoint from a raw environment value.
///
/// Empty or whitespace-only values are treated as unset, so an exported but
/// blank variable does not silently start an exporter against nowhere.
pub(crate) fn endpoint_from_value(value: Option<String>) -> Option<String> {
    let trimmed = value?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Resolve the OTLP endpoint from the process environment.
#[cfg_attr(not(feature = "otel"), allow(dead_code))]
pub(crate) fn endpoint_from_env() -> Option<String> {
    endpoint_from_value(std::env::var(OTLP_TRACES_ENDPOINT_ENV).ok())
        .or_else(|| endpoint_from_value(std::env::var(OTLP_ENDPOINT_ENV).ok()))
}

/// Handle that keeps the tracer provider alive and flushes spans on shutdown.
#[derive(Debug)]
pub(crate) struct TelemetryGuard {
    stopped: AtomicBool,
    #[cfg(feature = "otel")]
    provider: opentelemetry_sdk::trace::SdkTracerProvider,
}

impl TelemetryGuard {
    #[cfg(feature = "otel")]
    fn new(provider: opentelemetry_sdk::trace::SdkTracerProvider) -> Self {
        Self {
            stopped: AtomicBool::new(false),
            provider,
        }
    }

    #[cfg(not(feature = "otel"))]
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            stopped: AtomicBool::new(false),
        }
    }

    /// Flush pending spans and stop the exporter. Safe to call more than once
    /// (the explicit shutdown in `run` and the `Drop` impl both land here).
    pub(crate) fn shutdown(&self) {
        if self.stopped.swap(true, Ordering::Relaxed) {
            return;
        }
        #[cfg(feature = "otel")]
        {
            if let Err(error) = self.provider.shutdown() {
                tracing::warn!(%error, "OpenTelemetry shutdown failed");
            }
        }
        ACTIVE.store(false, Ordering::Relaxed);
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Install the tracing subscriber and, when configured, the OTLP exporter.
///
/// Returns `None` when OTLP export is disabled (feature off, endpoint unset, or
/// exporter construction failed). In that case the plain `fmt` subscriber is
/// installed exactly as before, so logs keep working.
pub(crate) fn init_tracing() -> Option<TelemetryGuard> {
    #[cfg(not(feature = "otel"))]
    {
        tracing_subscriber::fmt::init();
        None
    }

    #[cfg(feature = "otel")]
    {
        use opentelemetry::trace::TracerProvider as _;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let Some(endpoint) = endpoint_from_env() else {
            tracing_subscriber::fmt::init();
            return None;
        };

        match build_provider(&endpoint) {
            Ok(provider) => {
                let tracer = provider.tracer(SERVICE_NAME);
                opentelemetry::global::set_text_map_propagator(
                    opentelemetry_sdk::propagation::TraceContextPropagator::new(),
                );
                tracing_subscriber::registry()
                    // Matches `tracing_subscriber::fmt::init()`'s default filter.
                    .with(tracing_subscriber::filter::LevelFilter::INFO)
                    .with(tracing_subscriber::fmt::layer())
                    .with(tracing_opentelemetry::layer().with_tracer(tracer))
                    .init();
                ACTIVE.store(true, Ordering::Relaxed);
                tracing::info!(endpoint = %endpoint, "OpenTelemetry OTLP export enabled");
                Some(TelemetryGuard::new(provider))
            }
            Err(error) => {
                tracing_subscriber::fmt::init();
                tracing::warn!(%error, "OpenTelemetry disabled: {error}");
                None
            }
        }
    }
}

#[cfg(feature = "otel")]
fn build_provider(
    endpoint: &str,
) -> Result<opentelemetry_sdk::trace::SdkTracerProvider, opentelemetry_otlp::ExporterBuildError> {
    use opentelemetry_otlp::WithExportConfig;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .build()?;

    Ok(opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(SERVICE_NAME)
                .build(),
        )
        .build())
}

/// Start a `plugin.call` span for one plugin invocation.
///
/// Disabled (no-op span) unless OTLP export is active, so the default build and
/// the feature-enabled-but-unconfigured build keep today's behaviour.
pub(crate) fn call_span(plugin: &str, operation: &str, kind: &str) -> tracing::Span {
    if !enabled() {
        return tracing::Span::none();
    }
    new_call_span(plugin, operation, kind)
}

/// Unconditionally construct the `plugin.call` span. Kept separate so tests can
/// assert attributes without flipping the process-wide active flag.
fn new_call_span(plugin: &str, operation: &str, kind: &str) -> tracing::Span {
    #[cfg(feature = "otel")]
    {
        tracing::info_span!(
            "plugin.call",
            otel.kind = "server",
            otel.status_code = tracing::field::Empty,
            plugin = plugin,
            function = operation,
            call.kind = kind,
            status = tracing::field::Empty,
        )
    }
    #[cfg(not(feature = "otel"))]
    {
        let _ = (plugin, operation, kind);
        tracing::Span::none()
    }
}

/// Record the call outcome on a span created by [`call_span`].
pub(crate) fn record_outcome(span: &tracing::Span, ok: bool) {
    if !enabled() {
        return;
    }
    record_outcome_inner(span, ok);
}

fn record_outcome_inner(span: &tracing::Span, ok: bool) {
    span.record("status", if ok { "ok" } else { "error" });
    span.record("otel.status_code", if ok { "ok" } else { "error" });
}

/// Axum middleware: one `http.request` span per inbound request, with W3C
/// `traceparent` propagation so host spans join the caller's trace.
///
/// Only compiled with the `otel` feature; a passthrough when export is not
/// configured.
#[cfg(feature = "otel")]
pub(crate) async fn trace_context(request: Request<Body>, next: Next) -> Response {
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    if !enabled() {
        return next.run(request).await;
    }

    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let span = tracing::info_span!(
        "http.request",
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        http.request.method = %method,
        url.path = %path,
    );
    if let Some(parent) = parent_context(request.headers()) {
        // A malformed `traceparent` must never fail the request.
        if let Err(error) = span.set_parent(parent) {
            tracing::debug!(%error, "ignoring remote trace context");
        }
    }

    let response = next.run(request).instrument(span.clone()).await;
    let status = response.status();
    span.record("http.response.status_code", status.as_u16());
    span.record(
        "otel.status_code",
        if status.is_success() { "ok" } else { "error" },
    );
    response
}

#[cfg(feature = "otel")]
fn parent_context(headers: &HeaderMap) -> Option<opentelemetry::Context> {
    use opentelemetry::trace::TraceContextExt as _;

    struct HeaderExtractor<'a>(&'a HeaderMap);

    impl opentelemetry::propagation::Extractor for HeaderExtractor<'_> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.get(key).and_then(|value| value.to_str().ok())
        }

        fn keys(&self) -> Vec<&str> {
            self.0.keys().map(|key| key.as_str()).collect()
        }
    }

    let propagator = opentelemetry_sdk::propagation::TraceContextPropagator::new();
    let context = opentelemetry::propagation::TextMapPropagator::extract(
        &propagator,
        &HeaderExtractor(headers),
    );
    context.span().span_context().is_valid().then_some(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_or_missing_endpoint_disables_export() {
        assert_eq!(endpoint_from_value(None), None);
        assert_eq!(endpoint_from_value(Some(String::new())), None);
        assert_eq!(endpoint_from_value(Some("   \t".to_string())), None);
        assert_eq!(
            endpoint_from_value(Some(" http://127.0.0.1:4318 ".to_string())).as_deref(),
            Some("http://127.0.0.1:4318")
        );
    }

    #[test]
    fn call_span_is_disabled_without_active_export() {
        // The process never called `init_tracing` with an endpoint, so the
        // default build and the unconfigured feature build must stay no-op.
        assert!(!enabled());
        let span = call_span("notes", "greet", "library");
        assert!(span.is_disabled());
    }

    #[test]
    fn recording_outcome_without_export_is_a_noop() {
        let span = call_span("notes", "greet", "library");
        record_outcome(&span, false);
        assert!(span.is_disabled());
    }

    #[cfg(feature = "otel")]
    mod with_feature {
        use super::super::*;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        use tracing::field::{Field, Visit};
        use tracing::span::{Attributes, Id, Record};
        use tracing::{Event, Metadata, Subscriber};
        use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

        type Fields = HashMap<String, String>;
        type CreatedSpan = (Id, String, Fields);
        type RecordedFields = (Id, Fields);

        #[derive(Clone, Default)]
        struct Captured {
            spans: Arc<Mutex<Vec<CreatedSpan>>>,
            records: Arc<Mutex<Vec<RecordedFields>>>,
        }

        impl Captured {
            fn first_span(&self) -> (String, Fields) {
                let spans = self.spans.lock().unwrap();
                let (id, name, created) = spans.first().expect("span captured");
                let mut fields = created.clone();
                for (record_id, recorded) in self.records.lock().unwrap().iter() {
                    if record_id == id {
                        fields.extend(recorded.clone());
                    }
                }
                (name.clone(), fields)
            }
        }

        struct FieldVisitor<'a>(&'a mut Fields);

        impl Visit for FieldVisitor<'_> {
            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.insert(field.name().to_string(), value.to_string());
            }

            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0
                    .insert(field.name().to_string(), format!("{value:?}"));
            }
        }

        impl<S: Subscriber> Layer<S> for Captured {
            fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _ctx: Context<'_, S>) {
                let mut fields = HashMap::new();
                attrs.record(&mut FieldVisitor(&mut fields));
                self.spans.lock().unwrap().push((
                    id.clone(),
                    attrs.metadata().name().to_string(),
                    fields,
                ));
            }

            fn on_record(&self, id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
                let mut fields = HashMap::new();
                values.record(&mut FieldVisitor(&mut fields));
                self.records.lock().unwrap().push((id.clone(), fields));
            }

            fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) {}

            fn register_callsite(
                &self,
                _metadata: &'static Metadata<'static>,
            ) -> tracing::subscriber::Interest {
                tracing::subscriber::Interest::always()
            }
        }

        fn capture<T>(run: impl FnOnce() -> T) -> (Captured, T) {
            let captured = Captured::default();
            let subscriber = tracing_subscriber::registry().with(captured.clone());
            let guard = tracing::subscriber::set_default(subscriber);
            let value = run();
            drop(guard);
            (captured, value)
        }

        #[test]
        fn call_span_carries_plugin_function_and_kind() {
            let (captured, _) = capture(|| {
                let span = new_call_span("notes", "greet", "library");
                span.in_scope(|| {});
            });

            let (name, fields) = captured.first_span();
            assert_eq!(name, "plugin.call");
            assert_eq!(fields.get("plugin").map(String::as_str), Some("notes"));
            assert_eq!(fields.get("function").map(String::as_str), Some("greet"));
            assert_eq!(fields.get("call.kind").map(String::as_str), Some("library"));
            assert_eq!(fields.get("otel.kind").map(String::as_str), Some("server"));
        }

        #[test]
        fn outcome_is_recorded_as_status_attributes() {
            let (captured, _) = capture(|| {
                let span = new_call_span("chat-go", "/app", "executable");
                record_outcome_inner(&span, false);
            });

            let (_, fields) = captured.first_span();
            assert_eq!(fields.get("status").map(String::as_str), Some("error"));
            assert_eq!(
                fields.get("otel.status_code").map(String::as_str),
                Some("error")
            );
        }

        #[test]
        fn traceparent_header_yields_a_valid_parent_context() {
            let mut headers = HeaderMap::new();
            headers.insert(
                "traceparent",
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
                    .parse()
                    .unwrap(),
            );
            assert!(parent_context(&headers).is_some());

            headers.insert("traceparent", "garbage".parse().unwrap());
            assert!(parent_context(&headers).is_none());
        }
    }
}
