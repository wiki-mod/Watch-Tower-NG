#![forbid(unsafe_code)]

//! Registry pull options and authentication surface.
//!
//! This module provides the public interface for obtaining pull options with
//! authentication credentials and determining whether an API consumption warning
//! should be logged for a given container.
//!
//! Translated from `old-source/pkg/registry/registry.go`.

use crate::types::FilterableContainer;

use super::pull;
use super::trust;

/// Alias to pull::AuthHandler for backward compatibility.
pub type AuthHandler = pull::AuthHandler;

/// Alias to pull::PullOptions for backward compatibility.
pub type PullOptions = pull::PullOptions;

/// Return pull options for the provided image reference.
///
/// This function resolves registry credentials from the environment or Docker
/// config file and returns pull options that can be used with a Docker client.
/// If credentials are available, the pull options include an auth handler that
/// can be invoked if the initial authenticated request is rejected.
///
/// # Errors
///
/// Returns an error if the Docker config file cannot be read or parsed, or if
/// environment credentials are in an invalid format.
pub fn get_pull_options(image_name: &str) -> trust::Result<PullOptions> {
    let auth = trust::encoded_auth(image_name)?;
    tracing::debug!(image = %image_name, "Got image name");

    if auth.is_empty() {
        return Ok(PullOptions::default());
    }

    Ok(PullOptions {
        registry_auth: auth,
        privilege_func: Some(default_auth_handler),
    })
}

/// Retry handler used when a registry rejects the authenticated request.
///
/// The legacy Go implementation logged a debug message and returned an empty
/// auth header to indicate a retry without authentication. This Rust port
/// maintains that behavior.
///
/// This function is used as a callback when the initial authenticated pull
/// attempt fails with an authentication error from the registry.
#[must_use]
pub fn default_auth_handler() -> String {
    tracing::debug!("Authentication request was rejected. Trying again without authentication");
    String::new()
}

/// Return whether a failed digest HEAD request should trigger a warning.
///
/// The legacy Go runtime treats Docker Hub (index.docker.io) and GHCR
/// (ghcr.io) as special cases where API consumption warnings are warranted.
/// For all other registries, no warning is logged.
///
/// This function returns `true` (warn) for known registries and for any
/// errors in parsing the container image name. It returns `false` (no warn)
/// only for explicitly known registries other than Docker Hub and GHCR.
///
/// Mirrors Go's `WarnOnAPIConsumption` in `old-source/pkg/registry/registry.go`.
///
/// # Parameters
///
/// * `container` - A container implementing FilterableContainer trait
///
/// # Returns
///
/// `true` if a warning should be issued, `false` otherwise.
#[must_use]
pub fn warn_on_api_consumption(container: &impl FilterableContainer) -> bool {
    let registry = match super::helpers::get_registry_address(container.image_name()) {
        Ok(addr) => addr,
        Err(_) => return true, // Fail closed: warn if parsing fails
    };

    matches!(
        registry.as_str(),
        "index.docker.io" | "registry-1.docker.io" | "ghcr.io"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
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

    impl FilterableContainer for TestContainer {
        fn name(&self) -> &str {
            "test"
        }

        fn is_watchtower(&self) -> bool {
            false
        }

        fn enabled(&self) -> (bool, bool) {
            (true, true)
        }

        fn scope(&self) -> Option<&str> {
            None
        }

        fn image_name(&self) -> &str {
            self.image_name.as_str()
        }
    }

    #[test]
    fn default_auth_handler_returns_empty_string() {
        assert_eq!(default_auth_handler(), "");
    }

    #[test]
    fn warn_on_api_consumption_returns_true_for_docker_hub() {
        assert!(warn_on_api_consumption(&TestContainer::new("ubuntu")));
        assert!(warn_on_api_consumption(&TestContainer::new(
            "docker.io/library/nginx:latest"
        )));
    }

    #[test]
    fn warn_on_api_consumption_returns_true_for_ghcr() {
        assert!(warn_on_api_consumption(&TestContainer::new(
            "ghcr.io/watchtower/image:main"
        )));
    }

    #[test]
    fn warn_on_api_consumption_returns_false_for_other_registries() {
        assert!(!warn_on_api_consumption(&TestContainer::new(
            "registry.example.com/team/image:latest"
        )));
    }

    #[test]
    fn warn_on_api_consumption_returns_true_on_parse_error() {
        assert!(warn_on_api_consumption(&TestContainer::new("")));
    }
}
