#![forbid(unsafe_code)]

//! Library entrypoint for the Watchtower rewrite.
//!
//! This crate keeps the public surface focused on application configuration,
//! validation, and the single orchestration entrypoint used by the binary.

use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

pub mod filters;
pub mod actions;
pub mod api;
pub mod api_metrics;
pub mod api_update;
pub mod cgroup;
pub mod container;
pub mod lifecycle;
pub mod metrics;
pub mod notifications;
pub mod sorter;
pub mod session;
pub mod registry;
pub mod types;

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
/// This is the target shape for the CLI parser output. The binary can build it
/// directly or convert a parser struct into it with `Into<AppConfig>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Run a single update pass and exit.
    pub run_once: bool,
    /// Only monitor containers and skip container updates.
    pub monitor_only: bool,
    /// Remove old images after a successful restart.
    pub cleanup: bool,
    /// Remove anonymous volumes during container replacement.
    pub remove_volumes: bool,
    /// Include stopped containers in the scan.
    pub include_stopped: bool,
    /// Restart stopped containers that were updated.
    pub revive_stopped: bool,
    /// Include restarting containers in the scan.
    pub include_restarting: bool,
    /// Enable rolling restarts during updates.
    pub rolling_restart: bool,
    /// Accept a cron-style schedule instead of a fixed poll interval.
    pub schedule: Option<String>,
    /// Poll interval used when no schedule is set.
    pub interval: Option<Duration>,
    /// Token for the HTTP API, when enabled.
    pub http_api_token: Option<String>,
    /// Enable the HTTP update endpoint.
    pub enable_http_update_api: bool,
    /// Enable the HTTP metrics endpoint.
    pub enable_http_metrics_api: bool,
    /// Allow the HTTP API to unblock periodic polls.
    pub unblock_http_api: bool,
    /// Optional container scope.
    pub scope: Option<String>,
    /// Skip the standard health check path and exit immediately.
    pub health_check: bool,
}

impl AppConfig {
    /// Build a config from CLI parser output or an already-normalized config.
    pub fn from_cli(config: impl Into<Self>) -> Self {
        config.into()
    }

    /// Validate obvious startup mistakes before orchestration starts.
    pub fn validate(&self) -> Result<()> {
        self.validate_schedule_and_interval()?;
        self.validate_container_flags()?;
        self.validate_runtime_flags()?;
        Ok(())
    }

    fn validate_schedule_and_interval(&self) -> Result<()> {
        if let Some(schedule) = self.schedule.as_deref() {
            if schedule.trim().is_empty() {
                return Err(Error::InvalidConfig(
                    "schedule must not be empty".to_string(),
                ));
            }
        }

        if matches!(self.interval, Some(interval) if interval.is_zero()) {
            return Err(Error::InvalidConfig(
                "interval must be greater than zero".to_string(),
            ));
        }

        if self.schedule.is_some() && self.interval.is_some() {
            return Err(Error::InvalidConfig(
                "schedule and interval are mutually exclusive".to_string(),
            ));
        }

        Ok(())
    }

    fn validate_container_flags(&self) -> Result<()> {
        if self.rolling_restart && self.monitor_only {
            return Err(Error::InvalidConfig(
                "rolling_restart cannot be combined with monitor_only".to_string(),
            ));
        }

        if self.revive_stopped && !self.include_stopped {
            return Err(Error::InvalidConfig(
                "revive_stopped requires include_stopped".to_string(),
            ));
        }

        if let Some(scope) = self.scope.as_deref() {
            if scope.trim().is_empty() {
                return Err(Error::InvalidConfig(
                    "scope must not be empty".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn validate_runtime_flags(&self) -> Result<()> {
        if let Some(token) = self.http_api_token.as_deref() {
            if token.trim().is_empty() {
                return Err(Error::InvalidConfig(
                    "http_api_token must not be empty".to_string(),
                ));
            }
        }

        if self.health_check && self.run_once {
            return Err(Error::InvalidConfig(
                "health_check cannot be combined with run_once".to_string(),
            ));
        }

        Ok(())
    }
}

/// In-memory application handle.
///
/// The binary can construct this from parsed CLI/config data and then call the
/// convenience [`run`] function below.
#[derive(Debug, Clone)]
pub struct WatchtowerApp {
    config: AppConfig,
}

impl WatchtowerApp {
    /// Create a new application instance from configuration.
    pub fn new(config: impl Into<AppConfig>) -> Self {
        Self {
            config: config.into(),
        }
    }

    /// Execute the application.
    ///
    /// The real container and scheduling work will land here later; for now it
    /// validates the configuration and exits cleanly.
    pub fn run(&self) -> Result<()> {
        self.config.validate()?;
        Ok(())
    }
}

/// Convenience entrypoint for the binary crate.
pub fn run(config: impl Into<AppConfig>) -> Result<()> {
    WatchtowerApp::new(config).run()
}
