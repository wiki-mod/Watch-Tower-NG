#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

/// Result type returned by the manifest URL builder.
pub type Result<T> = std::result::Result<T, ManifestUrlError>;

/// Errors that can occur while building a registry manifest URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestUrlError {
    /// The image reference was empty after trimming whitespace.
    EmptyImageRef,
    /// The image reference did not contain an explicit tag.
    UntaggedImageRef { image_ref: String },
    /// The image reference could not be split into registry and repository.
    InvalidImageRef { image_ref: String, reason: String },
}

impl fmt::Display for ManifestUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImageRef => f.write_str("image reference must not be empty"),
            Self::UntaggedImageRef { image_ref } => {
                write!(f, "parsed container image ref has no tag: {image_ref}")
            }
            Self::InvalidImageRef { image_ref, reason } => {
                write!(f, "invalid image reference {image_ref:?}: {reason}")
            }
        }
    }
}

impl Error for ManifestUrlError {}

/// Build the Docker registry manifest URL for a container image reference.
///
/// The resulting URL follows the legacy Watchtower convention:
/// `https://<registry-host>/v2/<repository-path>/manifests/<tag>`
pub fn build_manifest_url(image_ref: &str) -> Result<String> {
    let image_ref = image_ref.trim();
    if image_ref.is_empty() {
        return Err(ManifestUrlError::EmptyImageRef);
    }

    let (name_ref, tag) = split_tag(image_ref)?;
    let (host, path) = split_registry_and_path(name_ref)?;

    Ok(format!("https://{host}/v2/{path}/manifests/{tag}"))
}

fn split_tag(image_ref: &str) -> Result<(&str, &str)> {
    let name_ref = image_ref.split_once('@').map_or(image_ref, |(left, _)| left);
    let slash_pos = name_ref.rfind('/');
    let tag_pos = name_ref.rfind(':');

    if let Some(tag_pos) = tag_pos {
        if slash_pos.is_none_or(|slash_pos| tag_pos > slash_pos) {
            let tag = &name_ref[tag_pos + 1..];
            if tag.is_empty() {
                return Err(ManifestUrlError::UntaggedImageRef {
                    image_ref: image_ref.to_string(),
                });
            }

            return Ok((&name_ref[..tag_pos], tag));
        }
    }

    Err(ManifestUrlError::UntaggedImageRef {
        image_ref: image_ref.to_string(),
    })
}

fn split_registry_and_path(image_ref: &str) -> Result<(String, String)> {
    let (host, path) = if let Some((registry, remainder)) = image_ref.split_once('/') {
        if is_registry_component(registry) {
            (normalize_registry_host(registry), remainder.to_string())
        } else {
            ("index.docker.io".to_string(), image_ref.to_string())
        }
    } else {
        ("index.docker.io".to_string(), format!("library/{image_ref}"))
    };

    let normalized_path = if host == "index.docker.io" {
        normalize_docker_hub_path(&path)
    } else {
        path
    };

    if normalized_path.is_empty() {
        return Err(ManifestUrlError::InvalidImageRef {
            image_ref: image_ref.to_string(),
            reason: "repository path is missing".to_string(),
        });
    }

    Ok((host, normalized_path))
}

fn is_registry_component(component: &str) -> bool {
    component.contains('.') || component.contains(':') || component == "localhost"
}

fn normalize_registry_host(host: &str) -> String {
    if host == "docker.io" {
        "index.docker.io".to_string()
    } else {
        host.to_string()
    }
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
    fn rejects_untagged_image_refs() {
        let err = build_manifest_url("docker-registry.domain/imagename").unwrap_err();

        assert_eq!(
            err,
            ManifestUrlError::UntaggedImageRef {
                image_ref: "docker-registry.domain/imagename".to_string(),
            }
        );
    }
}
