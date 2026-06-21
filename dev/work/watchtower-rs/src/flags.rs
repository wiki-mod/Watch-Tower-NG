#![forbid(unsafe_code)]

//! Legacy flag helper logic translated from `old-source/internal/flags/flags.go`.
//!
//! The derive-based CLI in `cli.rs` owns the actual argument registration, but
//! the legacy program behavior around log formatting, secret expansion, and
//! duration parsing lives here so the Rust rewrite keeps the same semantics in
//! one place.

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

use crate::cli::{LoggingConfig, LogFormat, LogLevel, WatchtowerConfig};

/// Minimum Docker API version accepted by Watchtower.
pub const DOCKER_API_MIN_VERSION: &str = "1.52";

/// Default polling interval used by the legacy program model.
#[allow(dead_code)]
pub const DEFAULT_INTERVAL_SECONDS: u64 = 24 * 60 * 60;

/// Return the legacy default polling interval.
#[allow(dead_code)]
pub fn default_interval() -> Duration {
    Duration::from_secs(DEFAULT_INTERVAL_SECONDS)
}

/// Return the current value of an environment variable or the empty string.
#[allow(dead_code)]
pub fn env_string(key: &str) -> String {
    env::var(key).unwrap_or_default()
}

/// Return the current value of an environment variable as a string slice list.
#[allow(dead_code)]
pub fn env_string_slice(key: &str) -> Vec<String> {
    match env::var(key) {
        Ok(value) => value
            .split([',', ' '])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Return the current value of an environment variable as an integer.
#[allow(dead_code)]
pub fn env_int(key: &str) -> i64 {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or_default()
}

/// Return the current value of an environment variable as a boolean.
#[allow(dead_code)]
pub fn env_bool(key: &str) -> bool {
    match env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "t" | "yes" | "y" | "on"
        ),
        Err(_) => false,
    }
}

/// Return the current value of an environment variable as a duration.
#[allow(dead_code)]
pub fn env_duration(key: &str) -> Duration {
    env::var(key)
        .ok()
        .and_then(|value| parse_duration(&value).ok())
        .unwrap_or_default()
}

/// Read the legacy update flags from resolved configuration.
#[allow(dead_code)]
pub fn read_flags(config: &WatchtowerConfig) -> (bool, bool, bool, Duration) {
    (
        config.update.cleanup,
        config.update.no_restart,
        config.update.monitor_only,
        config.scheduling.stop_timeout,
    )
}

/// Parse the legacy duration syntax used by the command line surface.
///
/// The accepted forms are:
/// - plain seconds, for example `60`
/// - mixed unit suffixes such as `1h30m`, `15m`, `20s`, or `2d`
pub fn parse_duration(input: &str) -> Result<Duration, String> {
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

/// Resolve the legacy logging level after applying debug/trace overrides.
pub fn effective_log_level(logging: &LoggingConfig) -> LogLevel {
    if logging.trace {
        LogLevel::Trace
    } else if logging.debug {
        LogLevel::Debug
    } else {
        logging.log_level
    }
}

/// Choose the legacy log output mode.
pub fn resolved_log_format(logging: &LoggingConfig) -> ResolvedLogFormat {
    match logging.log_format {
        LogFormat::Auto => {
            if std::io::stdout().is_terminal() {
                ResolvedLogFormat::Pretty
            } else {
                ResolvedLogFormat::Logfmt
            }
        }
        LogFormat::Json => ResolvedLogFormat::Json,
        LogFormat::Logfmt => ResolvedLogFormat::Logfmt,
        LogFormat::Pretty => ResolvedLogFormat::Pretty,
    }
}

/// Apply the logging configuration to the global tracing subscriber.
pub fn setup_logging(logging: &LoggingConfig) {
    let filter = tracing_subscriber::EnvFilter::new(effective_log_level(logging).to_string());
    let ansi_enabled = !logging.no_color;

    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(ansi_enabled);

    match resolved_log_format(logging) {
        ResolvedLogFormat::Pretty => {
            let _ = builder.pretty().try_init();
        }
        ResolvedLogFormat::Json => {
            let _ = builder.json().try_init();
        }
        ResolvedLogFormat::Logfmt => {
            let _ = builder.compact().try_init();
        }
    }
}

/// Legacy log output modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedLogFormat {
    Pretty,
    Json,
    Logfmt,
}

