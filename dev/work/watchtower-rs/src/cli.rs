#![forbid(unsafe_code)]

//! Watchtower CLI surface.
//!
//! This module keeps the initial parser explicit so later slices can wire it
//! into config loading and runtime behavior without having to rework the flag
//! model.

use std::fmt;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use thiserror::Error;

/// Default polling interval used by the legacy program model.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Default container stop timeout used by the legacy program model.
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Parsed command-line input.
///
/// This mirrors the legacy startup surface in small groups instead of trying to
/// normalize everything into a single flattened config too early.
#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "watchtower",
    version,
    about = "Watchtower container update daemon",
    disable_help_subcommand = true
)]
pub struct WatchtowerCli {
    /// Scheduling and polling controls.
    #[command(flatten)]
    pub scheduling: SchedulingArgs,

    /// Update and restart behavior.
    #[command(flatten)]
    pub update: UpdateArgs,

    /// Container targeting and scope filters.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// HTTP API options.
    #[command(flatten)]
    pub http_api: HttpApiArgs,

    /// Logging-related switches.
    #[command(flatten)]
    pub logging: LoggingArgs,

    /// Positional container names.
    ///
    /// When omitted, Watchtower monitors all eligible containers.
    #[arg(value_name = "CONTAINER")]
    pub containers: Vec<String>,
}

impl WatchtowerCli {
    /// Parse the process arguments and resolve environment-backed defaults.
    pub fn parse_resolved() -> Result<WatchtowerConfig, CliError> {
        Self::parse().try_into()
    }
}

/// Scheduling and polling controls.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct SchedulingArgs {
    /// Poll interval in seconds.
    ///
    /// This is mutually exclusive with `--schedule`.
    #[arg(
        short = 'i',
        long,
        env = "WATCHTOWER_POLL_INTERVAL",
        value_name = "SECONDS"
    )]
    pub interval_seconds: Option<u64>,

    /// Cron expression that defines the next update checks.
    ///
    /// This is mutually exclusive with `--interval`.
    #[arg(
        short = 's',
        long,
        env = "WATCHTOWER_SCHEDULE",
        value_name = "CRON"
    )]
    pub schedule: Option<String>,

    /// Timeout before a container is forcefully stopped.
    #[arg(
        short = 't',
        long = "stop-timeout",
        env = "WATCHTOWER_TIMEOUT",
        value_name = "DURATION",
        value_parser = parse_duration
    )]
    pub stop_timeout: Option<Duration>,
}

/// Update and restart behavior.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct UpdateArgs {
    /// Do not pull any new images.
    #[arg(long, env = "WATCHTOWER_NO_PULL")]
    pub no_pull: bool,

    /// Do not restart any containers after a successful update.
    #[arg(long, env = "WATCHTOWER_NO_RESTART")]
    pub no_restart: bool,

    /// Remove previously used images after updating.
    #[arg(short = 'c', long, env = "WATCHTOWER_CLEANUP")]
    pub cleanup: bool,

    /// Remove attached anonymous volumes before updating.
    #[arg(long, env = "WATCHTOWER_REMOVE_VOLUMES")]
    pub remove_volumes: bool,

    /// Restart containers one at a time.
    #[arg(long, env = "WATCHTOWER_ROLLING_RESTART")]
    pub rolling_restart: bool,

    /// Will also include restarting containers.
    #[arg(long, env = "WATCHTOWER_INCLUDE_RESTARTING")]
    pub include_restarting: bool,

    /// Will also include created and exited containers.
    #[arg(short = 'S', long, env = "WATCHTOWER_INCLUDE_STOPPED")]
    pub include_stopped: bool,

    /// Also start stopped containers that were updated.
    #[arg(long, env = "WATCHTOWER_REVIVE_STOPPED")]
    pub revive_stopped: bool,

    /// Only monitor for new images, do not update containers.
    #[arg(short = 'm', long, env = "WATCHTOWER_MONITOR_ONLY")]
    pub monitor_only: bool,

    /// Run once now and exit.
    #[arg(short = 'R', long, env = "WATCHTOWER_RUN_ONCE")]
    pub run_once: bool,

    /// Allow labels to take precedence over global arguments.
    #[arg(long, env = "WATCHTOWER_LABEL_TAKE_PRECEDENCE")]
    pub label_take_precedence: bool,
}

/// Container targeting and scope filters.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct SelectionArgs {
    /// Only watch containers with the enable label set to true.
    #[arg(short = 'e', long, env = "WATCHTOWER_LABEL_ENABLE")]
    pub label_enable: bool,

    /// Exclude containers by name.
    ///
    /// The legacy program accepted comma or whitespace separated values in the
    /// environment variable, so this keeps the parser permissive and normalizes
    /// the result after parsing.
    #[arg(
        short = 'x',
        long = "disable-containers",
        env = "WATCHTOWER_DISABLE_CONTAINERS",
        value_delimiter = ',',
        num_args = 0..,
        value_name = "CONTAINER"
    )]
    pub disable_containers: Vec<String>,

    /// Restrict the watchtower instance to a named scope.
    #[arg(long, env = "WATCHTOWER_SCOPE", value_name = "SCOPE")]
    pub scope: Option<String>,
}

