#![forbid(unsafe_code)]

//! Registry helper surface translated from `old-source/pkg/registry/registry.go`.

pub mod auth;
pub mod digest;
pub mod helpers;
pub mod manifest;
pub mod pull;
mod registry;
pub mod trust;

// Re-export public API from registry module
pub use registry::{default_auth_handler, get_pull_options, warn_on_api_consumption};
pub use registry::{AuthHandler, PullOptions};