/// Expand `WatchtowerConfig` secret file references in place.
pub fn resolve_secret_references(config: &mut WatchtowerConfig) -> io::Result<()> {
    config.http_api.token = expand_optional_secret(config.http_api.token.take())?;
    config.notifications.urls = expand_secret_list(config.notifications.urls.clone())?;
    config.notifications.email.password =
        expand_optional_secret(config.notifications.email.password.take())?;
    config.notifications.slack.hook_url =
        expand_optional_secret(config.notifications.slack.hook_url.take())?;
    config.notifications.msteams.hook =
        expand_optional_secret(config.notifications.msteams.hook.take())?;
    config.notifications.gotify.url = expand_optional_secret(config.notifications.gotify.url.take())?;
    config.notifications.gotify.token =
        expand_optional_secret(config.notifications.gotify.token.take())?;
    Ok(())
}

/// Expand a list of secret references into raw values.
pub fn expand_secret_list(values: Vec<String>) -> io::Result<Vec<String>> {
    let mut expanded = Vec::new();

    for value in values {
        if is_file_reference(&value) {
            let file = File::open(&value)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line?;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                expanded.push(trimmed.to_string());
            }
        } else {
            expanded.push(value);
        }
    }

    Ok(expanded)
}

/// Expand a single secret reference into its raw value.
pub fn expand_secret_value(value: &str) -> io::Result<String> {
    if is_file_reference(value) {
        let mut file = File::open(value)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(content.trim().to_string())
    } else {
        Ok(value.to_string())
    }
}

fn expand_optional_secret(value: Option<String>) -> io::Result<Option<String>> {
    match value {
        Some(value) => Ok(Some(expand_secret_value(&value)?)),
        None => Ok(None),
    }
}

/// Detect whether a string points to a readable file reference.
///
/// Paths with a colon in the second character position are still accepted so
/// Windows drive letters like `c:\path\to\file` continue to work.
pub fn is_file_reference(value: &str) -> bool {
    let first_colon = value.find(':');
    if matches!(first_colon, Some(index) if index != 1) {
        return false;
    }

    Path::new(value).exists()
}

