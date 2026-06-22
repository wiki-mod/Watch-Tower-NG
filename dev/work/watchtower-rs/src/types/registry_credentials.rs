#![forbid(unsafe_code)]

//! RegistryCredentials struct for registry basic auth.
//!
//! Translated from `old-source/pkg/types/registry_credentials.go`.

use serde::Deserialize;

/// Credentials for registry authentication.
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryCredentials {
    pub username: String,
    pub password: String,
}
