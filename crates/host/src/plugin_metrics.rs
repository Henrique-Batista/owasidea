//! Per-plugin metrics (PX-07).
//!
//! The host keeps one counter set per loaded plugin. Counters are plain
//! atomics so request paths never block on a global mutex; rendering happens
//! only when Prometheus scrapes `/metrics`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Counters for one loaded plugin.
#[derive(Debug, Default)]
pub(crate) struct PluginMetrics {
    requests: AtomicU64,
    errors: AtomicU64,
    latency_ms_sum: AtomicU64,
    latency_ms_count: AtomicU64,
    fuel: AtomicU64,
}

/// Point-in-time copy of one plugin's counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PluginMetricsSnapshot {
    pub requests: u64,
    pub errors: u64,
    pub latency_ms_sum: u64,
    pub latency_ms_count: u64,
    pub fuel: u64,
}

impl PluginMetrics {
    /// Record one instrumented operation for this plugin.
    pub(crate) fn record(&self, success: bool, elapsed_ms: u64, fuel: u64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if !success {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.latency_ms_sum.fetch_add(elapsed_ms, Ordering::Relaxed);
        self.latency_ms_count.fetch_add(1, Ordering::Relaxed);
        self.fuel.fetch_add(fuel, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> PluginMetricsSnapshot {
        PluginMetricsSnapshot {
            requests: self.requests.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            latency_ms_sum: self.latency_ms_sum.load(Ordering::Relaxed),
            latency_ms_count: self.latency_ms_count.load(Ordering::Relaxed),
            fuel: self.fuel.load(Ordering::Relaxed),
        }
    }
}

/// Registry of per-plugin counters, keyed by plugin name. Only loaded plugins
/// are registered, so `/metrics` never advertises a plugin that is not running.
pub(crate) type PluginMetricsMap = RwLock<HashMap<String, Arc<PluginMetrics>>>;

/// Record a metric for `name`, ignoring plugins that are not loaded.
pub(crate) fn record(
    map: &PluginMetricsMap,
    name: &str,
    success: bool,
    elapsed_ms: u64,
    fuel: u64,
) {
    if let Ok(guard) = map.read() {
        if let Some(metrics) = guard.get(name) {
            metrics.record(success, elapsed_ms, fuel);
        }
    }
}

/// Escape a Prometheus label value (backslash, double quote, newline).
pub(crate) fn escape_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Render the Prometheus text exposition for global and per-plugin metrics.
pub(crate) fn render(global_requests: u64, global_errors: u64, map: &PluginMetricsMap) -> String {
    let mut snapshots: Vec<(String, PluginMetricsSnapshot)> = map
        .read()
        .map(|guard| {
            guard
                .iter()
                .map(|(name, metrics)| (name.clone(), metrics.snapshot()))
                .collect()
        })
        .unwrap_or_default();
    // Deterministic output keeps scrape diffs and tests stable.
    snapshots.sort_by(|a, b| a.0.cmp(&b.0));

    let mut body = String::new();
    body.push_str("# TYPE owasidea_requests_total counter\n");
    body.push_str(&format!("owasidea_requests_total {global_requests}\n"));
    body.push_str("# TYPE owasidea_errors_total counter\n");
    body.push_str(&format!("owasidea_errors_total {global_errors}\n"));

    if snapshots.is_empty() {
        return body;
    }

    let labels = |name: &str| format!("{{plugin=\"{}\"}}", escape_label(name));
    let mut family = |metric: &str, value: fn(&PluginMetricsSnapshot) -> u64| {
        body.push_str(&format!("# TYPE {metric} counter\n"));
        for (name, snapshot) in &snapshots {
            body.push_str(&format!("{metric}{} {}\n", labels(name), value(snapshot)));
        }
    };
    family("owasidea_plugin_requests_total", |s| s.requests);
    family("owasidea_plugin_errors_total", |s| s.errors);
    family("owasidea_plugin_latency_ms_sum", |s| s.latency_ms_sum);
    family("owasidea_plugin_latency_ms_count", |s| s.latency_ms_count);
    family("owasidea_plugin_fuel_total", |s| s.fuel);

    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(names: &[&str]) -> PluginMetricsMap {
        let mut map = HashMap::new();
        for name in names {
            map.insert((*name).to_string(), Arc::new(PluginMetrics::default()));
        }
        RwLock::new(map)
    }

    #[test]
    fn records_success_and_error_counters() {
        let metrics = PluginMetrics::default();
        metrics.record(true, 10, 100);
        metrics.record(false, 20, 50);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.requests, 2);
        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.latency_ms_sum, 30);
        assert_eq!(snapshot.latency_ms_count, 2);
        assert_eq!(snapshot.fuel, 150);
    }

    #[test]
    fn record_ignores_unknown_plugins() {
        let map = registry(&["notes"]);
        record(&map, "ghost", true, 1, 1);
        let body = render(0, 0, &map);
        assert!(body.contains("owasidea_plugin_requests_total{plugin=\"notes\"} 0"));
        assert!(!body.contains("ghost"));
    }

    #[test]
    fn render_keeps_global_metrics_and_labels_plugins() {
        let map = registry(&["notes", "chat-go"]);
        record(&map, "notes", true, 12, 500);
        record(&map, "notes", false, 8, 250);
        record(&map, "chat-go", true, 3, 0);

        let body = render(9, 4, &map);
        assert!(body.contains("# TYPE owasidea_requests_total counter\nowasidea_requests_total 9"));
        assert!(body.contains("# TYPE owasidea_errors_total counter\nowasidea_errors_total 4"));
        assert!(body.contains("# TYPE owasidea_plugin_requests_total counter"));
        assert!(body.contains("owasidea_plugin_requests_total{plugin=\"notes\"} 2"));
        assert!(body.contains("owasidea_plugin_requests_total{plugin=\"chat-go\"} 1"));
        assert!(body.contains("owasidea_plugin_errors_total{plugin=\"notes\"} 1"));
        assert!(body.contains("owasidea_plugin_errors_total{plugin=\"chat-go\"} 0"));
        assert!(body.contains("owasidea_plugin_latency_ms_sum{plugin=\"notes\"} 20"));
        assert!(body.contains("owasidea_plugin_latency_ms_count{plugin=\"notes\"} 2"));
        assert!(body.contains("owasidea_plugin_fuel_total{plugin=\"notes\"} 750"));
        assert!(body.contains("owasidea_plugin_fuel_total{plugin=\"chat-go\"} 0"));
        // Sorted by plugin name, so chat-go precedes notes within each family.
        let first = body.find("owasidea_plugin_requests_total{plugin=\"chat-go\"}");
        let second = body.find("owasidea_plugin_requests_total{plugin=\"notes\"}");
        assert!(first < second);
    }

    #[test]
    fn render_escapes_label_values() {
        let map = registry(&["odd\"name\\with\nbreak"]);
        let body = render(0, 0, &map);
        assert!(body
            .contains("owasidea_plugin_requests_total{plugin=\"odd\\\"name\\\\with\\nbreak\"} 0"));
        assert!(!body.contains("odd\"name"));
    }

    #[test]
    fn render_without_plugins_preserves_global_metrics_only() {
        let map = registry(&[]);
        let body = render(3, 1, &map);
        assert_eq!(
            body,
            "# TYPE owasidea_requests_total counter\nowasidea_requests_total 3\n# TYPE owasidea_errors_total counter\nowasidea_errors_total 1\n"
        );
    }
}
