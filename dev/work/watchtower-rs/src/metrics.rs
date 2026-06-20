use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

const METRICS_QUEUE_CAPACITY: usize = 10;

/// Summary of a single scan, equivalent to the legacy Go metrics payload.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metric {
    pub scanned: usize,
    pub updated: usize,
    pub failed: usize,
}

/// Minimal report contract required to build a [`Metric`].
pub trait ReportLike {
    fn scanned_len(&self) -> usize;
    fn updated_len(&self) -> usize;
    fn stale_len(&self) -> usize;
    fn failed_len(&self) -> usize;
}

#[cfg(not(standalone_metrics_test))]
impl ReportLike for crate::types::Report {
    fn scanned_len(&self) -> usize {
        self.scanned.len()
    }

    fn updated_len(&self) -> usize {
        self.updated.len()
    }

    fn stale_len(&self) -> usize {
        self.stale.len()
    }

    fn failed_len(&self) -> usize {
        self.failed.len()
    }
}

/// Create a scan metric from a report.
///
/// `updated + stale` mirrors the legacy compatibility behavior from Go.
#[allow(non_snake_case)]
pub fn NewMetric<R>(report: &R) -> Metric
where
    R: ReportLike + ?Sized,
{
    Metric {
        scanned: report.scanned_len(),
        updated: report.updated_len() + report.stale_len(),
        failed: report.failed_len(),
    }
}

/// Read-only snapshot of the in-memory sink.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub scanned: usize,
    pub updated: usize,
    pub failed: usize,
    pub total: usize,
    pub skipped: usize,
}

#[derive(Debug, Default)]
struct InMemoryMetricsSink {
    state: Mutex<MetricsSnapshot>,
}

impl InMemoryMetricsSink {
    fn snapshot(&self) -> MetricsSnapshot {
        *self.state.lock().expect("metrics sink lock poisoned")
    }

    fn record_scan(&self, metric: Metric) {
        let mut state = self.state.lock().expect("metrics sink lock poisoned");
        state.total += 1;
        state.scanned = metric.scanned;
        state.updated = metric.updated;
        state.failed = metric.failed;
    }

    fn record_skipped_scan(&self) {
        let mut state = self.state.lock().expect("metrics sink lock poisoned");
        state.total += 1;
        state.skipped += 1;
        state.scanned = 0;
        state.updated = 0;
        state.failed = 0;
    }
}

#[derive(Debug, Default)]
struct MetricQueue {
    state: Mutex<VecDeque<Option<Metric>>>,
    not_empty: Condvar,
    not_full: Condvar,
}

impl MetricQueue {
    fn enqueue(&self, metric: Option<Metric>) {
        let mut state = self.state.lock().expect("metrics queue lock poisoned");
        while state.len() >= METRICS_QUEUE_CAPACITY {
            state = self
                .not_full
                .wait(state)
                .expect("metrics queue lock poisoned");
        }
        state.push_back(metric);
        self.not_empty.notify_one();
    }

    fn dequeue(&self) -> Option<Metric> {
        let mut state = self.state.lock().expect("metrics queue lock poisoned");
        while state.is_empty() {
            state = self
                .not_empty
                .wait(state)
                .expect("metrics queue lock poisoned");
        }
        let metric = state
            .pop_front()
            .expect("metrics queue must contain one item");
        self.not_full.notify_one();
        metric
    }

    fn is_empty(&self) -> bool {
        self.state
            .lock()
            .expect("metrics queue lock poisoned")
            .is_empty()
    }
}

/// In-memory equivalent of the Go metrics handler.
///
/// This module intentionally avoids wiring a Prometheus client directly. The
/// sink keeps the same semantics for the last-scan gauges and the total/skipped
/// counters, so HTTP exposure can be added later without changing scan logic.
#[derive(Debug, Default)]
pub struct Metrics {
    queue: MetricQueue,
    sink: InMemoryMetricsSink,
}

