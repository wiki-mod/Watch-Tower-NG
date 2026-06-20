#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

use tracing::{debug, info};

pub const PATH: &str = "/v1/update";

pub type UpdateLock = Arc<Mutex<()>>;

static LOCK: OnceLock<Mutex<UpdateLock>> = OnceLock::new();

fn lock_cell() -> &'static Mutex<UpdateLock> {
    LOCK.get_or_init(|| Mutex::new(default_lock()))
}

fn default_lock() -> UpdateLock {
    Arc::new(Mutex::new(()))
}

fn install_lock(update_lock: Option<UpdateLock>) {
    let lock = update_lock.unwrap_or_else(default_lock);
    let mut current = lock_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *current = lock;
}

fn current_lock() -> UpdateLock {
    lock_cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn split_images<I, S>(image_queries: I) -> Vec<String>
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

#[allow(non_snake_case)]
pub struct Handler<F> {
    fn_: F,
    pub Path: String,
}

#[allow(non_snake_case)]
pub fn New<F>(update_fn: F, update_lock: Option<UpdateLock>) -> Handler<F>
where
    F: Fn(Vec<String>),
{
    install_lock(update_lock);

    Handler {
        fn_: update_fn,
        Path: PATH.to_string(),
    }
}

#[allow(non_snake_case)]
impl<F> Handler<F>
where
    F: Fn(Vec<String>),
{
    pub fn Handle<I, S, R>(&self, body: &mut R, image_queries: Option<I>)
    where
        R: Read,
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        info!("Updates triggered by HTTP API request.");

        let mut stdout = io::stdout();
        if let Err(err) = io::copy(body, &mut stdout) {
            info!("{err}");
            return;
        }

        let images = image_queries.map(split_images).unwrap_or_default();

        if !images.is_empty() {
            let lock = current_lock();
            let _guard = lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (self.fn_)(images);
            return;
        }

        let lock = current_lock();
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
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Cursor;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::thread;
    use std::time::Duration;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("copy failed"))
        }
    }

    #[test]
    fn new_sets_the_legacy_path() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let handler = New(|_| {}, None);

        assert_eq!(handler.Path, PATH);
    }

    #[test]
    fn handle_splits_image_queries_like_the_legacy_handler() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let captured = Arc::new(Mutex::new(None));
        let captured_clone = Arc::clone(&captured);
        let handler = New(
            move |images| {
                *captured_clone
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(images);
            },
            None,
        );
        let mut body = Cursor::new(Vec::<u8>::new());

        handler.Handle(&mut body, Some(vec!["alpha,beta", "gamma,,delta"]));

        assert_eq!(
            captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .expect("handler should capture the image list"),
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
    fn handle_passes_an_empty_image_list_when_the_query_key_is_missing() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let captured = Arc::new(Mutex::new(None));
        let captured_clone = Arc::clone(&captured);
        let handler = New(
            move |images| {
                *captured_clone
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(images);
            },
            None,
        );
        let mut body = Cursor::new(Vec::<u8>::new());

        handler.Handle::<Vec<&str>, &str, _>(&mut body, None);

        assert_eq!(
            captured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .expect("handler should capture the image list"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn handle_skips_empty_image_updates_when_another_update_is_running() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let update_lock = default_lock();
        let _held = update_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let handler = New(
            move |_| {
                called_clone.store(true, Ordering::SeqCst);
            },
            Some(Arc::clone(&update_lock)),
        );
        let mut body = Cursor::new(Vec::<u8>::new());

        handler.Handle::<Vec<&str>, &str, _>(&mut body, None);

        assert!(!called.load(Ordering::SeqCst));
    }

    #[test]
    fn handle_waits_for_the_lock_when_images_are_present() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let update_lock = default_lock();
        let held = update_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (tx, rx) = mpsc::channel();
        let handler = New(
            move |images| {
                tx.send(images).expect("send should succeed");
            },
            Some(Arc::clone(&update_lock)),
        );

        let worker = thread::spawn(move || {
            let mut body = Cursor::new(Vec::<u8>::new());
            handler.Handle(&mut body, Some(vec!["alpha,beta"]));
        });

        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(held);

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1))
                .expect("worker should send the image list"),
            vec!["alpha".to_string(), "beta".to_string()]
        );
        worker.join().expect("worker should finish");
    }

    #[test]
    fn handle_returns_early_when_copying_the_request_body_fails() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let handler = New(
            move |_| {
                called_clone.store(true, Ordering::SeqCst);
            },
            None,
        );
        let mut body = FailingReader;

        handler.Handle::<Vec<&str>, &str, _>(&mut body, None);

        assert!(!called.load(Ordering::SeqCst));
    }
}
