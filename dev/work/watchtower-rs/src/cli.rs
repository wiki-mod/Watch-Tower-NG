#![forbid(unsafe_code)]

//! Watchtower CLI surface.
//!
//! This module keeps the initial parser explicit so later slices can wire it
//! into config loading and runtime behavior without having to rework the flag
//! model.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
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
    /// Docker connection settings.
    #[command(flatten)]
    pub docker: DockerArgs,

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

    /// Notification and output options.
    #[command(flatten)]
    pub notifications: NotificationArgs,

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
    pub fn try_parse_resolved() -> Result<WatchtowerConfig, WatchtowerCliError> {
        let cli = Self::try_parse()?;
        let mut config: WatchtowerConfig = cli.try_into()?;
        config.resolve_secret_references()?;
        Ok(config)
    }
}

/// Docker connection settings.
#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct DockerArgs {
    /// Docker daemon socket to connect to.
    #[arg(
        short = 'H',
        long,
        env = "DOCKER_HOST",
        default_value = "unix:///var/run/docker.sock",
        value_name = "HOST"
    )]
    pub host: String,

    /// Use TLS and verify the remote Docker daemon.
    #[arg(short = 'v', long, env = "DOCKER_TLS_VERIFY")]
    pub tlsverify: bool,

    /// API version to use by the Docker client.
    #[arg(
        short = 'a',
        long = "api-version",
        env = "DOCKER_API_VERSION",
        default_value = "1.52",
        value_name = "VERSION"
    )]
    pub api_version: String,
}

impl Default for DockerArgs {
    fn default() -> Self {
        Self {
            host: "unix:///var/run/docker.sock".to_string(),
            tlsverify: false,
            api_version: "1.52".to_string(),
        }
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
        value_name = "CRON",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
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

    /// Skip the standard health check path and exit immediately.
    #[arg(long = "health-check")]
    pub health_check: bool,

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
    /// The legacy program accepted comma- or whitespace-separated values, so
    /// each parsed chunk is normalized immediately and later flattened.
    #[arg(
        short = 'x',
        long = "disable-containers",
        env = "WATCHTOWER_DISABLE_CONTAINERS",
        num_args = 0..,
        value_parser = parse_disable_container_values,
        value_name = "CONTAINER"
    )]
    pub disable_containers: Vec<DisableContainerValues>,

    /// Restrict the watchtower instance to a named scope.
    #[arg(
        long,
        env = "WATCHTOWER_SCOPE",
        value_name = "SCOPE",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
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
    #[arg(
        long,
        env = "WATCHTOWER_HTTP_API_TOKEN",
        value_name = "TOKEN",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub token: Option<String>,

    /// Keep periodic polls active even when HTTP API mode is enabled.
    #[arg(long, env = "WATCHTOWER_HTTP_API_PERIODIC_POLLS")]
    pub periodic_polls: bool,
}

