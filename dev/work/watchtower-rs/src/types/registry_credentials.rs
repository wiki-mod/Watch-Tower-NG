#![forbid(unsafe_code)]

//! RegistryCredentials struct for registry basic auth.
//!
//! Translated from `old-source/pkg/types/registry_credentials.go`.

use serde::{Deserialize, Serialize};

/// Credentials for registry authentication.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryCredentials {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}
