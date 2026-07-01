#![forbid(unsafe_code)]

use thiserror::Error;
use tracing::debug;

use crate::registry::helpers;
use crate::types::Container;

/// Result type returned by the manifest URL builder.
pub type Result<T> = std::result::Result<T, ManifestUrlError>;

/// Errors that can occur while building a registry manifest URL.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ManifestUrlError {
    /// The image reference could not be interpreted as a valid Docker reference.
    #[error("invalid image reference {image_ref:?}: {reason}")]
    InvalidImageReference { image_ref: String, reason: String },
}

/// Build the Docker registry manifest URL for a container.
///
/// The resulting URL follows the legacy Watchtower convention:
/// `https://<registry-host>/v2/<repository-path>/manifests/<tag>`
///
/// Mirrors Go's `BuildManifestURL` from `old-source/pkg/registry/manifest/manifest.go`.
pub fn build_manifest_url(container: &impl Container) -> Result<String> {
    build_manifest_url_from_ref(container.image_name())
}

/// Build the Docker registry manifest URL for an image reference string.
///
/// This is an internal helper function that works with image reference strings
/// directly. The public `build_manifest_url` function that takes a `Container` is
/// the preferred API and mirrors the Go signature.
pub(crate) fn build_manifest_url_from_ref(image_ref: &str) -> Result<String> {
    let image_ref = validate_image_reference(image_ref)?;
    let (name_ref, tag) = split_tag(image_ref)?;
    let (host, path) = split_registry_and_path(name_ref)?;

    debug!(
        image = %path,
        tag = %tag,
        normalized = %name_ref,
        host = %host,
        "Parsing image ref"
    );

    Ok(format!("https://{host}/v2/{path}/manifests/{tag}"))
}

fn validate_image_reference(image_ref: &str) -> Result<&str> {
    if image_ref.is_empty() {
        return Err(ManifestUrlError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: "image reference must not be empty".to_string(),
        });
    }

    if image_ref.chars().any(char::is_whitespace) {
        return Err(ManifestUrlError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: "invalid reference format".to_string(),
        });
    }

    Ok(image_ref)
}

fn split_tag(image_ref: &str) -> Result<(&str, &str)> {
    if image_ref.contains('@') {
        return Err(ManifestUrlError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: "invalid reference format".to_string(),
        });
    }

    let slash_pos = image_ref.rfind('/');
    let tag_pos = image_ref.rfind(':');

    if let Some(tag_pos) = tag_pos {
        if slash_pos.is_none_or(|slash_pos| tag_pos > slash_pos) {
            let tag = &image_ref[tag_pos + 1..];
            if tag.is_empty() {
                return Err(ManifestUrlError::InvalidImageReference {
                    image_ref: image_ref.to_string(),
                    reason: "invalid reference format".to_string(),
                });
            }

            return Ok((&image_ref[..tag_pos], tag));
        }
    }

    Ok((image_ref, "latest"))
}

fn split_registry_and_path(image_ref: &str) -> Result<(String, String)> {
    let host = helpers::get_registry_address(image_ref).map_err(|err| {
        ManifestUrlError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: err.to_string(),
        }
    })?;

    let (host, path) = if let Some((registry, remainder)) = image_ref.split_once('/') {
        if is_registry_component(registry) {
            (host, remainder.to_string())
        } else {
            (host, image_ref.to_string())
        }
    } else {
        (host, format!("library/{image_ref}"))
    };

    if path.is_empty() || path.starts_with('/') || path.ends_with('/') || path.contains("//") {
        return Err(ManifestUrlError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: "invalid reference format".to_string(),
        });
    }

    let normalized_path = if host == "index.docker.io" {
        normalize_docker_hub_path(&path)
    } else {
        path
    };

    Ok((host, normalized_path))
}

fn is_registry_component(component: &str) -> bool {
    component.contains('.') || component.contains(':') || component == "localhost"
}

