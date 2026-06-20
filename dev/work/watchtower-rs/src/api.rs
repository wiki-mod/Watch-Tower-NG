#![forbid(unsafe_code)]

//! Pure helpers for the legacy HTTP API token guard and startup gating.
//!
//! This module intentionally stops before any HTTP server binding. It keeps the
//! semantics that can be decided from inputs alone: authorization header
//! matching, handler registration state, and whether the API would start.

use std::error::Error as StdError;
use std::fmt;

const TOKEN_MISSING_MSG: &str = "api token is empty or has not been set. exiting";

/// Small snapshot of the API state used for startup decisions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApiStatus {
    pub has_handlers: bool,
    pub token_is_set: bool,
}

/// Result of evaluating whether the API should start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartDecision {
    /// No handlers were registered, so the API stays disabled.
    Skipped,
    /// The API is enabled and the server may be started elsewhere.
    Start { block: bool },
}

/// Errors produced by the startup gating helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    TokenMissing,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenMissing => f.write_str(TOKEN_MISSING_MSG),
        }
    }
}

impl StdError for ApiError {}

/// In-memory representation of the legacy API guard state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Api {
    token: String,
    has_handlers: bool,
}

impl Api {
    /// Create a new API state holder from the configured token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            has_handlers: false,
        }
    }

    /// Return the current state snapshot.
    pub fn status(&self) -> ApiStatus {
        ApiStatus {
            has_handlers: self.has_handlers,
            token_is_set: self.token_is_set(),
        }
    }

    /// Mark that at least one API handler was registered.
    pub fn mark_handler_registered(&mut self) {
        self.has_handlers = true;
    }

    /// Return the exact Bearer authorization header expected by the API.
    pub fn expected_authorization(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Check whether an incoming `Authorization` header is valid.
    pub fn authorize(&self, authorization: Option<&str>) -> bool {
        authorization.is_some_and(|value| {
            let prefix = "Bearer ";
            value.len() == prefix.len() + self.token.len()
                && value.starts_with(prefix)
                && &value[prefix.len()..] == self.token
        })
    }

    /// Decide whether the API should start.
    ///
    /// The API is skipped when no handler was registered. If handlers are
    /// present, the token must be configured before the caller can continue.
    /// The returned decision only describes whether the server would start and
    /// whether it should block; the actual HTTP server binding stays outside
    /// this module.
    pub fn start_decision(&self, block: bool) -> Result<StartDecision, ApiError> {
        if !self.has_handlers {
            return Ok(StartDecision::Skipped);
        }

        if !self.token_is_set() {
            return Err(ApiError::TokenMissing);
        }

        Ok(StartDecision::Start { block })
    }

    fn token_is_set(&self) -> bool {
        !self.token.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_accepts_matching_bearer_header() {
        let api = Api::new("secret-token");

        assert!(api.authorize(Some("Bearer secret-token")));
    }

    #[test]
    fn authorize_rejects_missing_or_wrong_header() {
        let api = Api::new("secret-token");

        assert!(!api.authorize(None));
        assert!(!api.authorize(Some("Bearer wrong")));
        assert!(!api.authorize(Some("Basic secret-token")));
    }

    #[test]
    fn status_reflects_handler_registration_and_token_presence() {
        let mut api = Api::new("secret-token");

        assert_eq!(
            api.status(),
            ApiStatus {
                has_handlers: false,
                token_is_set: true,
            }
        );

        api.mark_handler_registered();

        assert_eq!(
            api.status(),
            ApiStatus {
                has_handlers: true,
                token_is_set: true,
            }
        );
    }

    #[test]
    fn start_decision_skips_when_no_handlers_exist() {
        let api = Api::new("");

        assert_eq!(api.start_decision(true).unwrap(), StartDecision::Skipped);
        assert_eq!(api.start_decision(false).unwrap(), StartDecision::Skipped);
    }

    #[test]
    fn start_decision_requires_token_when_handlers_exist() {
        let mut api = Api::new("");
        api.mark_handler_registered();

        let err = api
            .start_decision(true)
            .expect_err("token should be required once handlers exist");

        assert_eq!(err, ApiError::TokenMissing);
        assert_eq!(err.to_string(), TOKEN_MISSING_MSG);
    }

    #[test]
    fn start_decision_preserves_block_flag_when_ready() {
        let mut api = Api::new("secret-token");
        api.mark_handler_registered();

        assert_eq!(
            api.start_decision(true).unwrap(),
            StartDecision::Start { block: true }
        );

        assert_eq!(
            api.start_decision(false).unwrap(),
            StartDecision::Start { block: false }
        );
    }

    #[test]
    fn expected_authorization_matches_legacy_bearer_format() {
        let api = Api::new("abc123");

        assert_eq!(api.expected_authorization(), "Bearer abc123");
    }
}
