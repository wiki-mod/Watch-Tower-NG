#![forbid(unsafe_code)]

/// Rust does not need explicit suite registration like Ginkgo.
///
/// This no-op test preserves the legacy `TestRegistry` entry point so the
/// registry package still has a named suite anchor, while the actual test
/// discovery remains driven by Rust's built-in test harness.
#[test]
fn test_registry_suite() {}
