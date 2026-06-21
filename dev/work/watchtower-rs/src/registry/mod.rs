#![forbid(unsafe_code)]

//! Registry helper surface translated from `old-source/pkg/registry/registry.go`.

pub mod auth;
pub mod credentials;
pub mod digest;
pub mod helpers;
pub mod manifest;
pub mod pull;
pub mod trust;

use tracing::debug;

use crate::types::FilterableContainer;

pub type AuthHandler = pull::AuthHandler;
pub type PullOptions = pull::PullOptions;

/// Return pull options for the provided image reference.
pub fn get_pull_options(image_name: &str) -> credentials::Result<PullOptions> {
    let registry_auth = credentials::encoded_auth(image_name);
    debug!(image = %image_name, "Got image name");
    let registry_auth = registry_auth?;

    if registry_auth.is_empty() {
        return Ok(PullOptions::default());
    }

    Ok(PullOptions {
        registry_auth,
        privilege_func: Some(default_auth_handler),
    })
}

/// Retry a rejected auth attempt without sending registry credentials.
#[must_use]
pub fn default_auth_handler() -> String {
    debug!("Authentication request was rejected. Trying again without authentication");
    String::new()
}

/// Return whether a failed digest HEAD request should trigger a warning.
#[must_use]
pub fn warn_on_api_consumption(container: &impl FilterableContainer) -> bool {
    let container_host = match helpers::get_registry_address(container.image_name()) {
        Ok(container_host) => container_host,
        Err(_) => return true,
    };

    container_host == helpers::DEFAULT_REGISTRY_HOST || container_host == "ghcr.io"
}
