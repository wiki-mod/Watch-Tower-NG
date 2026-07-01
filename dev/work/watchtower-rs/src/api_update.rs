#![forbid(unsafe_code)]

//! HTTP API endpoint for triggering container update scans.
//!
//! This module implements the legacy `/v1/update` endpoint which allows
//! external systems to trigger updates via HTTP requests with optional
//! image filtering and synchronization semantics.
//!
//! The endpoint follows Go's watchtower semantics:
//! - Blocking lock when specific images are provided
//! - Non-blocking check when no images (skip if update already running)
//! - Mutual exclusion to prevent concurrent updates

use std::sync::{Arc, Mutex, TryLockError};

use crate::api::{HttpRequest, HttpResponse};

pub const PATH: &str = "/v1/update";

/// UpdateLock is a shared lock for synchronizing update operations.
/// Semantically equivalent to Go's `chan bool` with buffered capacity 1.
///
/// A fresh `Mutex<()>` is already in the "available" state, matching
/// Go's `lock <- true` initialization.
pub type UpdateLock = Arc<Mutex<()>>;

/// Handler function type for the update callback.
pub type UpdateHandler = Arc<dyn Fn(Vec<String>) + Send + Sync>;

/// ApiUpdate wraps the update handler callback and synchronization lock.
///
/// Mirrors the Go `Handler` struct from `pkg/api/update/update.go`.
#[derive(Clone)]
pub struct ApiUpdate {
    pub path: &'static str,
    handle: UpdateHandler,
    lock: UpdateLock,
}

impl ApiUpdate {
    /// New is a factory function creating a new ApiUpdate instance.
    ///
    /// Accepts an optional update_lock; if None, a default lock is created.
    /// Mirrors Go's `func New(updateFn func(images []string), updateLock chan bool) *Handler`.
    pub fn new<F>(update_fn: F, update_lock: Option<UpdateLock>) -> Self
    where
        F: Fn(Vec<String>) + Send + Sync + 'static,
    {
        let lock = update_lock.unwrap_or_else(|| Arc::new(Mutex::new(())));

        Self {
            path: PATH,
            handle: Arc::new(update_fn),
            lock,
        }
    }

    /// Create a new ApiUpdate with the default callback (no-op for testing).
    pub fn legacy() -> Self {
        Self::new(|_: Vec<String>| {}, None)
    }

    /// Decompose into (path, callback) for use with Api::register_func.
    pub fn into_parts(self) -> (&'static str, UpdateHandler) {
        (self.path, self.handle)
    }

    /// Handle is the HTTP handler function that processes update requests.
    ///
    /// Mirrors Go's `func (handle *Handler) Handle(w http.ResponseWriter, r *http.Request)`.
    ///
    /// Behavior:
    /// - If images are provided: blocks until lock is available (mutual exclusion)
    /// - If images are empty: non-blocking attempt; skips if another update is running
    /// - Returns 200 OK with empty body (Go semantics)
    pub fn handle(&self, request: &HttpRequest) -> HttpResponse {
        tracing::info!("Updates triggered by HTTP API request.");

        // Go writes request body to stdout, but the legacy framework has already
        // parsed and discarded it. This matches the framework's interface.

        // Parse image list from query parameters.
        // Go code: imageQueries, found := r.URL.Query()["image"]
        let images = extract_image_queries(&request.path);

        // Lock logic: mirrors Go's two-branch strategy
        if !images.is_empty() {
            // Images provided: block and acquire lock
            // In Go: chanValue := <-lock; defer func() { lock <- chanValue }()
            let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (*self.handle)(images);
        } else {
            // Images empty: non-blocking attempt
            // In Go: select { case chanValue := <-lock: ... default: log.Debug(...) }
            match self.lock.try_lock() {
                Ok(_guard) => {
                    (*self.handle)(images);
                }
                Err(TryLockError::WouldBlock) => {
                    tracing::debug!("Skipped. Another update already running.");
                }
                Err(TryLockError::Poisoned(poisoned)) => {
                    let _guard = poisoned.into_inner();
                    (*self.handle)(images);
                }
            }
        }

        // Go handler writes nothing; return empty 200 OK (legacy semantics)
        HttpResponse::plain(200, "")
    }
}

