#![forbid(unsafe_code)]

//! Legacy notification setup translated from `old-source/pkg/notifications/notifier.go`.
//!
//! The old Go file did not send notifications itself. It assembled the final
//! notifier state from CLI/config inputs:
//! - resolve the notification title and hostname
//! - append legacy service URLs to explicit URLs
//! - choose the effective delay
//! - keep the template selection consistent with legacy behavior
//!
//! This module keeps that orchestration in one place so the Rust codebase can
//! use a single, typed entrypoint instead of Cobra flag lookups.

use std::env;
use std::fs;
use std::process::Command;
use std::time::Duration;

use thiserror::Error;

use crate::notifications::{
    EmailSettings, GotifySettings, NotificationUrlError, SlackSettings, StaticData, TeamsSettings,
    TemplateDataInput, build_email_url, build_gotify_url, build_slack_url, build_teams_url,
    get_delay as legacy_get_delay, get_template_data as legacy_get_template_data,
};

const EMAIL_TYPE: &str = "email";
const SLACK_TYPE: &str = "slack";
const MS_TEAMS_TYPE: &str = "msteams";
const GOTIFY_TYPE: &str = "gotify";
const SHOUTRRR_TYPE: &str = "shoutrrr";

/// Notification log levels accepted by the legacy notifier entrypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationLogLevel {
    Panic,
    Fatal,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl NotificationLogLevel {
    /// Parse the legacy log-level string.
    pub fn parse(value: &str) -> Result<Self, NotifierError> {
        match value.to_ascii_lowercase().as_str() {
            "panic" => Ok(Self::Panic),
            "fatal" => Ok(Self::Fatal),
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            other => Err(NotifierError::InvalidNotificationLevel(other.to_string())),
        }
    }
}

impl From<crate::cli::NotificationLogLevel> for NotificationLogLevel {
    fn from(value: crate::cli::NotificationLogLevel) -> Self {
        match value {
            crate::cli::NotificationLogLevel::Panic => Self::Panic,
            crate::cli::NotificationLogLevel::Fatal => Self::Fatal,
            crate::cli::NotificationLogLevel::Error => Self::Error,
            crate::cli::NotificationLogLevel::Warn => Self::Warn,
            crate::cli::NotificationLogLevel::Info => Self::Info,
            crate::cli::NotificationLogLevel::Debug => Self::Debug,
            crate::cli::NotificationLogLevel::Trace => Self::Trace,
        }
    }
}

/// Legacy email notification settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmailNotificationSettings {
    pub from: Option<String>,
    pub to: Option<String>,
    pub server: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub port: u16,
    pub tls_skip_verify: bool,
    pub delay: Option<Duration>,
}

/// Legacy Slack notification settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlackNotificationSettings {
    pub hook_url: Option<String>,
    pub identifier: String,
    pub icon_emoji: Option<String>,
    pub icon_url: Option<String>,
}

/// Legacy Microsoft Teams notification settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamsNotificationSettings {
    pub hook: Option<String>,
}

/// Legacy Gotify notification settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GotifyNotificationSettings {
    pub url: Option<String>,
    pub token: Option<String>,
    pub tls_skip_verify: bool,
}

/// Resolved configuration for the legacy notifier entrypoint.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotifierInput {
    pub types: Vec<String>,
    pub level: NotificationLogLevel,
    pub delay: Option<Duration>,
    pub hostname: Option<String>,
    pub urls: Vec<String>,
    pub report: bool,
    pub template: Option<String>,
    pub title_tag: Option<String>,
    pub legacy_email_subject_tag: Option<String>,
    pub skip_title: bool,
    pub log_stdout: bool,
    pub email: EmailNotificationSettings,
    pub slack: SlackNotificationSettings,
    pub msteams: TeamsNotificationSettings,
    pub gotify: GotifyNotificationSettings,
}

