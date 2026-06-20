#![forbid(unsafe_code)]

//! Library entrypoint for the Watchtower rewrite.
//!
//! This crate starts with a narrow surface so later work can add:
//! - CLI parsing
//! - config loading
//! - container lifecycle logic
//! - scheduling / polling

use std::error::Error as StdError;
use std::fmt;

/// Shared result type for the library.
pub type Result<T> = std::result::Result<T, Error>;

/// Minimal error type for the initial skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The current configuration is not usable.
    InvalidConfig(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(f, "invalid config: {message}"),
        }
    }
}

impl StdError for Error {}

/// Inputs for the application.
///
/// This is intentionally empty for now so future agents can add fields without
/// reshaping the library API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {}

/// In-memory application handle.
///
/// The binary can construct this from parsed CLI/config data and then call
/// [`WatchtowerApp::run`] or the convenience [`run`] function below.
#[derive(Debug, Clone)]
pub struct WatchtowerApp {
    config: AppConfig,
}

impl WatchtowerApp {
    /// Create a new application instance from configuration.
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    /// Execute the application.
    ///
    /// The real container and scheduling work will land here later; for now the
    /// method validates the shape of the entrypoint and exits cleanly.
    pub fn run(&self) -> Result<()> {
        self.validate()?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let _ = &self.config;
        Ok(())
    }
}

/// Convenience entrypoint for the binary crate.
pub fn run(config: AppConfig) -> Result<()> {
    WatchtowerApp::new(config).run()
}
