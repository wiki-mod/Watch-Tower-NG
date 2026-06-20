#![forbid(unsafe_code)]

//! Pure helpers for the legacy `/v1/update` trigger semantics.
//!
//! The Go HTTP handler drained the request body before it parsed query
//! parameters. That transport-side I/O is intentionally not modeled here; this
//! module only covers input parsing and lock-gated update dispatch.

use std::sync::{Mutex, TryLockError};

/// Parse every `image` query value into a flat list of image filters.
///
/// The legacy handler used `strings.Split(value, ",")` for each query value and
/// preserved empty fragments. This helper keeps that exact behavior.
pub fn parse_image_queries<I, S>(image_queries: Option<I>) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    image_queries
        .map(flatten_image_queries)
        .unwrap_or_default()
}

/// Flatten query values into a single image list.
pub fn flatten_image_queries<I, S>(image_queries: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut images = Vec::new();

    for image_query in image_queries {
        images.extend(image_query.as_ref().split(',').map(str::to_string));
    }

    images
}

/// Outcome of a gated update attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateDecision {
    /// The update function ran.
    Triggered,
    /// The non-blocking empty-image path found the gate busy and skipped work.
    SkippedBusy,
}

/// Local gate that mirrors the legacy update lock behavior.
///
/// Empty image lists use a non-blocking try-lock and skip when another update is
/// already running. Non-empty image lists wait until the lock becomes available
/// and then run the update function.
#[derive(Debug, Default)]
pub struct UpdateGate {
    lock: Mutex<()>,
}

impl UpdateGate {
    /// Create a new gate instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Dispatch the update if the gate permits it.
    pub fn dispatch<F>(&self, images: &[String], update_fn: F) -> UpdateDecision
    where
        F: FnOnce(&[String]),
    {
        if images.is_empty() {
            return match self.lock.try_lock() {
                Ok(_guard) => {
                    update_fn(images);
                    UpdateDecision::Triggered
                }
                Err(TryLockError::WouldBlock) => UpdateDecision::SkippedBusy,
                Err(TryLockError::Poisoned(poisoned)) => {
                    let _guard = poisoned.into_inner();
                    update_fn(images);
                    UpdateDecision::Triggered
                }
            };
        }

        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update_fn(images);
        UpdateDecision::Triggered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::thread;

    #[test]
    fn parses_and_flattens_image_query_values_like_the_legacy_handler() {
        let images = parse_image_queries(Some(vec!["alpha,beta", "gamma,,delta", "epsilon"]));

        assert_eq!(
            images,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "".to_string(),
                "delta".to_string(),
                "epsilon".to_string(),
            ]
        );
    }

    #[test]
    fn parse_image_queries_returns_empty_when_the_key_is_missing() {
        let images: Vec<String> = parse_image_queries::<Vec<&str>, &str>(None);

        assert!(images.is_empty());
    }

    #[test]
    fn empty_image_updates_skip_when_another_update_holds_the_gate() {
        let gate = UpdateGate::new();
        let _held = gate.lock.lock().expect("gate lock should be available");
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let images = Vec::<String>::new();

        let decision = gate.dispatch(&images, move |_| {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert_eq!(decision, UpdateDecision::SkippedBusy);
        assert!(!called.load(Ordering::SeqCst));
    }

    #[test]
    fn non_empty_image_updates_wait_for_the_gate_and_then_trigger() {
        let gate = Arc::new(UpdateGate::new());
        let held = gate.lock.lock().expect("gate lock should be available");
        let (tx, rx) = mpsc::channel();
        let worker_gate = Arc::clone(&gate);

        let worker = thread::spawn(move || {
            let images = vec!["alpha".to_string(), "beta".to_string()];
            worker_gate.dispatch(&images, move |images| {
                tx.send(images.to_vec()).expect("send should succeed");
            })
        });

        drop(held);

        assert_eq!(
            worker.join().expect("worker should finish"),
            UpdateDecision::Triggered
        );
        assert_eq!(
            rx.recv().expect("worker should send the image list"),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }
}