impl Metrics {
    fn new_with_worker(spawn_worker: bool) -> Arc<Self> {
        let metrics = Arc::new(Self::default());

        if spawn_worker {
            let worker = Arc::clone(&metrics);
            thread::Builder::new()
                .name("watchtower-metrics".to_string())
                .spawn(move || worker.HandleUpdate())
                .expect("failed to spawn metrics worker");
        }

        metrics
    }

    fn apply_update(&self, change: Option<Metric>) {
        match change {
            Some(metric) => self.sink.record_scan(metric),
            None => self.sink.record_skipped_scan(),
        }
    }

    /// Return true when no queued updates are waiting to be processed.
    #[allow(non_snake_case)]
    pub fn QueueIsEmpty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Enqueue a scan metric. `None` means the scan was skipped and rescheduled.
    #[allow(non_snake_case)]
    pub fn Register(&self, metric: Option<Metric>) {
        self.queue.enqueue(metric);
    }

    /// Return the current in-memory counter/gauge state.
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.sink.snapshot()
    }

    /// Process queued metric updates forever.
    #[allow(non_snake_case)]
    pub fn HandleUpdate(self: Arc<Self>) -> ! {
        loop {
            let change = self.queue.dequeue();
            self.apply_update(change);
        }
    }
}

/// Return the process-global metrics handler.
#[allow(non_snake_case)]
pub fn Default() -> Arc<Metrics> {
    static DEFAULT_METRICS: OnceLock<Arc<Metrics>> = OnceLock::new();

    Arc::clone(DEFAULT_METRICS.get_or_init(|| Metrics::new_with_worker(true)))
}

/// Convenience wrapper around [`Default`] + [`Metrics::Register`].
#[allow(non_snake_case)]
pub fn RegisterScan(metric: Option<Metric>) {
    Default().Register(metric);
}

#[cfg(test)]
impl Metrics {
    fn new_for_tests() -> Arc<Self> {
        Self::new_with_worker(false)
    }

    fn handle_next_update_for_tests(&self) {
        let change = self.queue.dequeue();
        self.apply_update(change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct TestReport {
        scanned: usize,
        updated: usize,
        stale: usize,
        failed: usize,
    }

    impl ReportLike for TestReport {
        fn scanned_len(&self) -> usize {
            self.scanned
        }

        fn updated_len(&self) -> usize {
            self.updated
        }

        fn stale_len(&self) -> usize {
            self.stale
        }

        fn failed_len(&self) -> usize {
            self.failed
        }
    }

    #[test]
    fn new_metric_counts_updated_and_stale_together() {
        let report = TestReport {
            scanned: 6,
            updated: 2,
            stale: 3,
            failed: 1,
        };

        let metric = NewMetric(&report);

        assert_eq!(
            metric,
            Metric {
                scanned: 6,
                updated: 5,
                failed: 1,
            }
        );
    }

    #[test]
    fn register_enqueues_and_handle_update_records_latest_scan() {
        let metrics = Metrics::new_for_tests();

        assert!(metrics.QueueIsEmpty());

        metrics.Register(Some(Metric {
            scanned: 7,
            updated: 4,
            failed: 2,
        }));

        assert!(!metrics.QueueIsEmpty());

        metrics.handle_next_update_for_tests();

        assert!(metrics.QueueIsEmpty());
        assert_eq!(
            metrics.snapshot(),
            MetricsSnapshot {
                scanned: 7,
                updated: 4,
                failed: 2,
                total: 1,
                skipped: 0,
            }
        );
    }

    #[test]
    fn skipped_scan_increments_counter_and_resets_gauges() {
        let metrics = Metrics::new_for_tests();

        metrics.Register(Some(Metric {
            scanned: 3,
            updated: 1,
            failed: 1,
        }));
        metrics.handle_next_update_for_tests();

        metrics.Register(None);
        metrics.handle_next_update_for_tests();

        assert_eq!(
            metrics.snapshot(),
            MetricsSnapshot {
                scanned: 0,
                updated: 0,
                failed: 0,
                total: 2,
                skipped: 1,
            }
        );
    }

    #[test]
    fn default_returns_singleton_instance() {
        let left = Default();
        let right = Default();

        assert!(Arc::ptr_eq(&left, &right));
    }
}