/// Notification and porcelain output options.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct NotificationArgs {
    /// Notification transports to activate.
    #[arg(
        short = 'n',
        long = "notifications",
        env = "WATCHTOWER_NOTIFICATIONS",
        num_args = 0..,
        value_parser = parse_notification_types,
        value_name = "TYPE"
    )]
    pub types: Vec<NotificationTypeValues>,

    /// Log level used by the notification subsystem.
    #[arg(
        long = "notifications-level",
        env = "WATCHTOWER_NOTIFICATIONS_LEVEL",
        default_value_t = NotificationLogLevel::Info,
        value_enum
    )]
    pub level: NotificationLogLevel,

    /// Delay before sending notifications.
    #[arg(
        long = "notifications-delay",
        env = "WATCHTOWER_NOTIFICATIONS_DELAY",
        value_name = "SECONDS",
        value_parser = parse_duration
    )]
    pub delay: Option<Duration>,

    /// Hostname used in notification titles.
    #[arg(
        long = "notifications-hostname",
        env = "WATCHTOWER_NOTIFICATIONS_HOSTNAME",
        value_name = "HOST"
    )]
    pub hostname: Option<String>,

    /// Additional notification URLs.
    #[arg(
        long = "notification-url",
        env = "WATCHTOWER_NOTIFICATION_URL",
        num_args = 0..,
        value_parser = parse_notification_urls,
        value_name = "URL"
    )]
    pub urls: Vec<NotificationUrlValues>,

    /// Use the session report as notification template data.
    #[arg(long = "notification-report", env = "WATCHTOWER_NOTIFICATION_REPORT")]
    pub report: bool,

    /// Notification message template.
    #[arg(
        long = "notification-template",
        env = "WATCHTOWER_NOTIFICATION_TEMPLATE",
        value_name = "TPL"
    )]
    pub template: Option<String>,

    /// Prefix tag for notification titles.
    #[arg(
        long = "notification-title-tag",
        env = "WATCHTOWER_NOTIFICATION_TITLE_TAG",
        value_name = "TAG"
    )]
    pub title_tag: Option<String>,

    /// Do not pass a title to notification transports.
    #[arg(long = "notification-skip-title", env = "WATCHTOWER_NOTIFICATION_SKIP_TITLE")]
    pub skip_title: bool,

    /// Write notification logs to stdout instead of stderr.
    #[arg(long = "notification-log-stdout", env = "WATCHTOWER_NOTIFICATION_LOG_STDOUT")]
    pub log_stdout: bool,

    /// Enable porcelain output compatibility.
    #[arg(long = "porcelain", env = "WATCHTOWER_PORCELAIN", value_enum)]
    pub porcelain: Option<PorcelainVersion>,

    /// When to warn about HEAD pull failures.
    #[arg(
        long = "warn-on-head-failure",
        env = "WATCHTOWER_WARN_ON_HEAD_FAILURE",
        default_value_t = WarnOnHeadFailure::Auto,
        value_enum
    )]
    pub warn_on_head_failure: WarnOnHeadFailure,

    /// Email transport settings.
    #[command(flatten)]
    pub email: EmailNotificationArgs,

    /// Slack transport settings.
    #[command(flatten)]
    pub slack: SlackNotificationArgs,

    /// Microsoft Teams transport settings.
    #[command(flatten)]
    pub msteams: TeamsNotificationArgs,

    /// Gotify transport settings.
    #[command(flatten)]
    pub gotify: GotifyNotificationArgs,
}

/// A normalized chunk of notification transport types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationTypeValues {
    values: Vec<String>,
}

impl NotificationTypeValues {
    /// Consume the parsed chunk and return the normalized values.
    pub fn into_inner(self) -> Vec<String> {
        self.values
    }
}

/// Notification subsystem log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[value(rename_all = "lower")]
pub enum NotificationLogLevel {
    Panic,
    Fatal,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

/// Supported porcelain output format versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum PorcelainVersion {
    V1,
}

impl PorcelainVersion {
    /// Return the legacy template version string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
        }
    }
}

/// When to warn on failed registry HEAD requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
#[value(rename_all = "lower")]
pub enum WarnOnHeadFailure {
    Always,
    #[default]
    Auto,
    Never,
}

/// Email transport settings.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct EmailNotificationArgs {
    /// Address to send notification emails from.
    #[arg(
        long = "notification-email-from",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_FROM",
        value_name = "ADDRESS"
    )]
    pub from: Option<String>,

    /// Address to send notification emails to.
    #[arg(
        long = "notification-email-to",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_TO",
        value_name = "ADDRESS"
    )]
    pub to: Option<String>,

    /// SMTP server to send notification emails through.
    #[arg(
        long = "notification-email-server",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_SERVER",
        value_name = "HOST"
    )]
    pub server: Option<String>,

    /// SMTP server user for sending notifications.
    #[arg(
        long = "notification-email-server-user",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_SERVER_USER",
        value_name = "USER"
    )]
    pub user: Option<String>,

    /// SMTP server password for sending notifications.
    #[arg(
        long = "notification-email-server-password",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_SERVER_PASSWORD",
        value_name = "PASSWORD"
    )]
    pub password: Option<String>,

    /// SMTP server port to send notification emails through.
    #[arg(
        long = "notification-email-server-port",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_SERVER_PORT",
        default_value_t = 25,
        value_name = "PORT"
    )]
    pub port: u16,

    /// Controls whether watchtower verifies the SMTP server certificate.
    #[arg(
        long = "notification-email-server-tls-skip-verify",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_SERVER_TLS_SKIP_VERIFY"
    )]
    pub tls_skip_verify: bool,

    /// Delay before sending email notifications.
    #[arg(
        long = "notification-email-delay",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_DELAY",
        value_name = "SECONDS",
        value_parser = parse_duration
    )]
    pub delay: Option<Duration>,

    /// Subject prefix tag for notifications via mail.
    #[arg(
        long = "notification-email-subjecttag",
        env = "WATCHTOWER_NOTIFICATION_EMAIL_SUBJECTTAG",
        value_name = "TAG"
    )]
    pub subject_tag: Option<String>,
}