/// Build a compact description of the Docker flags and defaults.
#[allow(dead_code)]
pub fn docker_defaults() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("host", "unix:///var/run/docker.sock"),
        ("tlsverify", "false"),
        ("api-version", DOCKER_API_MIN_VERSION),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{
        DockerConfig, HttpApiConfig, NotificationConfig, NotificationLogLevel, WatchtowerConfig,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn default_interval_matches_legacy_value() {
        assert_eq!(default_interval(), Duration::from_secs(DEFAULT_INTERVAL_SECONDS));
    }

    #[test]
    fn parse_duration_supports_plain_seconds_and_units() {
        assert_eq!(parse_duration("60").expect("seconds"), Duration::from_secs(60));
        assert_eq!(parse_duration("1h30m").expect("units"), Duration::from_secs(5400));
        assert_eq!(parse_duration("2d").expect("days"), Duration::from_secs(172800));
    }

    #[test]
    fn parse_duration_rejects_invalid_strings() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("1x").is_err());
    }

    #[test]
    fn effective_log_level_applies_trace_before_debug() {
        let base = LoggingConfig {
            log_level: LogLevel::Warn,
            log_format: LogFormat::Auto,
            debug: false,
            trace: false,
            no_color: false,
            no_startup_message: false,
        };

        assert_eq!(effective_log_level(&base), LogLevel::Warn);
        assert_eq!(effective_log_level(&LoggingConfig { debug: true, ..base }), LogLevel::Debug);
        assert_eq!(effective_log_level(&LoggingConfig { trace: true, ..base }), LogLevel::Trace);
    }

    #[test]
    fn resolved_log_format_follows_legacy_modes() {
        let logging = LoggingConfig {
            log_level: LogLevel::Info,
            log_format: LogFormat::Pretty,
            debug: false,
            trace: false,
            no_color: false,
            no_startup_message: false,
        };

        assert_eq!(resolved_log_format(&logging), ResolvedLogFormat::Pretty);
        assert_eq!(
            resolved_log_format(&LoggingConfig { log_format: LogFormat::Json, ..logging }),
            ResolvedLogFormat::Json
        );
        assert_eq!(
            resolved_log_format(&LoggingConfig { log_format: LogFormat::Logfmt, ..logging }),
            ResolvedLogFormat::Logfmt
        );
    }

    #[test]
    fn file_references_expand_line_by_line() {
        let file = write_temp_file("secret-list", "alpha\n\n beta \n");
        let values = expand_secret_list(vec![file.to_string_lossy().into_owned()]).expect("expands");

        assert_eq!(values, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn file_reference_detection_keeps_windows_drive_letters_valid() {
        assert!(!is_file_reference("plain-text"));
        assert!(!is_file_reference("secret:with:colon"));
    }

    #[test]
    fn resolve_secret_references_expands_all_secret_fields() {
        let http_token = write_temp_file("http-token", "secret-token\n");
        let slack_hook = write_temp_file("slack-hook", "https://hooks.slack.com/services/AAA/BBB/CCC\n");
        let msteams_hook = write_temp_file("msteams-hook", "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc\n");
        let gotify_url = write_temp_file("gotify-url", "https://gotify.local/\n");
        let gotify_token = write_temp_file("gotify-token", "gotify-secret\n");
        let notification_urls = write_temp_file(
            "notification-urls",
            "https://example.test/first\n\nhttps://example.test/second\n",
        );
        let email_password = write_temp_file("email-password", "mail-secret\n");

        let mut config = WatchtowerConfig {
            docker: DockerConfig {
                host: "unix:///var/run/docker.sock".to_string(),
                tlsverify: false,
                api_version: DOCKER_API_MIN_VERSION.to_string(),
            },
            containers: Vec::new(),
            scheduling: crate::cli::SchedulingConfig {
                mode: crate::cli::PollingMode::Interval(Duration::from_secs(60)),
                stop_timeout: Duration::from_secs(10),
                periodic_polls: false,
            },
            update: crate::cli::UpdateConfig {
                no_pull: false,
                no_restart: false,
                cleanup: false,
                remove_volumes: false,
                rolling_restart: false,
                include_restarting: false,
                include_stopped: false,
                revive_stopped: false,
                monitor_only: false,
                run_once: false,
                label_take_precedence: false,
            },
            health_check: false,
            selection: crate::cli::SelectionConfig {
                label_enable: false,
                disable_containers: Vec::new(),
                scope: None,
            },
            http_api: HttpApiConfig {
                update: false,
                metrics: false,
                token: Some(http_token.to_string_lossy().into_owned()),
            },
            notifications: NotificationConfig {
                types: vec!["email".to_string()],
                level: NotificationLogLevel::Info,
                delay: None,
                hostname: None,
                urls: vec![notification_urls.to_string_lossy().into_owned()],
                report: false,
                template: None,
                title_tag: None,
                skip_title: false,
                log_stdout: false,
                porcelain: None,
                warn_on_head_failure: crate::cli::WarnOnHeadFailure::Auto,
                email: crate::cli::EmailNotificationConfig {
                    from: None,
                    to: None,
                    server: None,
                    user: None,
                    password: Some(email_password.to_string_lossy().into_owned()),
                    port: 25,
                    tls_skip_verify: false,
                    delay: None,
                    subject_tag: None,
                },
                slack: crate::cli::SlackNotificationConfig {
                    hook_url: Some(slack_hook.to_string_lossy().into_owned()),
                    identifier: "watchtower".to_string(),
                    channel: None,
                    icon_emoji: None,
                    icon_url: None,
                },
                msteams: crate::cli::TeamsNotificationConfig {
                    hook: Some(msteams_hook.to_string_lossy().into_owned()),
                    data: false,
                },
                gotify: crate::cli::GotifyNotificationConfig {
                    url: Some(gotify_url.to_string_lossy().into_owned()),
                    token: Some(gotify_token.to_string_lossy().into_owned()),
                    tls_skip_verify: false,
                },
            },
            logging: LoggingConfig {
                log_level: LogLevel::Info,
                log_format: LogFormat::Auto,
                debug: false,
                trace: false,
                no_color: false,
                no_startup_message: false,
            },
        };

        resolve_secret_references(&mut config).expect("secrets resolve");

        assert_eq!(config.http_api.token.as_deref(), Some("secret-token"));
        assert_eq!(
            config.notifications.urls,
            vec![
                "https://example.test/first".to_string(),
                "https://example.test/second".to_string(),
            ]
        );
        assert_eq!(config.notifications.email.password.as_deref(), Some("mail-secret"));
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
        assert_eq!(config.notifications.gotify.token.as_deref(), Some("gotify-secret"));
    }

    fn write_temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        path.push(format!("watchtower-rs-flags-{name}-{}-{stamp}.txt", std::process::id()));
        fs::write(&path, content).expect("temp file should be written");
        path
    }
}
