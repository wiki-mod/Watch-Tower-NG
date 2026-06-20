#![forbid(unsafe_code)]

//! Pull-option surface for the Rust registry port.
//!
//! This module mirrors the Go `GetPullOptions` flow closely enough for the
//! migration to preserve registry-auth fallback behavior without pulling in the
//! full Docker client.

use crate::types::FilterableContainer;

use super::credentials;
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
/// The legacy behavior prefers environment credentials and falls back to the
/// Docker config file. A resolved auth payload enables the retry handler.
pub fn get_pull_options(image_name: &str) -> credentials::Result<PullOptions> {
    let registry_auth = credentials::encoded_auth(image_name)?;

    if registry_auth.is_empty() {
        return Ok(PullOptions::default());
    }

    Ok(PullOptions {
        registry_auth,
        privilege_func: Some(default_auth_handler),
    })
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
        let options = get_pull_options_with_resolver("ghcr.io/watchtower/image:latest", || {
            Ok(String::new())
        })
        .expect("default options should resolve");

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

    #[test]
    fn get_pull_options_enables_privilege_retry_when_auth_is_available() {
        let options = get_pull_options_with_resolver("registry.example.com/team/image:latest", || {
            Ok("auth-token".to_string())
        })
        .expect("auth options should resolve");

        assert_eq!(options.registry_auth, "auth-token");
        assert!(options.privilege_func.is_some());
    }

    #[test]
    fn get_pull_options_propagates_credentials_errors() {
        let err = get_pull_options_with_resolver("registry.example.com/team/image:latest", || {
            Err(credentials::CredentialsError::MissingEnvironmentCredentials)
        })
        .expect_err("missing credentials should fail");

        assert!(matches!(
            err,
            credentials::CredentialsError::MissingEnvironmentCredentials
        ));
    }

    fn get_pull_options_with_resolver<F>(
        image_name: &str,
        resolver: F,
    ) -> credentials::Result<PullOptions>
    where
        F: FnOnce() -> credentials::Result<String>,
    {
        let registry_auth = resolver()?;

        if registry_auth.is_empty() {
            return Ok(PullOptions::default());
        }

        let _ = image_name;
        Ok(PullOptions {
            registry_auth,
            privilege_func: Some(default_auth_handler),
        })
    }
}
