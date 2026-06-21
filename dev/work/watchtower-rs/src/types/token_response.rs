#![forbid(unsafe_code)]

//! TokenResponse struct returned by registry auth endpoints.
//!
//! Translated from `old-source/pkg/types/token_response.go`.

use serde::{Deserialize, Serialize};

/// Token payload returned by registry auth endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub token: String,
}
