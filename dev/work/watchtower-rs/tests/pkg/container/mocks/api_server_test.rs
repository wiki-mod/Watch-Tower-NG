#![forbid(unsafe_code)]

// This test file exists to ensure the ApiServer mock module compiles without errors or warnings.
// It imports the mock API server utilities and runs basic sanity checks.

#[path = "ApiServer.rs"]
mod api_server;

use api_server::*;

#[test]
fn test_create_filter_args_single() {
    let filters = createFilterArgs(&["running"]);
    assert!(filters.0.contains_key("status"));
}

#[test]
fn test_create_filter_args_multiple() {
    let filters = createFilterArgs(&["running", "paused", "exited"]);
    assert_eq!(filters.0.get("status").map(|m| m.len()), Some(3));
}

#[test]
fn test_watchtower_container_ref() {
    assert_eq!(Watchtower.name, "watchtower");
    assert!(!Watchtower.is_missing);
}

#[test]
fn test_stopped_container_ref() {
    assert_eq!(Stopped.name, "stopped");
    assert!(!Stopped.is_missing);
}

#[test]
fn test_running_container_ref() {
    assert_eq!(Running.name, "running");
    assert!(!Running.is_missing);
}

#[test]
fn test_restarting_container_ref() {
    assert_eq!(Restarting.name, "restarting");
    assert!(!Restarting.is_missing);
}

#[test]
fn test_net_consumer_ok() {
    assert_eq!(NetConsumerOK.name, "net_consumer");
    assert!(!NetConsumerOK.is_missing);
}

#[test]
fn test_net_consumer_invalid_supplier() {
    assert_eq!(NetConsumerInvalidSupplier.name, "net_consumer-missing_supplier");
    assert!(!NetConsumerInvalidSupplier.is_missing);
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_found_status() {
    // Verify the FoundStatus constants are correctly defined
    assert!(Found);
    assert!(!Missing);
}

#[test]
fn test_net_supplier_not_found_id() {
    assert_eq!(
        NetSupplierNotFoundID,
        "badc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc"
    );
}

#[test]
fn test_net_supplier_container_name() {
    assert_eq!(NetSupplierContainerName, "/wt-contnet-producer-1");
}
