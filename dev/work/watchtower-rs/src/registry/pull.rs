#![forbid(unsafe_code)]

//! Pull-option surface for the Rust registry port.
//!
//! The credentials path is intentionally not implemented in this slice. The
//! module keeps the Go-shaped API surface and wires warning decisions through
//! the existing registry helper and trust logic.

use crate::types::FilterableContainer;

use super::trust;

/// Signature used for retrying a pull without authentication.
pub type AuthHandler = fn() -> String;

/// Minimal pull options used by the registry client slice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PullOptions {
    /// Base64-encoded registry auth payload, when available.
    pub registry_auth: String,
    /// Handler used when the registry rejects the first authenticated request.
    pub privilege_func: Option<AuthHandler>,
}

/// Return the pull options for an image reference.
///
/// This port does not ship registry credential lookup yet, so the function
/// preserves the call shape and returns the default pull options.
#[must_use]
pub fn get_pull_options(_image_name: &str) -> PullOptions {
    PullOptions::default()
}

/// Retry handler used after a registry rejects the authenticated request.
///
/// The legacy Go implementation logged a retry without auth and returned an
/// empty header value. The Rust port keeps that behavior without introducing a
/// second credentials source.
#[must_use]
pub fn default_auth_handler() -> String {
    tracing::debug!("Authentication request was rejected. Trying again without authentication");
    String::new()
}

/// Return whether the registry is expected to warrant an API-consumption warning.
///
/// The fail-closed behavior matches the Go helper: malformed image references
/// or registry helper errors are treated as warning cases.
#[must_use]
pub fn warn_on_api_consumption(container: &impl FilterableContainer) -> bool {
    trust::warn_on_api_consumption(container.image_name()).unwrap_or(true)
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
    fn get_pull_options_returns_the_default_shape() {
        let options = get_pull_options("ghcr.io/watchtower/image:latest");

        assert_eq!(options.registry_auth, "");
        assert!(options.privilege_func.is_none());
    }

    #[test]
    fn default_auth_handler_returns_an_empty_retry_header() {
        assert_eq!(default_auth_handler(), "");
    }

    #[test]
    fn warns_for_docker_hub_and_ghcr_images() {
        assert!(warn_on_api_consumption(&TestContainer::new("ubuntu")));
        assert!(warn_on_api_consumption(&TestContainer::new(
            "docker.io/library/nginx:latest"
        )));
        assert!(warn_on_api_consumption(&TestContainer::new(
            "ghcr.io/watchtower/image:main"
        )));
    }

    #[test]
    fn does_not_warn_for_other_explicit_registries() {
        assert!(!warn_on_api_consumption(&TestContainer::new(
            "registry.example.com/team/image:latest"
        )));
        assert!(!warn_on_api_consumption(&TestContainer::new(
            "localhost:5000/team/image@sha256:deadbeef"
        )));
    }
}
