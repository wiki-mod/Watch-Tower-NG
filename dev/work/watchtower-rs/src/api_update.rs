#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

use tracing::{debug, info};

pub const PATH: &str = "/v1/update";

/// UpdateLock is a shared lock for synchronizing update operations.
/// Semantically equivalent to Go's `chan bool` with buffered capacity 1.
pub type UpdateLock = Arc<Mutex<bool>>;

/// Global lock cell for storing the active update lock.
/// If no lock is installed, a default is created on first access.
static LOCK_CELL: OnceLock<UpdateLock> = OnceLock::new();

fn lock_cell() -> &'static UpdateLock {
    LOCK_CELL.get_or_init(|| Arc::new(Mutex::new(true)))
}

fn install_lock(update_lock: Option<UpdateLock>) {
    if let Some(lock) = update_lock {
        let _ = LOCK_CELL.get_or_init(|| lock);
    } else {
        let _ = LOCK_CELL.get_or_init(|| Arc::new(Mutex::new(true)));
    }
}

/// Splits comma-separated image query strings into individual image names.
/// Mirrors Go's behavior: `strings.Split(image, ",")`.
fn split_images(queries: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    for query in queries {
        // Split comma-separated values, preserving empty strings (Go behavior)
        result.extend(query.split(',').map(str::to_string));
    }
    result
}

/// Handler is an API handler used for triggering container update scans.
/// Mirrors the Go structure with a callback function and HTTP path.
pub struct Handler<F>
where
    F: Fn(Vec<String>),
{
    fn_: F,
    pub path: String,
}

/// New is a factory function creating a new Handler instance.
/// It accepts an optional update_lock; if None, a default lock is created.
pub fn new<F>(update_fn: F, update_lock: Option<UpdateLock>) -> Handler<F>
where
    F: Fn(Vec<String>),
{
    install_lock(update_lock);

    Handler {
        fn_: update_fn,
        path: PATH.to_string(),
    }
}

impl<F> Handler<F>
where
    F: Fn(Vec<String>),
{
    /// Handle is the HTTP handler function that processes update requests.
    /// It reads the request body, parses image query parameters, and invokes the callback
    /// with synchronization controlled by the update lock.
    ///
    /// Mimics the Go behavior:
    /// - If images are provided: blocks until lock is available (mutual exclusion)
    /// - If images are empty: non-blocking attempt; skips if another update is running
    pub fn handle<R: Read>(&self, body: &mut R, image_queries: Option<Vec<String>>) {
        info!("Updates triggered by HTTP API request.");

        // Copy request body to stdout (mirrors Go's io.Copy(os.Stdout, r.Body))
        let mut stdout = io::stdout();
        if let Err(err) = io::copy(body, &mut stdout) {
            info!("{err}");
            return;
        }

        // Parse image list from queries
        let images = image_queries.map(split_images).unwrap_or_default();

        let lock = lock_cell();

        // Lock logic: mirrors Go's two-branch strategy
        if !images.is_empty() {
            // Images provided: block and acquire lock
            // In Go: chanValue := <-lock; defer func() { lock <- chanValue }()
            let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (self.fn_)(images);
        } else {
            // Images empty: non-blocking attempt
            // In Go: select { case chanValue := <-lock: ... default: log.Debug(...) }
            match lock.try_lock() {
                Ok(_guard) => {
                    (self.fn_)(images);
                }
                Err(TryLockError::WouldBlock) => {
                    debug!("Skipped. Another update already running.");
                }
                Err(TryLockError::Poisoned(poisoned)) => {
                    let _guard = poisoned.into_inner();
                    (self.fn_)(images);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn new_sets_the_path() {
        let _guard = TEST_LOCK.lock();
        let handler = new(|_: Vec<String>| {}, None);
        assert_eq!(handler.path, PATH);
    }

    #[test]
    fn handle_splits_image_queries_correctly() {
        let _guard = TEST_LOCK.lock();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);

        let handler = new(
            move |images: Vec<String>| {
                *captured_clone.lock().unwrap() = images;
            },
            None,
        );

        let mut body = Cursor::new(Vec::<u8>::new());
        handler.handle(
            &mut body,
            Some(vec!["alpha,beta".to_string(), "gamma,,delta".to_string()]),
        );

        let result = captured.lock().unwrap();
        assert_eq!(
            *result,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "".to_string(),
                "delta".to_string(),
            ]
        );
    }

    #[test]
    fn handle_passes_empty_image_list_when_query_is_missing() {
        let _guard = TEST_LOCK.lock();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);

        let handler = new(
            move |images: Vec<String>| {
                *captured_clone.lock().unwrap() = images;
            },
            None,
        );

        let mut body = Cursor::new(Vec::<u8>::new());
        handler.handle(&mut body, None);

        let result = captured.lock().unwrap();
        assert_eq!(*result, Vec::<String>::new());
    }

    #[test]
    fn handle_returns_early_on_io_error() {
        let _guard = TEST_LOCK.lock();
        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);

        let handler = new(
            move |_: Vec<String>| {
                *called_clone.lock().unwrap() = true;
            },
            None,
        );

        // Simulate read error by using a reader that fails
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("copy failed"))
            }
        }

        let mut body = FailingReader;
        handler.handle(&mut body, None);

        assert!(!*called.lock().unwrap());
    }
}
