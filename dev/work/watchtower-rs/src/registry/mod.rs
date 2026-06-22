#![forbid(unsafe_code)]

//! Registry helper surface translated from `old-source/pkg/registry/registry.go`.

pub mod auth;
pub mod digest;
pub mod helpers;
pub mod manifest;
pub mod pull;
mod core;
pub mod trust;

// Re-export public API from core module
pub use core::{default_auth_handler, get_pull_options, warn_on_api_consumption};
pub use core::{AuthHandler, PullOptions};