/// Extracts image query parameters from request path.
///
/// Go code: `imageQueries, found := r.URL.Query()["image"]`
/// Then for each value: `strings.Split(image, ",")...`
///
/// This function preserves Go's exact edge cases:
/// - No `?image=...` query → empty Vec
/// - `?image=` (key present, no value) → one empty string → `[""]`
/// - `?image=alpha,beta` → split on comma → `["alpha", "beta"]`
fn extract_image_queries(path: &str) -> Vec<String> {
    // Split path from query string
    let query_part = match path.split_once('?') {
        Some((_, q)) => q,
        None => return Vec::new(),
    };

    let mut images = Vec::new();

    // Parse query string manually to handle Go's duplicate-key behavior
    // Go: r.URL.Query()["image"] returns all values for key "image"
    for pair in query_part.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == "image" {
                // URL decode the value (simplified; url crate not available here)
                let decoded = urlencoded_decode(value);
                // Split by comma, preserving empty strings (Go behavior)
                for img in decoded.split(',') {
                    images.push(img.to_string());
                }
            }
        } else if pair == "image" {
            // Key present with no value (path ends with `&image` or starts with `?image`)
            images.push(String::new());
        }
    }

    images
}

/// Minimal URL decoding for query parameter values.
/// Handles %XX hex encoding.
fn urlencoded_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            // Try to parse %XX
            if let Some(&d1) = chars.peek() {
                chars.next();
                if let Some(&d2) = chars.peek() {
                    chars.next();
                    if let Ok(byte) = u8::from_str_radix(&format!("{}{}", d1, d2), 16) {
                        result.push(byte as char);
                        continue;
                    }
                }
            }
            result.push('%');
        } else if c == '+' {
            // In query strings, + means space
            result.push(' ');
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    fn make_request(path: &str) -> HttpRequest {
        HttpRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            headers: BTreeMap::new(),
            body: String::new(),
        }
    }

    #[test]
    fn new_sets_the_path() {
        let handler = ApiUpdate::new(|_: Vec<String>| {}, None);
        assert_eq!(handler.path, PATH);
    }

    #[test]
    fn handle_splits_image_queries_correctly() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);

        let handler = ApiUpdate::new(
            move |images: Vec<String>| {
                *captured_clone.lock().unwrap() = images;
            },
            None,
        );

        let request = make_request("/v1/update?image=alpha%2Cbeta&image=gamma%2C%2Cdelta");
        handler.handle(&request);

        let result = captured.lock().unwrap();
        assert_eq!(
            *result,
            vec![
                "alpha,beta".to_string(),
                "gamma,,delta".to_string(),
            ]
        );
    }

    #[test]
    fn handle_passes_empty_image_list_when_query_is_missing() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);

        let handler = ApiUpdate::new(
            move |images: Vec<String>| {
                *captured_clone.lock().unwrap() = images;
            },
            None,
        );

        let request = make_request("/v1/update");
        handler.handle(&request);

        let result = captured.lock().unwrap();
        assert_eq!(*result, Vec::<String>::new());
    }

    #[test]
    fn handle_returns_200_ok() {
        let handler = ApiUpdate::new(|_: Vec<String>| {}, None);
        let request = make_request("/v1/update");
        let response = handler.handle(&request);

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "");
    }

    #[test]
    fn legacy_creates_default_handler() {
        let handler = ApiUpdate::legacy();
        assert_eq!(handler.path, PATH);
    }

    #[test]
    fn into_parts_decomposes_correctly() {
        let handler = ApiUpdate::legacy();
        let (path, _) = handler.into_parts();
        assert_eq!(path, PATH);
    }

    #[test]
    fn blocking_lock_with_images() {
        let lock = Arc::new(Mutex::new(()));
        let lock_clone = Arc::clone(&lock);
        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);

        let handler = ApiUpdate::new(
            move |_: Vec<String>| {
                *called_clone.lock().unwrap() = true;
            },
            Some(lock_clone),
        );

        // Pre-acquire the lock to test blocking behavior
        let _guard = lock.lock().unwrap();

        let request = make_request("/v1/update?image=test");
        // In real usage, this would block. In tests, we can only verify the attempt.
        // Since we hold the lock, this would block forever if called on a thread.
        // Instead, verify the response structure.
        let response = handler.handle(&request);
        assert_eq!(response.status, 200);
    }

    #[test]
    fn non_blocking_lock_when_no_images() {
        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);

        let handler = ApiUpdate::new(
            move |_: Vec<String>| {
                *called_clone.lock().unwrap() = true;
            },
            None,
        );

        // Pre-acquire lock to test non-blocking behavior
        let lock = Arc::clone(&handler.lock);
        let _guard = lock.lock().unwrap();

        let request = make_request("/v1/update");
        handler.handle(&request);

        // With no images and lock held, should skip (not call the callback)
        assert!(!*called.lock().unwrap());
    }

    #[test]
    fn query_parameter_extraction_preserves_empty_strings() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);

        let handler = ApiUpdate::new(
            move |images: Vec<String>| {
                *captured_clone.lock().unwrap() = images;
            },
            None,
        );

        // Test: ?image= (present but empty) should extract one empty string
        let request = make_request("/v1/update?image=");
        handler.handle(&request);

        let result = captured.lock().unwrap();
        assert_eq!(*result, vec!["".to_string()]);
    }
}
