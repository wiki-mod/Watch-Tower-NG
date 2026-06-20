#![forbid(unsafe_code)]

use watchtower_rs::registry::helpers::get_registry_address;

#[test]
fn test_helpers_returns_error_if_passed_empty_string() {
    assert!(get_registry_address("").is_err());
}

#[test]
fn test_helpers_returns_index_docker_io_for_image_refs_with_no_explicit_registry() {
    assert_eq!(
        get_registry_address("watchtower").expect("should resolve"),
        "index.docker.io"
    );
    assert_eq!(
        get_registry_address("marrrrrrrrry/watchtower").expect("should resolve"),
        "index.docker.io"
    );
}

#[test]
fn test_helpers_returns_index_docker_io_for_image_refs_with_docker_io_domain() {
    assert_eq!(
        get_registry_address("docker.io/watchtower").expect("should resolve"),
        "index.docker.io"
    );
    assert_eq!(
        get_registry_address("docker.io/marrrrrrrrry/watchtower").expect("should resolve"),
        "index.docker.io"
    );
}

#[test]
fn test_helpers_returns_the_host_if_passed_an_image_name_containing_a_local_host() {
    assert_eq!(
        get_registry_address("henk:80/watchtower").expect("should resolve"),
        "henk:80"
    );
    assert_eq!(
        get_registry_address("localhost/watchtower").expect("should resolve"),
        "localhost"
    );
}

#[test]
fn test_helpers_returns_the_server_address_if_passed_a_fully_qualified_image_name() {
    assert_eq!(
        get_registry_address("github.com/containrrr/config").expect("should resolve"),
        "github.com"
    );
}
