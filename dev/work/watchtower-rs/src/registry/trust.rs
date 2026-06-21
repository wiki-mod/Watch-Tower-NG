#![forbid(unsafe_code)]

use thiserror::Error;

use super::helpers;

/// Result type used by the registry trust helper.
pub type Result<T> = std::result::Result<T, TrustError>;

/// Trust decision derived from the registry host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    /// The caller should warn because the registry is expected to use API
    /// consumption in a way the legacy implementation treated as special.
    WarnOnApiConsumption,
    /// The caller can proceed without a warning.
    NoWarning,
}

/// Errors raised while deriving a trust decision.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustError {
    /// The image reference could not be resolved to a registry.
    #[error(transparent)]
    Registry(#[from] helpers::RegistryError),
}

/// Return the legacy trust decision for an image reference.
///
/// The Go implementation treated Docker Hub and GHCR as special cases and
/// warned on API consumption for those registries. All other explicit
/// registries are treated as non-warning cases.
pub fn trust_decision(image_ref: &str) -> Result<TrustDecision> {
    let registry = helpers::get_registry_address(image_ref)?;

    if registry == helpers::DEFAULT_REGISTRY_HOST || registry == "ghcr.io" {
        return Ok(TrustDecision::WarnOnApiConsumption);
    }

    Ok(TrustDecision::NoWarning)
}

/// Return whether the legacy runtime should warn on registry API consumption.
pub fn warn_on_api_consumption(image_ref: &str) -> Result<bool> {
    Ok(matches!(
        trust_decision(image_ref)?,
        TrustDecision::WarnOnApiConsumption
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_for_legacy_docker_hub_and_ghcr_registry_references() {
        assert_eq!(
            trust_decision("ubuntu").expect("should resolve"),
            TrustDecision::WarnOnApiConsumption
        );
        assert_eq!(
            trust_decision("docker.io/library/nginx:latest").expect("should resolve"),
            TrustDecision::WarnOnApiConsumption
        );
        assert_eq!(
            trust_decision("index.docker.io/library/nginx").expect("should resolve"),
            TrustDecision::WarnOnApiConsumption
        );
        assert_eq!(
            trust_decision("ghcr.io/watchtower/image:main").expect("should resolve"),
            TrustDecision::WarnOnApiConsumption
        );
    }

    #[test]
    fn keeps_other_registry_hosts_out_of_the_warning_path() {
        assert_eq!(
            trust_decision("registry.example.com/team/image:latest").expect("should resolve"),
            TrustDecision::NoWarning
        );
        assert_eq!(
            trust_decision("localhost:5000/team/image@sha256:deadbeef").expect("should resolve"),
            TrustDecision::NoWarning
        );
        assert_eq!(
            trust_decision("[2001:db8::1]:5000/repo/image:latest").expect("should resolve"),
            TrustDecision::NoWarning
        );
    }

    #[test]
    fn handles_digest_references_the_same_way_as_tagged_references() {
        assert_eq!(
            trust_decision("ghcr.io/watchtower/image@sha256:deadbeef").expect("should resolve"),
            TrustDecision::WarnOnApiConsumption
        );
        assert_eq!(
            trust_decision("registry.example.com/team/image@sha256:deadbeef")
                .expect("should resolve"),
            TrustDecision::NoWarning
        );
    }

    #[test]
    fn exposes_registry_helper_errors_explicitly() {
        assert!(matches!(
            trust_decision(""),
            Err(TrustError::Registry(helpers::RegistryError::EmptyReference))
        ));
        assert!(matches!(
            trust_decision(" ubuntu"),
            Err(TrustError::Registry(helpers::RegistryError::InvalidReference(value))) if value == " ubuntu"
        ));
    }

    #[test]
    fn boolean_wrapper_matches_the_trust_decision() {
        assert!(warn_on_api_consumption("docker.io/library/nginx:latest").expect("should resolve"));
        assert!(
            !warn_on_api_consumption("registry.example.com/team/image:latest")
                .expect("should resolve")
        );
    }
}