/// Slack transport settings.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct SlackNotificationArgs {
    /// The Slack Hook URL to send notifications to.
    #[arg(
        long = "notification-slack-hook-url",
        env = "WATCHTOWER_NOTIFICATION_SLACK_HOOK_URL",
        value_name = "URL"
    )]
    pub hook_url: Option<String>,

    /// Identifier used to identify this watchtower instance.
    #[arg(
        long = "notification-slack-identifier",
        env = "WATCHTOWER_NOTIFICATION_SLACK_IDENTIFIER",
        default_value = "watchtower",
        value_name = "NAME"
    )]
    pub identifier: String,

    /// Override the webhook's default channel.
    #[arg(
        long = "notification-slack-channel",
        env = "WATCHTOWER_NOTIFICATION_SLACK_CHANNEL",
        value_name = "CHANNEL"
    )]
    pub channel: Option<String>,

    /// Emoji to use instead of the default icon.
    #[arg(
        long = "notification-slack-icon-emoji",
        env = "WATCHTOWER_NOTIFICATION_SLACK_ICON_EMOJI",
        value_name = "EMOJI"
    )]
    pub icon_emoji: Option<String>,

    /// Icon image URL to use instead of the default icon.
    #[arg(
        long = "notification-slack-icon-url",
        env = "WATCHTOWER_NOTIFICATION_SLACK_ICON_URL",
        value_name = "URL"
    )]
    pub icon_url: Option<String>,
}

/// Microsoft Teams transport settings.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct TeamsNotificationArgs {
    /// The Teams webhook URL to send notifications to.
    #[arg(
        long = "notification-msteams-hook",
        env = "WATCHTOWER_NOTIFICATION_MSTEAMS_HOOK_URL",
        value_name = "URL"
    )]
    pub hook: Option<String>,

    /// Try to include log entry data as Teams message facts.
    #[arg(
        long = "notification-msteams-data",
        env = "WATCHTOWER_NOTIFICATION_MSTEAMS_USE_LOG_DATA"
    )]
    pub data: bool,
}

/// Gotify transport settings.
#[derive(Debug, Clone, Parser, PartialEq, Eq, Default)]
pub struct GotifyNotificationArgs {
    /// The Gotify URL to send notifications to.
    #[arg(
        long = "notification-gotify-url",
        env = "WATCHTOWER_NOTIFICATION_GOTIFY_URL",
        value_name = "URL"
    )]
    pub url: Option<String>,

    /// The Gotify application token.
    #[arg(
        long = "notification-gotify-token",
        env = "WATCHTOWER_NOTIFICATION_GOTIFY_TOKEN",
        value_name = "TOKEN"
    )]
    pub token: Option<String>,

    /// Controls whether watchtower verifies the Gotify server certificate.
    #[arg(
        long = "notification-gotify-tls-skip-verify",
        env = "WATCHTOWER_NOTIFICATION_GOTIFY_TLS_SKIP_VERIFY"
    )]
    pub tls_skip_verify: bool,
}

/// A normalized chunk of notification URL values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationUrlValues {
    values: Vec<String>,
}

impl NotificationUrlValues {
    /// Consume the parsed chunk and return the normalized values.
    pub fn into_inner(self) -> Vec<String> {
        self.values
    }
}

