#![forbid(unsafe_code)]

//! Pure wait-loop helpers translated from the legacy Docker client loops.
//!
//! The legacy Go client used a handful of nearly identical polling loops for
//! stop, removal and exec completion. This module keeps those semantics in a
//! deterministic, transport-agnostic shape.

use std::time::{Duration, Instant};

/// Outcome for a wait loop that can time out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Completed,
    TimedOut,
}

/// Result for exec polling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecOutcome {
    Completed,
    SkipUpdate,
    ExitCode(i64),
}

/// Poll until a stop predicate returns `false` or the timeout expires.
///
/// The Go client considered a container "stopped" when repeated inspections no
/// longer reported it as running.
pub fn wait_for_stop_or_timeout<F>(wait_time: Duration, mut is_running: F) -> WaitOutcome
where
    F: FnMut() -> bool,
{
    poll_until(wait_time, || !is_running(), Duration::from_secs(1))
}

/// Poll until a removal predicate returns `true` or the timeout expires.
pub fn wait_for_removal_or_timeout<F>(wait_time: Duration, is_removed: F) -> WaitOutcome
where
    F: FnMut() -> bool,
{
    poll_until(wait_time, is_removed, Duration::from_secs(1))
}

/// Poll an exec state until it is no longer running.
///
/// `exit_code == 75` is preserved as the legacy "skip update" condition.
pub fn wait_for_exec_or_timeout<F>(
    timeout: Option<Duration>,
    exec_output: &str,
    mut inspect: F,
) -> ExecOutcome
where
    F: FnMut() -> Option<(bool, i64)>,
{
    let start = Instant::now();

    loop {
        let Some((running, exit_code)) = inspect() else {
            return ExecOutcome::Completed;
        };

        if running {
            if let Some(timeout) = timeout {
                if start.elapsed() >= timeout {
                    return ExecOutcome::Completed;
                }
            }

            std::thread::sleep(Duration::from_secs(1));
            continue;
        }

        if !exec_output.trim().is_empty() {
            tracing::info!("Command output:\n{exec_output}");
        }

        if exit_code == 75 {
            return ExecOutcome::SkipUpdate;
        }

        if exit_code > 0 {
            return ExecOutcome::ExitCode(exit_code);
        }

        return ExecOutcome::Completed;
    }
}

fn poll_until<F>(wait_time: Duration, mut predicate: F, sleep_interval: Duration) -> WaitOutcome
where
    F: FnMut() -> bool,
{
    let start = Instant::now();

    loop {
        if predicate() {
            return WaitOutcome::Completed;
        }

        if start.elapsed() >= wait_time {
            return WaitOutcome::TimedOut;
        }

        std::thread::sleep(sleep_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn stop_wait_completes_once_the_container_is_no_longer_running() {
        let states = [true, true, false];
        let index = Cell::new(0usize);

        let outcome = wait_for_stop_or_timeout(Duration::from_secs(5), || {
            let idx = index.get();
            index.set(idx + 1);
            states.get(idx).copied().unwrap_or(false)
        });

        assert_eq!(outcome, WaitOutcome::Completed);
    }

    #[test]
    fn removal_wait_times_out_when_the_container_never_disappears() {
        let outcome = wait_for_removal_or_timeout(Duration::from_millis(0), || false);

        assert_eq!(outcome, WaitOutcome::TimedOut);
    }

    #[test]
    fn exec_wait_returns_skip_update_for_exit_code_75() {
        let mut called = false;
        let outcome = wait_for_exec_or_timeout(Some(Duration::from_secs(1)), "done", || {
            if called {
                None
            } else {
                called = true;
                Some((false, 75))
            }
        });

        assert_eq!(outcome, ExecOutcome::SkipUpdate);
    }

    #[test]
    fn exec_wait_returns_exit_code_for_non_zero_failure() {
        let outcome =
            wait_for_exec_or_timeout(Some(Duration::from_secs(1)), "", || Some((false, 2)));

        assert_eq!(outcome, ExecOutcome::ExitCode(2));
    }
}