/// HTTP API mode options.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct HttpApiArgs {
    /// Enable HTTP API update mode.
    #[arg(long, env = "WATCHTOWER_HTTP_API_UPDATE")]
    pub update: bool,

    /// Enable the Prometheus metrics HTTP API.
    #[arg(long, env = "WATCHTOWER_HTTP_API_METRICS")]
    pub metrics: bool,

    /// Authentication token for HTTP API requests.
    ///
    /// This is intentionally kept as plain text at the CLI layer; future slices
    /// can add secret-file expansion at the config boundary if needed.
    #[arg(long, env = "WATCHTOWER_HTTP_API_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// Keep periodic polls active even when HTTP API mode is enabled.
    #[arg(long, env = "WATCHTOWER_HTTP_API_PERIODIC_POLLS")]
    pub periodic_polls: bool,
}

/// Logging-related switches.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct LoggingArgs {
    /// Maximum log level written to stderr.
    #[arg(long, env = "WATCHTOWER_LOG_LEVEL", value_enum, default_value_t = LogLevel::Info)]
    pub log_level: LogLevel,

    /// Log formatting mode.
    #[arg(short = 'l', long, env = "WATCHTOWER_LOG_FORMAT", value_enum, default_value_t = LogFormat::Auto)]
    pub log_format: LogFormat,

    /// Enable debug mode.
    #[arg(short = 'd', long, env = "WATCHTOWER_DEBUG")]
    pub debug: bool,

    /// Enable trace mode.
    #[arg(long, env = "WATCHTOWER_TRACE")]
    pub trace: bool,

    /// Disable ANSI color escape codes in log output.
    #[arg(long, env = "NO_COLOR")]
    pub no_color: bool,

    /// Prevent the startup message from being emitted.
    #[arg(long, env = "WATCHTOWER_NO_STARTUP_MESSAGE")]
    pub no_startup_message: bool,
}

/// Maximum log level values supported by the legacy surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[value(rename_all = "lower")]
pub enum LogLevel {
    Panic,
    Fatal,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

/// Log formatting values supported by the legacy surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[value(rename_all = "lower")]
pub enum LogFormat {
    #[default]
    Auto,
    Logfmt,
    Pretty,
    Json,
}

/// Resolved application configuration derived from CLI and environment data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchtowerConfig {
    /// Positional containers to include.
    pub containers: Vec<String>,

    /// Resolved scheduling mode.
    pub scheduling: SchedulingConfig,

    /// Resolved update behavior.
    pub update: UpdateConfig,

    /// Resolved selection filters.
    pub selection: SelectionConfig,

    /// Resolved HTTP API mode.
    pub http_api: HttpApiConfig,

    /// Resolved logging behavior.
    pub logging: LoggingConfig,
}

/// Resolved scheduling configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingConfig {
    /// Either a fixed interval or a cron expression.
    pub mode: PollingMode,

    /// Timeout before a container is forcefully stopped.
    pub stop_timeout: Duration,

    /// Keep periodic polls active when HTTP API mode is enabled.
    pub periodic_polls: bool,
}

/// The active polling mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollingMode {
    /// Poll every fixed interval.
    Interval(Duration),

    /// Poll according to a cron expression.
    Schedule(String),
}

/// Resolved update behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateConfig {
    pub no_pull: bool,
    pub no_restart: bool,
    pub cleanup: bool,
    pub remove_volumes: bool,
    pub rolling_restart: bool,
    pub include_restarting: bool,
    pub include_stopped: bool,
    pub revive_stopped: bool,
    pub monitor_only: bool,
    pub run_once: bool,
    pub label_take_precedence: bool,
}

/// Resolved selection filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionConfig {
    pub label_enable: bool,
    pub disable_containers: Vec<String>,
    pub scope: Option<String>,
}

/// Resolved HTTP API configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpApiConfig {
    pub update: bool,
    pub metrics: bool,
    pub token: Option<String>,
}

/// Resolved logging configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    pub log_level: LogLevel,
    pub log_format: LogFormat,
    pub debug: bool,
    pub trace: bool,
    pub no_color: bool,
    pub no_startup_message: bool,
}

/// Errors produced while resolving the parsed CLI surface into runtime config.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliError {
    /// Both supported polling styles were configured at the same time.
    #[error("`--interval` and `--schedule` are mutually exclusive")]
    PollingConflict,

    /// The poll interval was explicitly set to zero.
    #[error("`--interval` must be greater than zero")]
    InvalidInterval,

    /// A duration value could not be parsed.
    #[error("invalid duration {value:?}: {message}")]
    InvalidDuration {
        value: String,
        message: String,
    },
}

