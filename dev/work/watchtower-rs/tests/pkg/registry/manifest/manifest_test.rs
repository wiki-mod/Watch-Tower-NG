#![forbid(unsafe_code)]

use watchtower_rs::registry::manifest::{build_manifest_url, Result};

fn build_mock_container_manifest_url(image_ref: &str) -> Result<String> {
    build_manifest_url(image_ref)
}

#[test]
fn should_return_a_valid_url_given_a_fully_qualified_image() {
    let image_ref = "ghcr.io/marrrrrrrrry/watchtower:mytag";
    let expected = "https://ghcr.io/v2/marrrrrrrrry/watchtower/manifests/mytag";

    let url = build_mock_container_manifest_url(image_ref).expect("expected manifest url");

    assert_eq!(url, expected);
}

#[test]
fn should_assume_docker_hub_for_image_refs_with_no_explicit_registry() {
    let image_ref = "marrrrrrrrry/watchtower:latest";
    let expected = "https://index.docker.io/v2/marrrrrrrrry/watchtower/manifests/latest";

    let url = build_mock_container_manifest_url(image_ref).expect("expected manifest url");

    assert_eq!(url, expected);
}

#[test]
fn should_assume_latest_for_image_refs_with_no_explicit_tag() {
    let image_ref = "marrrrrrrrry/watchtower";
    let expected = "https://index.docker.io/v2/marrrrrrrrry/watchtower/manifests/latest";

    let url = build_mock_container_manifest_url(image_ref).expect("expected manifest url");

    assert_eq!(url, expected);
}

#[test]
fn should_not_prepend_library_for_single_part_container_names_in_registries_other_than_docker_hub() {
    let image_ref = "docker-registry.domain/imagename:latest";
    let expected = "https://docker-registry.domain/v2/imagename/manifests/latest";

    let url = build_mock_container_manifest_url(image_ref).expect("expected manifest url");

    assert_eq!(url, expected);
}

#[test]
fn should_throw_an_error_on_pinned_images() {
    let image_ref = "docker-registry.domain/imagename@sha256:daf7034c5c89775afe3008393ae033529913548243b84926931d7c84398ecda7";

    let result = build_mock_container_manifest_url(image_ref);

    assert!(result.is_err());
}
