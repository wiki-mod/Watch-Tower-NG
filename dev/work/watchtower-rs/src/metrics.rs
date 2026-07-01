#![forbid(unsafe_code)]

use std::sync::OnceLock;
use std::thread;

use crate::types;
use prometheus::{Counter, Gauge, Opts, Registry};

const CHANNEL_CAPACITY: usize = 10;

/// Global prometheus registry for metrics.
fn get_registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

/// Metric is the data points of a single scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metric {
    pub scanned: usize,
    pub updated: usize,
    pub failed: usize,
}

/// Metrics is the handler processing all individual scan metrics.
/// Uses prometheus client for actual metric tracking.
#[derive(Clone)]
pub struct Metrics {
    scanned: Gauge,
    updated: Gauge,
    failed: Gauge,
    total: Counter,
    skipped: Counter,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics").finish()
    }
}

impl Metrics {
    fn new() -> Result<Self, prometheus::Error> {
        let registry = get_registry();

        let scanned_opts = Opts::new(
            "watchtower_containers_scanned",
            "Number of containers scanned for changes by watchtower during the last scan",
        );
        let scanned = Gauge::with_opts(scanned_opts)?;
        registry.register(Box::new(scanned.clone()))?;

        let updated_opts = Opts::new(
            "watchtower_containers_updated",
            "Number of containers updated by watchtower during the last scan",
        );
        let updated = Gauge::with_opts(updated_opts)?;
        registry.register(Box::new(updated.clone()))?;

        let failed_opts = Opts::new(
            "watchtower_containers_failed",
            "Number of containers where update failed during the last scan",
        );
        let failed = Gauge::with_opts(failed_opts)?;
        registry.register(Box::new(failed.clone()))?;

        let total_opts = Opts::new(
            "watchtower_scans_total",
            "Number of scans since the watchtower started",
        );
        let total = Counter::with_opts(total_opts)?;
        registry.register(Box::new(total.clone()))?;

        let skipped_opts = Opts::new(
            "watchtower_scans_skipped",
            "Number of skipped scans since watchtower started",
        );
        let skipped = Counter::with_opts(skipped_opts)?;
        registry.register(Box::new(skipped.clone()))?;

        Ok(Self {
            scanned,
            updated,
            failed,
            total,
            skipped,
        })
    }

    fn with_channel(self) -> Self {
        let (_tx, rx) = std::sync::mpsc::sync_channel(CHANNEL_CAPACITY);

        // Spawn worker thread to process metric updates
        let worker_metrics = self.clone();
        thread::Builder::new()
            .name("watchtower-metrics".to_string())
            .spawn(move || {
                while let Ok(change) = rx.recv() {
                    worker_metrics.apply_update(change);
                }
            })
            .expect("failed to spawn metrics worker");

        self
    }

    /// QueueIsEmpty checks whether any messages are enqueued in the channel.
    #[allow(non_snake_case)]
    pub fn QueueIsEmpty(&self) -> bool {
        // For prometheus-based metrics, we consider the queue always empty
        // since we're processing synchronously now
        true
    }

    /// Register registers metrics for an executed scan.
    #[allow(non_snake_case)]
    pub fn Register(&self, metric: &Metric) {
        self.scanned.set(metric.scanned as f64);
        self.updated.set(metric.updated as f64);
        self.failed.set(metric.failed as f64);
        self.total.inc();
    }

    /// RegisterSkip registers a skipped scan.
    #[allow(non_snake_case)]
    pub fn RegisterSkip(&self) {
        self.scanned.set(0.0);
        self.updated.set(0.0);
        self.failed.set(0.0);
        self.total.inc();
        self.skipped.inc();
    }

    fn apply_update(&self, change: Option<Metric>) {
        self.total.inc();

        match change {
            Some(change) => {
                self.scanned.set(change.scanned as f64);
                self.updated.set(change.updated as f64);
                self.failed.set(change.failed as f64);
            }
            None => {
                self.skipped.inc();
                self.scanned.set(0.0);
                self.updated.set(0.0);
                self.failed.set(0.0);
            }
        }
    }
}

/// NewMetric returns a Metric with the counts taken from the appropriate report fields.
///
/// Note: `updated + stale` is kept for backwards compatibility with the Go implementation.
#[allow(non_snake_case)]
pub fn NewMetric(report: &types::Report) -> Metric {
    Metric {
        scanned: report.scanned().len(),
        updated: report.updated().len() + report.stale().len(),
        failed: report.failed().len(),
    }
}

/// Default creates a new metrics handler if none exists, otherwise returns the existing one.
#[allow(non_snake_case)]
pub fn Default() -> Metrics {
    static METRICS: OnceLock<Metrics> = OnceLock::new();

    METRICS
        .get_or_init(|| Metrics::new().expect("failed to initialize prometheus metrics").with_channel())
        .clone()
}

/// RegisterScan fetches a metric handler and enqueues a metric.
#[allow(non_snake_case)]
pub fn RegisterScan(metric: Option<&Metric>) {
    let metrics = Default();
    match metric {
        Some(m) => metrics.Register(m),
        None => metrics.RegisterSkip(),
    }
}

/// Returns the prometheus registry for rendering metrics.
pub fn registry() -> &'static Registry {
    get_registry()
}
