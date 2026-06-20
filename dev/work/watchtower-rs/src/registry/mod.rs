#![forbid(unsafe_code)]

//! Registry helper surface used by the Rust migration.
//!
//! The module stays small and explicit so the HTTP-auth and digest slices can
//! land independently without forcing unrelated wiring changes.

pub mod helpers;
pub mod manifest;
pub mod auth;
pub mod digest;
pub mod credentials;
pub mod trust;
pub mod pull;
