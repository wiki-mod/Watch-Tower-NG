#![forbid(unsafe_code)]

//! Pure helpers for the legacy `/v1/metrics` endpoint shape.
//!
//! The Go version wrapped a Prometheus handler around a metrics singleton.
//! This Rust counterpart keeps only the stable data needed for later HTTP
//! wiring: the endpoint path and a handle to the existing in-memory metrics
//! basis.

use std::sync::Arc;

use crate::metrics::{self, Metrics, MetricsSnapshot};

/// Legacy HTTP path for the metrics endpoint.
pub const PATH: &str = "/v1/metrics";

/// Small, testable shell for the metrics endpoint wiring.
///
/// The shell is intentionally data-only. An HTTP adapter can read the path and
/// the metrics handle later without depending on Prometheus-specific glue.
#[derive(Debug, Clone)]
pub struct ApiMetrics {
    pub path: &'static str,
    pub metrics: Arc<Metrics>,
}

impl ApiMetrics {
    /// Build the endpoint shell from an explicit metrics handle.
    ///
    /// This keeps the module input-driven and easy to test.
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self { path: PATH, metrics }
    }

    /// Build the legacy-compatible variant backed by the process-global
    /// metrics singleton.
    pub fn legacy() -> Self {
        Self::new(metrics::Default())
    }

    /// Read the current metrics snapshot from the injected basis.
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Split the shell into its path and metrics handle.
    pub fn into_parts(self) -> (&'static str, Arc<Metrics>) {
        (self.path, self.metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keeps_the_legacy_path_and_injected_metrics_handle() {
        let metrics = Arc::new(Metrics::default());
        let api = ApiMetrics::new(Arc::clone(&metrics));

        assert_eq!(api.path, PATH);
        assert!(Arc::ptr_eq(&api.metrics, &metrics));
    }

    #[test]
    fn snapshot_reads_from_the_existing_metrics_basis() {
        let metrics = Arc::new(Metrics::default());
        let api = ApiMetrics::new(Arc::clone(&metrics));

        assert_eq!(api.snapshot(), metrics.snapshot());
        assert_eq!(api.snapshot(), MetricsSnapshot::default());
    }

    #[test]
    fn legacy_uses_the_process_global_metrics_singleton() {
        let api = ApiMetrics::legacy();
        let global_metrics = metrics::Default();

        assert_eq!(api.path, PATH);
        assert!(Arc::ptr_eq(&api.metrics, &global_metrics));
    }

    #[test]
    fn into_parts_preserves_the_data_shell() {
        let metrics = Arc::new(Metrics::default());
        let api = ApiMetrics::new(Arc::clone(&metrics));

        let (path, returned_metrics) = api.into_parts();

        assert_eq!(path, PATH);
        assert!(Arc::ptr_eq(&returned_metrics, &metrics));
    }
}