/// Logging-related switches.
#[derive(Debug, Clone, Copy, Parser, PartialEq, Eq, Default)]
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

/// A normalized chunk of `disable-containers` values.
///
/// Clap parses each raw CLI occurrence or environment chunk independently, so
/// this keeps the parser explicit while still preserving the legacy support for
/// commas and whitespace as separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisableContainerValues {
    values: Vec<String>,
}

impl DisableContainerValues {
    /// Consume the parsed chunk and return the normalized container names.
    pub fn into_inner(self) -> Vec<String> {
        self.values
    }
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
    /// Resolved Docker connection settings.
    pub docker: DockerConfig,

    /// Positional containers to include.
    pub containers: Vec<String>,

    /// Resolved scheduling mode.
    pub scheduling: SchedulingConfig,

    /// Resolved update behavior.
    pub update: UpdateConfig,

    /// Legacy health-check startup mode.
    pub health_check: bool,

    /// Resolved selection filters.
    pub selection: SelectionConfig,

    /// Resolved HTTP API mode.
    pub http_api: HttpApiConfig,

    /// Resolved notification settings.
    pub notifications: NotificationConfig,

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

/// Resolved Docker connection settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerConfig {
    pub host: String,
    pub tlsverify: bool,
    pub api_version: String,
}

/// Resolved notification settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationConfig {
    pub types: Vec<String>,
    pub level: NotificationLogLevel,
    pub delay: Option<Duration>,
    pub hostname: Option<String>,
    pub urls: Vec<String>,
    pub report: bool,
    pub template: Option<String>,
    pub title_tag: Option<String>,
    pub skip_title: bool,
    pub log_stdout: bool,
    pub porcelain: Option<PorcelainVersion>,
    pub warn_on_head_failure: WarnOnHeadFailure,
    pub email: EmailNotificationConfig,
    pub slack: SlackNotificationConfig,
    pub msteams: TeamsNotificationConfig,
    pub gotify: GotifyNotificationConfig,
}

/// Resolved email notification settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailNotificationConfig {
    pub from: Option<String>,
    pub to: Option<String>,
    pub server: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub port: u16,
    pub tls_skip_verify: bool,
    pub delay: Option<Duration>,
    pub subject_tag: Option<String>,
}

/// Resolved Slack notification settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackNotificationConfig {
    pub hook_url: Option<String>,
    pub identifier: String,
    pub channel: Option<String>,
    pub icon_emoji: Option<String>,
    pub icon_url: Option<String>,
}

/// Resolved Microsoft Teams notification settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamsNotificationConfig {
    pub hook: Option<String>,
    pub data: bool,
}

/// Resolved Gotify notification settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotifyNotificationConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub tls_skip_verify: bool,
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
}

