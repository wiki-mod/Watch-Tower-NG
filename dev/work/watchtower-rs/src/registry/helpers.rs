use thiserror::Error;

/// Canonical Docker Hub registry domain used by normalized image references.
pub const DEFAULT_REGISTRY_DOMAIN: &str = "docker.io";
/// Legacy Docker Hub host returned by the historical Go helper.
pub const DEFAULT_REGISTRY_HOST: &str = "index.docker.io";
/// Legacy Docker Hub domain accepted by older image references.
pub const LEGACY_DEFAULT_REGISTRY_DOMAIN: &str = "index.docker.io";

/// Result type used by the registry helper.
pub type Result<T> = std::result::Result<T, RegistryError>;

/// Errors raised while deriving a registry address from an image reference.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    /// The caller passed an empty or whitespace-only image reference.
    #[error("image reference must not be empty")]
    EmptyReference,
    /// The input cannot be interpreted as a Docker image reference.
    #[error("invalid image reference `{0}`")]
    InvalidReference(String),
}

/// Return the registry address for an image reference.
///
/// Docker Hub is special: the normalized domain is `docker.io`, but the
/// historical registry endpoint used by Watchtower is `index.docker.io`.
pub fn get_registry_address(image_ref: &str) -> Result<String> {
    let normalized = normalize_image_reference(image_ref)?;
    let registry = registry_domain(&normalized)?;

    if registry == DEFAULT_REGISTRY_DOMAIN {
        return Ok(DEFAULT_REGISTRY_HOST.to_string());
    }

    Ok(registry.to_string())
}

fn normalize_image_reference(image_ref: &str) -> Result<&str> {
    let trimmed = image_ref.trim();

    if trimmed.is_empty() {
        return Err(RegistryError::EmptyReference);
    }

    if trimmed != image_ref {
        return Err(RegistryError::InvalidReference(image_ref.to_string()));
    }

    Ok(trimmed)
}

fn registry_domain(image_ref: &str) -> Result<&str> {
    let without_digest = image_ref.split_once('@').map_or(image_ref, |(name, _)| name);

    let Some((first_segment, _)) = without_digest.split_once('/') else {
        return Ok(DEFAULT_REGISTRY_DOMAIN);
    };

    if first_segment.is_empty() {
        return Err(RegistryError::InvalidReference(image_ref.to_string()));
    }

    // Docker treats the first path segment as a registry only when it looks
    // like a host name. Otherwise the reference is assumed to live on Docker
    // Hub, and we return the legacy host expected by the rest of the runtime.
    if is_registry_candidate(first_segment) {
        validate_registry_candidate(first_segment, image_ref)?;
        return Ok(first_segment);
    }

    Ok(DEFAULT_REGISTRY_DOMAIN)
}

fn is_registry_candidate(segment: &str) -> bool {
    segment == "localhost" || segment.contains('.') || segment.contains(':')
}

fn validate_registry_candidate(candidate: &str, original: &str) -> Result<()> {
    if candidate.starts_with('[') {
        validate_bracketed_host(candidate, original)?;
        return Ok(());
    }

    let colon_count = candidate.matches(':').count();
    if colon_count > 1 {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    if let Some((host, port)) = candidate.rsplit_once(':') {
        if host.is_empty() || port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(RegistryError::InvalidReference(original.to_string()));
        }
        return Ok(());
    }

    if candidate.contains('/') || candidate.contains('@') || candidate.contains(' ') {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    Ok(())
}

fn validate_bracketed_host(candidate: &str, original: &str) -> Result<()> {
    let closing = candidate
        .find(']')
        .ok_or_else(|| RegistryError::InvalidReference(original.to_string()))?;

    let host = &candidate[1..closing];
    if host.is_empty() {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    let remainder = &candidate[closing + 1..];
    if remainder.is_empty() {
        return Ok(());
    }

    let port = remainder
        .strip_prefix(':')
        .ok_or_else(|| RegistryError::InvalidReference(original.to_string()))?;

    if port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_legacy_docker_hub_host_for_unqualified_images() {
        assert_eq!(
            get_registry_address("ubuntu").expect("should resolve"),
            DEFAULT_REGISTRY_HOST
        );
        assert_eq!(
            get_registry_address("library/alpine:3.20").expect("should resolve"),
            DEFAULT_REGISTRY_HOST
        );
    }

    #[test]
    fn maps_canonical_docker_hub_references_to_legacy_host() {
        assert_eq!(
            get_registry_address("docker.io/library/nginx:latest").expect("should resolve"),
            DEFAULT_REGISTRY_HOST
        );
        assert_eq!(
            get_registry_address("index.docker.io/library/nginx").expect("should resolve"),
            LEGACY_DEFAULT_REGISTRY_DOMAIN
        );
    }

    #[test]
    fn keeps_non_docker_hub_registry_addresses_intact() {
        assert_eq!(
            get_registry_address("ghcr.io/watchtower/image:main").expect("should resolve"),
            "ghcr.io"
        );
        assert_eq!(
            get_registry_address("localhost:5000/team/image@sha256:deadbeef")
                .expect("should resolve"),
            "localhost:5000"
        );
        assert_eq!(
            get_registry_address("[2001:db8::1]:5000/repo/image").expect("should resolve"),
            "[2001:db8::1]:5000"
        );
    }

    #[test]
    fn treats_bare_names_as_docker_hub_references() {
        assert_eq!(
            get_registry_address("ghcr.io").expect("should resolve"),
            DEFAULT_REGISTRY_HOST
        );
        assert_eq!(
            get_registry_address("localhost").expect("should resolve"),
            DEFAULT_REGISTRY_HOST
        );
    }

    #[test]
    fn rejects_blank_or_whitespace_padded_references() {
        assert_eq!(
            get_registry_address("").expect_err("should reject"),
            RegistryError::EmptyReference
        );
        assert!(matches!(
            get_registry_address(" ubuntu"),
            Err(RegistryError::InvalidReference(value)) if value == " ubuntu"
        ));
    }

    #[test]
    fn rejects_schemed_or_malformed_registry_candidates() {
        assert!(matches!(
            get_registry_address("http://ghcr.io/image"),
            Err(RegistryError::InvalidReference(value)) if value == "http://ghcr.io/image"
        ));
        assert!(matches!(
            get_registry_address("ghcr.io:tag/image"),
            Err(RegistryError::InvalidReference(value)) if value == "ghcr.io:tag/image"
        ));
    }
}