impl TryFrom<WatchtowerCli> for WatchtowerConfig {
    type Error = CliError;

    fn try_from(cli: WatchtowerCli) -> Result<Self, Self::Error> {
        let WatchtowerCli {
            scheduling,
            update,
            selection,
            http_api,
            logging,
            containers,
        } = cli;

        let scheduling = resolve_scheduling(scheduling, http_api.periodic_polls)?;
        let logging = resolve_logging(logging);

        Ok(Self {
            containers,
            scheduling,
            update: UpdateConfig {
                no_pull: update.no_pull,
                no_restart: update.no_restart,
                cleanup: update.cleanup,
                remove_volumes: update.remove_volumes,
                rolling_restart: update.rolling_restart,
                include_restarting: update.include_restarting,
                include_stopped: update.include_stopped,
                revive_stopped: update.revive_stopped,
                monitor_only: update.monitor_only,
                run_once: update.run_once,
                label_take_precedence: update.label_take_precedence,
            },
            selection: SelectionConfig {
                label_enable: selection.label_enable,
                disable_containers: normalize_list(selection.disable_containers),
                scope: selection.scope,
            },
            http_api: HttpApiConfig {
                update: http_api.update,
                metrics: http_api.metrics,
                token: http_api.token,
            },
            logging,
        })
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Panic => "panic",
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        };
        f.write_str(value)
    }
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Logfmt => "logfmt",
            Self::Pretty => "pretty",
            Self::Json => "json",
        };
        f.write_str(value)
    }
}

fn resolve_scheduling(
    args: SchedulingArgs,
    periodic_polls: bool,
) -> Result<SchedulingConfig, CliError> {
    let SchedulingArgs {
        interval_seconds,
        schedule,
        stop_timeout,
    } = args;

    if interval_seconds.is_some() && schedule.is_some() {
        return Err(CliError::PollingConflict);
    }

    let mode = match (interval_seconds, schedule) {
        (Some(interval), None) => {
            if interval == 0 {
                return Err(CliError::InvalidInterval);
            }
            PollingMode::Interval(Duration::from_secs(interval))
        }
        (None, Some(schedule)) => {
            if schedule.trim().is_empty() {
                return Err(CliError::InvalidDuration {
                    value: schedule,
                    message: "schedule cannot be empty".to_owned(),
                });
            }
            PollingMode::Schedule(schedule)
        }
        (Some(interval), Some(_)) => {
            debug_assert!(interval > 0);
            return Err(CliError::PollingConflict);
        }
        (None, None) => PollingMode::Interval(DEFAULT_POLL_INTERVAL),
    };

    Ok(SchedulingConfig {
        mode,
        stop_timeout: stop_timeout.unwrap_or(DEFAULT_STOP_TIMEOUT),
        periodic_polls,
    })
}

fn resolve_logging(args: LoggingArgs) -> LoggingConfig {
    let log_level = if args.trace {
        LogLevel::Trace
    } else if args.debug {
        LogLevel::Debug
    } else {
        args.log_level
    };

    LoggingConfig {
        log_level,
        log_format: args.log_format,
        debug: args.debug,
        trace: args.trace,
        no_color: args.no_color,
        no_startup_message: args.no_startup_message,
    }
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();

    for raw in values {
        for item in raw.split(|c: char| c == ',' || c.is_whitespace()) {
            let item = item.trim();
            if !item.is_empty() {
                normalized.push(item.to_owned());
            }
        }
    }

    normalized
}

fn parse_duration(input: &str) -> Result<Duration, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".to_owned());
    }

    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Ok(Duration::from_secs(seconds));
    }

    let mut total = 0u64;
    let mut current = 0u64;
    let mut saw_unit = false;
    let mut saw_digit = false;

    for ch in trimmed.chars() {
        if let Some(digit) = ch.to_digit(10) {
            saw_digit = true;
            current = current
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit as u64))
                .ok_or_else(|| format!("duration value {trimmed:?} overflows u64"))?;
            continue;
        }

        if !saw_digit {
            return Err(format!("invalid duration {trimmed:?}"));
        }

        let multiplier = match ch {
            's' => 1,
            'm' => 60,
            'h' => 60 * 60,
            'd' => 60 * 60 * 24,
            _ => return Err(format!("invalid duration unit {ch:?} in {trimmed:?}")),
        };

        total = total
            .checked_add(current.saturating_mul(multiplier))
            .ok_or_else(|| format!("duration value {trimmed:?} overflows u64"))?;
        current = 0;
        saw_unit = true;
    }

    if !saw_unit || current != 0 {
        return Err(format!("invalid duration {trimmed:?}"));
    }

    Ok(Duration::from_secs(total))
}
