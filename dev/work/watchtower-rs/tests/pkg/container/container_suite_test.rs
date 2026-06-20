#![forbid(unsafe_code)]

/// Rust does not need explicit suite registration like Ginkgo.
///
/// This no-op test preserves the legacy `TestContainer` entry point so the
/// container package still has a named suite anchor, while the actual test
/// discovery remains driven by Rust's built-in test harness.
#[test]
fn test_container_suite() {}
