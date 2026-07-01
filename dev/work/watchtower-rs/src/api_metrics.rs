#![forbid(unsafe_code)]

//! Metrics API endpoint serving Prometheus-formatted metrics.
//!
//! **Experimental Feature**: This API was added in v1.0.4 and is still considered experimental.
//! If you notice any strange behavior, please report it in the repository issues.
//! See `docs/metrics.md` for configuration details.

use crate::metrics;
use prometheus::TextEncoder;

pub const PATH: &str = "/v1/metrics";

pub type Handle = fn() -> String;

#[derive(Debug, Clone)]
pub struct ApiMetrics {
    pub path: &'static str,
    pub handle: Handle,
}

impl ApiMetrics {
    pub fn new() -> Self {
        Self {
            path: PATH,
            handle: render_prometheus_metrics,
        }
    }

    pub fn legacy() -> Self {
        Self::new()
    }

    pub fn into_parts(self) -> (&'static str, Handle) {
        (self.path, self.handle)
    }
}

impl Default for ApiMetrics {
    fn default() -> Self {
        Self::new()
    }
}

pub type Handler = ApiMetrics;

fn render_prometheus_metrics() -> String {
    let registry = metrics::registry();
    let encoder = TextEncoder::new();

    match encoder.encode_to_string(&registry.gather()) {
        Ok(metrics_text) => metrics_text,
        Err(e) => {
            format!("# Error encoding metrics: {}\n", e)
        }
    }
}