impl From<&crate::cli::NotificationConfig> for NotifierInput {
    fn from(config: &crate::cli::NotificationConfig) -> Self {
        Self {
            types: config.types.clone(),
            level: config.level.into(),
            delay: config.delay,
            hostname: config.hostname.clone(),
            urls: config.urls.clone(),
            report: config.report,
            template: config.template.clone(),
            title_tag: config.title_tag.clone(),
            legacy_email_subject_tag: config.email.subject_tag.clone(),
            skip_title: config.skip_title,
            log_stdout: config.log_stdout,
            email: EmailNotificationSettings {
                from: config.email.from.clone(),
                to: config.email.to.clone(),
                server: config.email.server.clone(),
                user: config.email.user.clone(),
                password: config.email.password.clone(),
                port: config.email.port,
                tls_skip_verify: config.email.tls_skip_verify,
                delay: config.email.delay,
            },
            slack: SlackNotificationSettings {
                hook_url: config.slack.hook_url.clone(),
                identifier: config.slack.identifier.clone(),
                icon_emoji: config.slack.icon_emoji.clone(),
                icon_url: config.slack.icon_url.clone(),
            },
            msteams: TeamsNotificationSettings {
                hook: config.msteams.hook.clone(),
            },
            gotify: GotifyNotificationSettings {
                url: config.gotify.url.clone(),
                token: config.gotify.token.clone(),
                tls_skip_verify: config.gotify.tls_skip_verify,
            },
        }
    }
}

/// Result of translating the legacy notifier setup into Rust data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifierSetup {
    /// The explicit notification URLs plus URLs synthesized from legacy flags.
    pub urls: Vec<String>,
    /// Effective notification log level.
    pub level: NotificationLogLevel,
    /// The raw template flag value passed into the legacy createNotifier flow.
    pub template: String,
    /// True when the legacy entry-point should treat entries as a batch.
    pub legacy_template: bool,
    /// Static template data shared across all notifications.
    pub data: StaticData,
    /// Whether notification logs should go to stdout.
    pub stdout: bool,
    /// Delay before dispatching a notification batch.
    pub delay: Duration,
}

/// Errors returned while translating the legacy notifier inputs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NotifierError {
    /// The requested notification level is invalid.
    #[error("invalid notification level: {0}")]
    InvalidNotificationLevel(String),

    /// A legacy notification type was requested but is not supported.
    #[error("unknown notification type: {0}")]
    UnknownNotificationType(String),

    /// URL synthesis for one of the legacy transports failed.
    #[error(transparent)]
    NotificationUrl(#[from] NotificationUrlError),
}

/// Build the legacy notifier state from resolved configuration.
pub fn new_notifier(input: &NotifierInput) -> Result<NotifierSetup, NotifierError> {
    let fallback_hostname = system_hostname();
    let data = get_template_data(input, fallback_hostname.as_deref());
    let (urls, legacy_delay) = append_legacy_urls(input.urls.clone(), input)?;

    Ok(NotifierSetup {
        urls,
        level: input.level,
        template: input.template.clone().unwrap_or_default(),
        legacy_template: !input.report,
        data,
        stdout: input.log_stdout,
        delay: get_delay(input.delay, legacy_delay),
    })
}

/// Build the legacy notifier state directly from the resolved CLI/config surface.
pub fn new_notifier_from_config(
    config: &crate::cli::NotificationConfig,
) -> Result<NotifierSetup, NotifierError> {
    new_notifier(&NotifierInput::from(config))
}

/// Append URLs synthesized from legacy notification flags.
pub fn append_legacy_urls(
    mut urls: Vec<String>,
    input: &NotifierInput,
) -> Result<(Vec<String>, Duration), NotifierError> {
    let mut legacy_delay = Duration::ZERO;

    for notif_type in &input.types {
        match notif_type.as_str() {
            SHOUTRRR_TYPE => continue,
            EMAIL_TYPE => {
                let url = build_email_url(&email_settings(input))?;
                legacy_delay = input.email.delay.unwrap_or(Duration::ZERO);
                urls.push(url);
            }
            SLACK_TYPE => {
                urls.push(build_slack_url(&slack_settings(input))?);
            }
            MS_TEAMS_TYPE => {
                urls.push(build_teams_url(&teams_settings(input))?);
            }
            GOTIFY_TYPE => {
                urls.push(
                    build_gotify_url(&gotify_settings(input))
                        .map_err(NotificationUrlError::InvalidUrl)?,
                );
            }
            other => return Err(NotifierError::UnknownNotificationType(other.to_string())),
        }
    }

    Ok((urls, legacy_delay))
}