/// Errors that can occur while parsing or resolving the CLI surface.
#[derive(Debug, Error)]
pub enum WatchtowerCliError {
    /// clap rejected the raw CLI arguments.
    #[error(transparent)]
    Parse(#[from] clap::Error),

    /// The parsed values were not a valid runtime configuration.
    #[error(transparent)]
    Resolve(#[from] CliError),

    /// A configured secret file could not be read.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl TryFrom<WatchtowerCli> for WatchtowerConfig {
    type Error = CliError;

    fn try_from(cli: WatchtowerCli) -> Result<Self, Self::Error> {
        let WatchtowerCli {
            docker,
            scheduling,
            update,
            selection,
            http_api,
            notifications,
            logging,
            containers,
        } = cli;

        let scheduling = resolve_scheduling(scheduling, http_api.periodic_polls)?;
        let logging = logging.into();

        Ok(Self {
            docker: docker.into(),
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
            health_check: update.health_check,
            selection: SelectionConfig {
                label_enable: selection.label_enable,
                disable_containers: flatten_disable_container_values(selection.disable_containers),
                scope: selection.scope,
            },
            http_api: HttpApiConfig {
                update: http_api.update,
                metrics: http_api.metrics,
                token: http_api.token,
            },
            notifications: notifications.into(),
            logging,
        })
    }
}

impl LoggingArgs {
    /// Resolve the effective log level after applying the legacy debug/trace
    /// overrides.
    pub fn effective_level(&self) -> LogLevel {
        if self.trace {
            LogLevel::Trace
        } else if self.debug {
            LogLevel::Debug
        } else {
            self.log_level
        }
    }
}

impl LoggingConfig {
    /// Return the effective log level after legacy overrides.
    pub fn effective_level(&self) -> LogLevel {
        self.log_level
    }

    /// Whether ANSI color codes should be emitted.
    pub fn ansi_enabled(&self) -> bool {
        !self.no_color
    }
}

impl From<LoggingArgs> for LoggingConfig {
    fn from(args: LoggingArgs) -> Self {
        let log_level = args.effective_level();

        Self {
            log_level,
            log_format: args.log_format,
            debug: args.debug,
            trace: args.trace,
            no_color: args.no_color,
            no_startup_message: args.no_startup_message,
        }
    }
}

impl WatchtowerConfig {
    /// Resolve file-backed secrets in the finalized configuration.
    pub fn resolve_secret_references(&mut self) -> io::Result<()> {
        self.http_api.token = expand_optional_secret(self.http_api.token.take())?;
        self.notifications.urls = expand_secret_list(self.notifications.urls.clone())?;
        self.notifications.email.password =
            expand_optional_secret(self.notifications.email.password.take())?;
        self.notifications.slack.hook_url =
            expand_optional_secret(self.notifications.slack.hook_url.take())?;
        self.notifications.msteams.hook = expand_optional_secret(self.notifications.msteams.hook.take())?;
        self.notifications.gotify.url = expand_optional_secret(self.notifications.gotify.url.take())?;
        self.notifications.gotify.token =
            expand_optional_secret(self.notifications.gotify.token.take())?;
        Ok(())
    }
}

impl From<DockerArgs> for DockerConfig {
    fn from(args: DockerArgs) -> Self {
        Self {
            host: args.host,
            tlsverify: args.tlsverify,
            api_version: args.api_version,
        }
    }
}

impl From<NotificationArgs> for NotificationConfig {
    fn from(args: NotificationArgs) -> Self {
        let mut urls = flatten_notification_urls(args.urls);
        let mut report = args.report;
        let mut template = args.template;
        let mut log_stdout = args.log_stdout;

        if let Some(version) = args.porcelain {
            urls.push("logger://".to_string());
            report = true;
            log_stdout = true;
            template = Some(format!("porcelain.{}.summary-no-log", version.as_str()));
        }

        Self {
            types: flatten_notification_types(args.types),
            level: args.level,
            delay: args.delay,
            hostname: args.hostname,
            urls,
            report,
            template,
            title_tag: args.title_tag,
            skip_title: args.skip_title,
            log_stdout,
            porcelain: args.porcelain,
            warn_on_head_failure: args.warn_on_head_failure,
            email: EmailNotificationConfig {
                from: args.email.from,
                to: args.email.to,
                server: args.email.server,
                user: args.email.user,
                password: args.email.password,
                port: args.email.port,
                tls_skip_verify: args.email.tls_skip_verify,
                delay: args.email.delay,
                subject_tag: args.email.subject_tag,
            },
            slack: SlackNotificationConfig {
                hook_url: args.slack.hook_url,
                identifier: args.slack.identifier,
                channel: args.slack.channel,
                icon_emoji: args.slack.icon_emoji,
                icon_url: args.slack.icon_url,
            },
            msteams: TeamsNotificationConfig {
                hook: args.msteams.hook,
                data: args.msteams.data,
            },
            gotify: GotifyNotificationConfig {
                url: args.gotify.url,
                token: args.gotify.token,
                tls_skip_verify: args.gotify.tls_skip_verify,
            },
        }
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
        (None, Some(schedule)) => PollingMode::Schedule(schedule),
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

fn flatten_disable_container_values(values: Vec<DisableContainerValues>) -> Vec<String> {
    let mut normalized = Vec::new();

    for value in values {
        normalized.extend(value.into_inner());
    }

    normalized
}

fn flatten_notification_types(values: Vec<NotificationTypeValues>) -> Vec<String> {
    let mut normalized = Vec::new();

    for value in values {
        normalized.extend(value.into_inner());
    }

    normalized
}

fn flatten_notification_urls(values: Vec<NotificationUrlValues>) -> Vec<String> {
    let mut normalized = Vec::new();

    for value in values {
        normalized.extend(value.into_inner());
    }

    normalized
}

fn parse_notification_types(input: &str) -> Result<NotificationTypeValues, String> {
    let values = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();

    Ok(NotificationTypeValues { values })
}

fn parse_notification_urls(input: &str) -> Result<NotificationUrlValues, String> {
    let values = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();

    Ok(NotificationUrlValues { values })
}

fn parse_disable_container_values(input: &str) -> Result<DisableContainerValues, String> {
    let values = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();

    Ok(DisableContainerValues { values })
}

fn expand_optional_secret(value: Option<String>) -> io::Result<Option<String>> {
    match value {
        Some(value) => Ok(Some(expand_secret_value(&value)?)),
        None => Ok(None),
    }
}

fn expand_secret_list(values: Vec<String>) -> io::Result<Vec<String>> {
    let mut expanded = Vec::new();

    for value in values {
        if is_file_reference(&value) {
            let content = fs::read_to_string(&value)?;
            expanded.extend(
                content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned),
            );
        } else {
            expanded.push(value);
        }
    }

    Ok(expanded)
}

fn expand_secret_value(value: &str) -> io::Result<String> {
    if is_file_reference(value) {
        let content = fs::read_to_string(value)?;
        Ok(content.trim().to_string())
    } else {
        Ok(value.to_string())
    }
}

fn is_file_reference(value: &str) -> bool {
    let colon = value.find(':');
    if matches!(colon, Some(index) if index != 1) {
        return false;
    }

    Path::new(value).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_disable_containers_from_mixed_separators() {
        let disable_containers = flatten_disable_container_values(vec![
            parse_disable_container_values("alpha,beta gamma").expect("chunk parses"),
            parse_disable_container_values("delta").expect("chunk parses"),
            parse_disable_container_values("epsilon,zeta").expect("chunk parses"),
        ]);

        assert_eq!(
            disable_containers,
            vec![
                "alpha".to_owned(),
                "beta".to_owned(),
                "gamma".to_owned(),
                "delta".to_owned(),
                "epsilon".to_owned(),
                "zeta".to_owned(),
            ]
        );
    }

    #[test]
    fn logging_args_apply_debug_and_trace_overrides() {
        let base = LoggingArgs {
            log_level: LogLevel::Warn,
            log_format: LogFormat::Auto,
            debug: false,
            trace: false,
            no_color: false,
            no_startup_message: false,
        };

        assert_eq!(base.effective_level(), LogLevel::Warn);
        assert_eq!((LoggingArgs { debug: true, ..base }).effective_level(), LogLevel::Debug);
        assert_eq!((LoggingArgs { trace: true, ..base }).effective_level(), LogLevel::Trace);
    }

    #[test]
    fn carries_health_check_into_resolved_config() {
        let cli = WatchtowerCli {
            docker: DockerArgs::default(),
            scheduling: SchedulingArgs::default(),
            update: UpdateArgs {
                health_check: true,
                ..UpdateArgs::default()
            },
            selection: SelectionArgs::default(),
            http_api: HttpApiArgs::default(),
            notifications: NotificationArgs::default(),
            logging: LoggingArgs::default(),
            containers: Vec::new(),
        };

        let config: WatchtowerConfig = cli
            .try_into()
            .expect("config resolves");

        assert!(config.health_check);
    }

    #[test]
    fn resolves_secret_file_references_for_http_and_notifications() {
        let http_token = write_temp_file("http-token", "secret-token\n");
        let slack_hook = write_temp_file("slack-hook", "https://hooks.slack.com/services/AAA/BBB/CCC\n");
        let msteams_hook = write_temp_file("msteams-hook", "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc\n");
        let gotify_url = write_temp_file("gotify-url", "https://gotify.local/\n");
        let gotify_token = write_temp_file("gotify-token", "gotify-secret\n");
        let notification_urls = write_temp_file("notification-urls", "https://example.test/first\n\nhttps://example.test/second\n");
        let email_password = write_temp_file("email-password", "mail-secret\n");

        let cli = WatchtowerCli {
            docker: DockerArgs::default(),
            scheduling: SchedulingArgs::default(),
            update: UpdateArgs::default(),
            selection: SelectionArgs::default(),
            http_api: HttpApiArgs {
                token: Some(http_token.to_string_lossy().into_owned()),
                ..HttpApiArgs::default()
            },
            notifications: NotificationArgs {
                urls: vec![NotificationUrlValues {
                    values: vec![notification_urls.to_string_lossy().into_owned()],
                }],
                email: EmailNotificationArgs {
                    password: Some(email_password.to_string_lossy().into_owned()),
                    ..EmailNotificationArgs::default()
                },
                slack: SlackNotificationArgs {
                    hook_url: Some(slack_hook.to_string_lossy().into_owned()),
                    ..SlackNotificationArgs::default()
                },
                msteams: TeamsNotificationArgs {
                    hook: Some(msteams_hook.to_string_lossy().into_owned()),
                    ..TeamsNotificationArgs::default()
                },
                gotify: GotifyNotificationArgs {
                    url: Some(gotify_url.to_string_lossy().into_owned()),
                    token: Some(gotify_token.to_string_lossy().into_owned()),
                    ..GotifyNotificationArgs::default()
                },
                ..NotificationArgs::default()
            },
            logging: LoggingArgs::default(),
            containers: Vec::new(),
        };

        let mut config: WatchtowerConfig = cli.try_into().expect("config resolves");
        config.resolve_secret_references().expect("secrets resolve");

        assert_eq!(config.http_api.token.as_deref(), Some("secret-token"));
        assert_eq!(
            config.notifications.urls,
            vec![
                "https://example.test/first".to_string(),
                "https://example.test/second".to_string(),
            ]
        );
        assert_eq!(
            config.notifications.email.password.as_deref(),
            Some("mail-secret")
        );
        assert_eq!(
            config.notifications.slack.hook_url.as_deref(),
            Some("https://hooks.slack.com/services/AAA/BBB/CCC")
        );
        assert_eq!(
            config.notifications.msteams.hook.as_deref(),
            Some("https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc")
        );
        assert_eq!(
            config.notifications.gotify.url.as_deref(),
            Some("https://gotify.local/")
        );
        assert_eq!(
            config.notifications.gotify.token.as_deref(),
            Some("gotify-secret")
        );
    }

    #[test]
    fn porcelain_mode_enables_logger_output_and_report_template() {
        let cli = WatchtowerCli {
            docker: DockerArgs::default(),
            scheduling: SchedulingArgs::default(),
            update: UpdateArgs::default(),
            selection: SelectionArgs::default(),
            http_api: HttpApiArgs::default(),
            notifications: NotificationArgs {
                porcelain: Some(PorcelainVersion::V1),
                ..NotificationArgs::default()
            },
            logging: LoggingArgs::default(),
            containers: Vec::new(),
        };

        let config: WatchtowerConfig = cli.try_into().expect("config resolves");

        assert!(config.notifications.log_stdout);
        assert!(config.notifications.report);
        assert_eq!(
            config.notifications.template.as_deref(),
            Some("porcelain.v1.summary-no-log")
        );
        assert!(
            config
                .notifications
                .urls
                .iter()
                .any(|url| url == "logger://")
        );
    }

    fn write_temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        path.push(format!("watchtower-rs-{name}-{}-{stamp}.txt", std::process::id()));
        fs::write(&path, content).expect("temp file should be written");
        path
    }
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
        saw_digit = false;
    }

    if !saw_unit || current != 0 {
        return Err(format!("invalid duration {trimmed:?}"));
    }

    Ok(Duration::from_secs(total))
}
