use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use crate::types;

const CHANNEL_CAPACITY: usize = 10;

/// Metric is the data points of a single scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metric {
    pub scanned: usize,
    pub updated: usize,
    pub failed: usize,
}

/// Read-only copy of the current metric state for API rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub scanned: usize,
    pub updated: usize,
    pub failed: usize,
    pub total: usize,
    pub skipped: usize,
}

/// Metrics is the handler processing all individual scan metrics.
#[derive(Debug)]
pub struct Metrics {
    channel: Arc<MetricChannel>,
    state: Mutex<MetricsSnapshot>,
}

#[derive(Debug)]
struct MetricChannel {
    queue: Mutex<Vec<Option<Metric>>>,
    not_empty: Condvar,
}

impl MetricChannel {
    fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::with_capacity(CHANNEL_CAPACITY)),
            not_empty: Condvar::new(),
        }
    }

    fn enqueue(&self, metric: Option<Metric>) {
        let mut queue = self.queue.lock().expect("metrics channel lock poisoned");
        queue.push(metric);
        self.not_empty.notify_one();
    }

    fn dequeue(&self) -> Option<Metric> {
        let mut queue = self.queue.lock().expect("metrics channel lock poisoned");
        while queue.is_empty() {
            queue = self
                .not_empty
                .wait(queue)
                .expect("metrics channel lock poisoned");
        }
        queue.remove(0)
    }

    fn is_empty(&self) -> bool {
        self.queue
            .lock()
            .expect("metrics channel lock poisoned")
            .is_empty()
    }
}

impl Metrics {
    fn new() -> Self {
        Self {
            channel: Arc::new(MetricChannel::new()),
            state: Mutex::new(MetricsSnapshot::default()),
        }
    }

    /// QueueIsEmpty checks whether any messages are enqueued in the channel.
    #[allow(non_snake_case)]
    pub fn QueueIsEmpty(&self) -> bool {
        self.channel.is_empty()
    }

    /// Register registers metrics for an executed scan.
    #[allow(non_snake_case)]
    pub fn Register(&self, metric: &Metric) {
        self.channel.enqueue(Some(*metric));
    }

    /// snapshot returns a read-only copy of the current metric state.
    /// This is used by the API for rendering metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        *self.state.lock().expect("metrics state lock poisoned")
    }

    /// HandleUpdate dequeues the metric channel and processes it.
    #[allow(non_snake_case)]
    pub fn HandleUpdate(self: Arc<Self>) -> ! {
        loop {
            let change = self.channel.dequeue();
            self.apply_update(change);
        }
    }

    fn apply_update(&self, change: Option<Metric>) {
        let mut state = self.state.lock().expect("metrics state lock poisoned");

        state.total += 1;

        match change {
            Some(change) => {
                state.scanned = change.scanned;
                state.updated = change.updated;
                state.failed = change.failed;
            }
            None => {
                state.skipped += 1;
                state.scanned = 0;
                state.updated = 0;
                state.failed = 0;
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
pub fn Default() -> Arc<Metrics> {
    static METRICS: OnceLock<Arc<Metrics>> = OnceLock::new();

    Arc::clone(METRICS.get_or_init(|| {
        let metrics = Arc::new(Metrics::new());
        let worker = Arc::clone(&metrics);
        thread::Builder::new()
            .name("watchtower-metrics".to_string())
            .spawn(move || worker.HandleUpdate())
            .expect("failed to spawn metrics worker");
        metrics
    }))
}

/// RegisterScan fetches a metric handler and enqueues a metric.
#[allow(non_snake_case)]
pub fn RegisterScan(metric: &Metric) {
    let metrics = Default();
    metrics.Register(metric);
}