/// Resolve the effective delay just like the legacy Go helper.
pub fn get_delay(configured_delay: Option<Duration>, legacy_delay: Duration) -> Duration {
    legacy_get_delay(configured_delay, legacy_delay)
}

/// Format the notification title like the legacy helper.
pub fn get_title(hostname: &str, tag: &str) -> String {
    crate::notifications::get_title(hostname, tag)
}

/// Resolve the static template data from typed configuration.
pub fn get_template_data(input: &NotifierInput, fallback_hostname: Option<&str>) -> StaticData {
    let input = TemplateDataInput {
        configured_hostname: input.hostname.clone(),
        fallback_hostname: fallback_hostname
            .map(str::to_owned)
            .or_else(system_hostname),
        skip_title: input.skip_title,
        title_tag: input.title_tag.clone(),
        legacy_email_subject_tag: input.legacy_email_subject_tag.clone(),
    };

    legacy_get_template_data(&input)
}

/// Return the legacy color hex value.
pub const COLOR_HEX: &str = crate::notifications::COLOR_HEX;

/// Return the legacy color int value.
pub const COLOR_INT: u32 = crate::notifications::COLOR_INT;

fn system_hostname() -> Option<String> {
    read_trimmed_file("/proc/sys/kernel/hostname")
        .or_else(|| read_trimmed_file("/etc/hostname"))
        .or_else(command_hostname)
        .or_else(env_hostname)
}

