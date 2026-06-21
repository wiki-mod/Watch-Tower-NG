#![forbid(unsafe_code)]

use thiserror::Error;
use tracing::debug;

use crate::registry::helpers;

/// Result type returned by the manifest URL builder.
pub type Result<T> = std::result::Result<T, ManifestUrlError>;

/// Errors that can occur while building a registry manifest URL.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ManifestUrlError {
    /// The image reference could not be interpreted as a valid Docker reference.
    #[error("invalid image reference {image_ref:?}: {reason}")]
    InvalidImageReference { image_ref: String, reason: String },
}

/// Build the Docker registry manifest URL for an image reference.
///
/// The resulting URL follows the legacy Watchtower convention:
/// `https://<registry-host>/v2/<repository-path>/manifests/<tag>`
pub fn build_manifest_url(image_ref: &str) -> Result<String> {
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

    #[test]
    fn builds_manifest_url_for_fully_qualified_image() {
        let url = build_manifest_url("ghcr.io/marrrrrrrrry/watchtower:mytag").unwrap();

        assert_eq!(
            url,
            "https://ghcr.io/v2/marrrrrrrrry/watchtower/manifests/mytag"
        );
    }

    #[test]
    fn assumes_docker_hub_for_implicit_registry_and_single_segment_repo() {
        let url = build_manifest_url("watchtower:latest").unwrap();

        assert_eq!(
            url,
            "https://index.docker.io/v2/library/watchtower/manifests/latest"
        );
    }

    #[test]
    fn assumes_docker_hub_for_implicit_registry_and_multi_segment_repo() {
        let url = build_manifest_url("marrrrrrrrry/watchtower:latest").unwrap();

        assert_eq!(
            url,
            "https://index.docker.io/v2/marrrrrrrrry/watchtower/manifests/latest"
        );
    }

    #[test]
    fn normalizes_docker_io_to_index_docker_io() {
        let url = build_manifest_url("docker.io/watchtower:latest").unwrap();

        assert_eq!(
            url,
            "https://index.docker.io/v2/library/watchtower/manifests/latest"
        );
    }

    #[test]
    fn uses_registry_host_without_library_prefix_for_explicit_registry() {
        let url = build_manifest_url("docker-registry.domain/imagename:latest").unwrap();

        assert_eq!(
            url,
            "https://docker-registry.domain/v2/imagename/manifests/latest"
        );
    }

    #[test]
    fn rejects_pinned_image_refs() {
        let err = build_manifest_url(
            "docker-registry.domain/imagename@sha256:daf7034c5c89775afe3008393ae033529913548243b84926931d7c84398ecda7",
        )
        .expect_err("pinned images should be rejected");

        assert!(matches!(
            err,
            ManifestUrlError::InvalidImageReference { .. }
        ));
    }
}
