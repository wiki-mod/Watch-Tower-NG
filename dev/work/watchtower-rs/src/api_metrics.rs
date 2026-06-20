#![forbid(unsafe_code)]

use std::sync::Arc;

use crate::metrics::{self, Metrics};

pub const PATH: &str = "/v1/metrics";

pub type Handle = fn(&Metrics) -> String;

#[derive(Debug, Clone)]
pub struct ApiMetrics {
    pub path: &'static str,
    pub handle: Handle,
    pub metrics: Arc<Metrics>,
}

impl ApiMetrics {
    pub fn new() -> Self {
        Self {
            path: PATH,
            handle: render_prometheus_metrics,
            metrics: metrics::Default(),
        }
    }

    pub fn legacy() -> Self {
        Self::new()
    }

    pub fn into_parts(self) -> (&'static str, Handle, Arc<Metrics>) {
        (self.path, self.handle, self.metrics)
    }
}

impl Default for ApiMetrics {
    fn default() -> Self {
        Self::new()
    }
}

pub type Handler = ApiMetrics;

fn render_prometheus_metrics(metrics: &Metrics) -> String {
    let snapshot = metrics.snapshot();

    format!(
        concat!(
            "# HELP watchtower_containers_scanned Number of containers scanned for changes by watchtower during the last scan\n",
            "# TYPE watchtower_containers_scanned gauge\n",
            "watchtower_containers_scanned {}\n",
            "# HELP watchtower_containers_updated Number of containers updated by watchtower during the last scan\n",
            "# TYPE watchtower_containers_updated gauge\n",
            "watchtower_containers_updated {}\n",
            "# HELP watchtower_containers_failed Number of containers where update failed during the last scan\n",
            "# TYPE watchtower_containers_failed gauge\n",
            "watchtower_containers_failed {}\n",
            "# HELP watchtower_scans_total Number of scans since the watchtower started\n",
            "# TYPE watchtower_scans_total counter\n",
            "watchtower_scans_total {}\n",
            "# HELP watchtower_scans_skipped Number of skipped scans since watchtower started\n",
            "# TYPE watchtower_scans_skipped counter\n",
            "watchtower_scans_skipped {}\n",
        ),
        snapshot.scanned,
        snapshot.updated,
        snapshot.failed,
        snapshot.total,
        snapshot.skipped,
    )
}