fn read_trimmed_file(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn command_hostname() -> Option<String> {
    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_hostname() -> Option<String> {
    env::var("HOSTNAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn email_settings(input: &NotifierInput) -> EmailSettings<'_> {
    EmailSettings {
        from: input.email.from.as_deref().unwrap_or(""),
        to: input.email.to.as_deref().unwrap_or(""),
        server: input.email.server.as_deref().unwrap_or(""),
        user: input.email.user.as_deref().unwrap_or(""),
        password: input.email.password.as_deref().unwrap_or(""),
        port: input.email.port,
        tls_skip_verify: input.email.tls_skip_verify,
    }
}

fn slack_settings(input: &NotifierInput) -> SlackSettings<'_> {
    SlackSettings {
        hook_url: input.slack.hook_url.as_deref().unwrap_or(""),
        username: input.slack.identifier.as_str(),
        icon_emoji: input.slack.icon_emoji.as_deref().unwrap_or(""),
        icon_url: input.slack.icon_url.as_deref().unwrap_or(""),
    }
}

fn teams_settings(input: &NotifierInput) -> TeamsSettings<'_> {
    TeamsSettings {
        hook_url: input.msteams.hook.as_deref().unwrap_or(""),
    }
}

fn gotify_settings(input: &NotifierInput) -> GotifySettings<'_> {
    GotifySettings {
        api_url: input.gotify.url.as_deref().unwrap_or(""),
        token: input.gotify.token.as_deref().unwrap_or(""),
        tls_skip_verify: input.gotify.tls_skip_verify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        EmailNotificationConfig, GotifyNotificationConfig, NotificationConfig,
        NotificationLogLevel as CliNotificationLogLevel, SlackNotificationConfig,
        TeamsNotificationConfig, WarnOnHeadFailure,
    };

    fn typed_config() -> NotificationConfig {
        NotificationConfig {
            types: vec![
                "shoutrrr".to_string(),
                "email".to_string(),
                "slack".to_string(),
                "msteams".to_string(),
                "gotify".to_string(),
            ],
            level: CliNotificationLogLevel::Debug,
            delay: Some(Duration::from_secs(5)),
            hostname: Some("configured.host".to_string()),
            urls: vec!["logger://".to_string()],
            report: false,
            template: None,
            title_tag: Some("PREFIX".to_string()),
            skip_title: false,
            log_stdout: true,
            porcelain: None,
            warn_on_head_failure: WarnOnHeadFailure::Auto,
            email: EmailNotificationConfig {
                from: Some("from@example.test".to_string()),
                to: Some("to@example.test".to_string()),
                server: Some("smtp.example.test".to_string()),
                user: Some("user".to_string()),
                password: Some("password".to_string()),
                port: 2525,
                tls_skip_verify: false,
                delay: Some(Duration::from_secs(7)),
                subject_tag: Some("LEGACY".to_string()),
            },
            slack: SlackNotificationConfig {
                hook_url: Some(slack_webhook_url_for_tests()),
                identifier: "watchtower".to_string(),
                channel: Some("alerts".to_string()),
                icon_emoji: Some(":whale:".to_string()),
                icon_url: Some("https://example.test/icon.png".to_string()),
            },
            msteams: TeamsNotificationConfig {
                hook: Some(
                    "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc".to_string(),
                ),
                data: true,
            },
            gotify: GotifyNotificationConfig {
                url: Some("https://gotify.example.test".to_string()),
                token: Some("token".to_string()),
                tls_skip_verify: false,
            },
        }
    }

    fn input() -> NotifierInput {
        let mut input = NotifierInput::from(&typed_config());
        input.level = NotificationLogLevel::Info;
        input
    }

    fn slack_webhook_url_for_tests() -> String {
        concat!(
            "https://hooks.",
            "slack.com/services/",
            "AAAAAAAAA/BBBBBBBBB/123456789123456789123456",
        )
        .to_string()
    }

    #[test]
    fn parse_notification_levels_matches_legacy_names() {
        assert_eq!(
            NotificationLogLevel::parse("warn"),
            Ok(NotificationLogLevel::Warn)
        );
        assert!(matches!(
            NotificationLogLevel::parse("invalid"),
            Err(NotifierError::InvalidNotificationLevel(level)) if level == "invalid"
        ));
        assert_eq!(
            NotificationLogLevel::from(CliNotificationLogLevel::Trace),
            NotificationLogLevel::Trace
        );
    }

    #[test]
    fn notifier_input_from_config_preserves_typed_notification_surface() {
        let config = typed_config();

        assert_eq!(
            NotifierInput::from(&config),
            NotifierInput {
                types: vec![
                    "shoutrrr".to_string(),
                    "email".to_string(),
                    "slack".to_string(),
                    "msteams".to_string(),
                    "gotify".to_string(),
                ],
                level: NotificationLogLevel::Debug,
                delay: Some(Duration::from_secs(5)),
                hostname: Some("configured.host".to_string()),
                urls: vec!["logger://".to_string()],
                report: false,
                template: None,
                title_tag: Some("PREFIX".to_string()),
                legacy_email_subject_tag: Some("LEGACY".to_string()),
                skip_title: false,
                log_stdout: true,
                email: EmailNotificationSettings {
                    from: Some("from@example.test".to_string()),
                    to: Some("to@example.test".to_string()),
                    server: Some("smtp.example.test".to_string()),
                    user: Some("user".to_string()),
                    password: Some("password".to_string()),
                    port: 2525,
                    tls_skip_verify: false,
                    delay: Some(Duration::from_secs(7)),
                },
                slack: SlackNotificationSettings {
                    hook_url: Some(slack_webhook_url_for_tests()),
                    identifier: "watchtower".to_string(),
                    icon_emoji: Some(":whale:".to_string()),
                    icon_url: Some("https://example.test/icon.png".to_string()),
                },
                msteams: TeamsNotificationSettings {
                    hook: Some(
                        "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc"
                            .to_string(),
                    ),
                },
                gotify: GotifyNotificationSettings {
                    url: Some("https://gotify.example.test".to_string()),
                    token: Some("token".to_string()),
                    tls_skip_verify: false,
                },
            }
        );
    }

    #[test]
    fn new_notifier_resolves_the_legacy_setup() {
        let setup = new_notifier(&input()).expect("setup should resolve");

        assert_eq!(
            setup.urls,
            vec![
                "logger://".to_string(),
                "smtp://user:password@smtp.example.test:2525/?auth=Plain&fromaddress=from%40example.test&fromname=Watchtower&subject=&toaddresses=to%40example.test".to_string(),
                "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook?botname=watchtower&color=%23406170&icon=https%3A%2F%2Fexample.test%2Ficon.png".to_string(),
                "teams://aaa/bbb/ccc?color=%23406170".to_string(),
                "gotify://gotify.example.test/token?title=".to_string(),
            ]
        );
        assert_eq!(setup.level, NotificationLogLevel::Info);
        assert_eq!(setup.template, "");
        assert!(setup.legacy_template);
        assert_eq!(
            setup.data,
            StaticData {
                title: "[PREFIX] Watchtower updates on configured.host".to_string(),
                host: "configured.host".to_string(),
            }
        );
        assert!(setup.stdout);
        assert_eq!(setup.delay, Duration::from_secs(7));
    }

    #[test]
    fn new_notifier_from_config_uses_the_typed_bridge() {
        let setup = new_notifier_from_config(&typed_config()).expect("setup should resolve");

        assert_eq!(setup.level, NotificationLogLevel::Debug);
        assert_eq!(
            setup.data,
            StaticData {
                title: "[PREFIX] Watchtower updates on configured.host".to_string(),
                host: "configured.host".to_string(),
            }
        );
        assert_eq!(setup.delay, Duration::from_secs(7));
    }

    #[test]
    fn new_notifier_preserves_the_raw_template_flag() {
        let mut cfg = input();
        cfg.template = Some("default".to_string());

        let setup = new_notifier(&cfg).expect("setup should resolve");

        assert_eq!(setup.template, "default");
        assert!(setup.legacy_template);
    }

    #[test]
    fn append_legacy_urls_rejects_unknown_types() {
        let mut cfg = input();
        cfg.types.push("invalid".to_string());

        let err = append_legacy_urls(Vec::new(), &cfg).expect_err("unknown type should fail");
        assert_eq!(
            err,
            NotifierError::UnknownNotificationType("invalid".to_string())
        );
    }

    #[test]
    fn get_template_data_uses_fallback_hostname_when_needed() {
        let mut cfg = input();
        cfg.hostname = None;
        cfg.skip_title = false;

        let data = get_template_data(&cfg, Some("fallback.host"));
        assert_eq!(
            data,
            StaticData {
                title: "[PREFIX] Watchtower updates on fallback.host".to_string(),
                host: "fallback.host".to_string(),
            }
        );
    }

    #[test]
    fn get_template_data_uses_legacy_email_subject_tag_fallback() {
        let mut cfg = input();
        cfg.title_tag = None;

        let data = get_template_data(&cfg, Some("fallback.host"));
        assert_eq!(
            data,
            StaticData {
                title: "[LEGACY] Watchtower updates on configured.host".to_string(),
                host: "configured.host".to_string(),
            }
        );
    }

    #[test]
    fn get_template_data_can_skip_title() {
        let mut cfg = input();
        cfg.skip_title = true;

        let data = get_template_data(&cfg, Some("fallback.host"));
        assert_eq!(
            data,
            StaticData {
                title: String::new(),
                host: "configured.host".to_string(),
            }
        );
    }

    #[test]
    fn get_delay_prefers_legacy_delay() {
        assert_eq!(
            get_delay(Some(Duration::from_secs(5)), Duration::from_secs(7)),
            Duration::from_secs(7)
        );
    }
}