fn normalize_docker_hub_path(path: &str) -> String {
    if path.matches('/').count() == 0 {
        format!("library/{path}")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test container for validating manifest URL building.
    struct TestContainer {
        image_name: String,
    }

    impl TestContainer {
        fn new(image_name: &str) -> Self {
            Self {
                image_name: image_name.to_string(),
            }
        }
    }

    impl Container for TestContainer {
        fn container_info(&self) -> Option<&crate::container::ContainerInspect> {
            None
        }

        fn id(&self) -> &crate::types::ContainerID {
            unimplemented!()
        }

        fn is_running(&self) -> bool {
            unimplemented!()
        }

        fn name(&self) -> &str {
            unimplemented!()
        }

        fn image_id(&self) -> &crate::types::ImageID {
            unimplemented!()
        }

        fn safe_image_id(&self) -> crate::types::ImageID {
            unimplemented!()
        }

        fn image_name(&self) -> &str {
            &self.image_name
        }

        fn enabled(&self) -> (bool, bool) {
            unimplemented!()
        }

        fn is_monitor_only(&self, _: &crate::types::UpdateParams) -> bool {
            unimplemented!()
        }

        fn scope(&self) -> (Option<&str>, bool) {
            unimplemented!()
        }

        fn links(&self) -> &[String] {
            unimplemented!()
        }

        fn to_restart(&self) -> bool {
            unimplemented!()
        }

        fn is_watchtower(&self) -> bool {
            unimplemented!()
        }

        fn stop_signal(&self) -> &str {
            unimplemented!()
        }

        fn has_image_info(&self) -> bool {
            unimplemented!()
        }

        fn image_info(&self) -> Option<&crate::container::ImageInspect> {
            unimplemented!()
        }

        fn get_lifecycle_pre_check_command(&self) -> &str {
            unimplemented!()
        }

        fn get_lifecycle_post_check_command(&self) -> &str {
            unimplemented!()
        }

        fn get_lifecycle_pre_update_command(&self) -> &str {
            unimplemented!()
        }

        fn get_lifecycle_post_update_command(&self) -> &str {
            unimplemented!()
        }

        fn verify_configuration(&self) -> crate::Result<()> {
            unimplemented!()
        }

        fn set_stale(&mut self, _: bool) {
            unimplemented!()
        }

        fn is_stale(&self) -> bool {
            unimplemented!()
        }

        fn is_no_pull(&self, _: &crate::types::UpdateParams) -> bool {
            unimplemented!()
        }

        fn set_linked_to_restarting(&mut self, _: bool) {
            unimplemented!()
        }

        fn is_linked_to_restarting(&self) -> bool {
            unimplemented!()
        }

        fn pre_update_timeout(&self) -> i32 {
            unimplemented!()
        }

        fn post_update_timeout(&self) -> i32 {
            unimplemented!()
        }

        fn is_restarting(&self) -> bool {
            unimplemented!()
        }

        fn get_create_config(&self) -> Option<&crate::container::ContainerConfig> {
            unimplemented!()
        }

        fn get_create_host_config(&self) -> Option<&crate::container::HostConfig> {
            unimplemented!()
        }
    }

    #[test]
    fn builds_manifest_url_for_fully_qualified_image() {
        let container = TestContainer::new("ghcr.io/marrrrrrrrry/watchtower:mytag");
        let url = build_manifest_url(&container).unwrap();

        assert_eq!(
            url,
            "https://ghcr.io/v2/marrrrrrrrry/watchtower/manifests/mytag"
        );
    }

    #[test]
    fn assumes_docker_hub_for_implicit_registry_and_single_segment_repo() {
        let container = TestContainer::new("watchtower:latest");
        let url = build_manifest_url(&container).unwrap();

        assert_eq!(
            url,
            "https://index.docker.io/v2/library/watchtower/manifests/latest"
        );
    }

    #[test]
    fn assumes_docker_hub_for_implicit_registry_and_multi_segment_repo() {
        let container = TestContainer::new("marrrrrrrrry/watchtower:latest");
        let url = build_manifest_url(&container).unwrap();

        assert_eq!(
            url,
            "https://index.docker.io/v2/marrrrrrrrry/watchtower/manifests/latest"
        );
    }

    #[test]
    fn normalizes_docker_io_to_index_docker_io() {
        let container = TestContainer::new("docker.io/watchtower:latest");
        let url = build_manifest_url(&container).unwrap();

        assert_eq!(
            url,
            "https://index.docker.io/v2/library/watchtower/manifests/latest"
        );
    }

    #[test]
    fn uses_registry_host_without_library_prefix_for_explicit_registry() {
        let container = TestContainer::new("docker-registry.domain/imagename:latest");
        let url = build_manifest_url(&container).unwrap();

        assert_eq!(
            url,
            "https://docker-registry.domain/v2/imagename/manifests/latest"
        );
    }

    #[test]
    fn rejects_pinned_image_refs() {
        let container = TestContainer::new(
            "docker-registry.domain/imagename@sha256:daf7034c5c89775afe3008393ae033529913548243b84926931d7c84398ecda7",
        );
        let err = build_manifest_url(&container).expect_err("pinned images should be rejected");

        assert!(matches!(
            err,
            ManifestUrlError::InvalidImageReference { .. }
        ));
    }
}
