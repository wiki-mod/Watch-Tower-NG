#![forbid(unsafe_code)]

//! Startup summary helpers translated from the legacy Go root command.
//!
//! The real scheduling and update loop are still outside this module. It only
//! formats the observable startup messages that were previously emitted by the
//! Go entrypoint.

use std::time::Duration;

use crate::{filters, meta, AppConfig};

/// Build the legacy startup message sequence for a resolved configuration.
pub fn build_startup_messages(config: &AppConfig) -> Vec<String> {
    if config.no_startup_message {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(format!("Watchtower {}", meta::version()));

    if config.notification_types.is_empty() {
        lines.push("Using no notifications".to_string());
    } else {
        lines.push(format!(
            "Using notifications: {}",
            config.notification_types.join(", ")
        ));
    }

    lines.push(filters::build_filter_description(
        &config.containers,
        &config.disable_containers,
        config.label_enable,
        config.scope.as_deref().unwrap_or(""),
    ));

    match (&config.schedule, config.interval) {
        (Some(schedule), _) => {
            lines.push(format!("Scheduling updates with cron expression {schedule}."));
        }
        (None, Some(interval)) if interval > Duration::ZERO => {
            lines.push(format!(
                "Periodic runs are enabled every {}.",
                format_duration(interval)
            ));
        }
        _ if config.run_once => {
            lines.push("Running a one time update.".to_string());
        }
        _ => {
            lines.push("Periodic runs are not enabled.".to_string());
        }
    }

    if config.enable_http_update_api {
        lines.push("The HTTP API is enabled at :8080.".to_string());
    }

    if config.trace_enabled {
        lines.push(
            "Trace level enabled: log will include sensitive information as credentials and tokens"
                .to_string(),
        );
    }

    lines
}

/// Emit startup messages through tracing.
pub fn emit_startup_messages(config: &AppConfig) {
    for line in build_startup_messages(config) {
        tracing::info!("{line}");
    }
}

/// Format a duration similar to the legacy Go helper.
pub fn format_duration(duration: Duration) -> String {
    let mut parts = Vec::new();
    let hours = duration.as_secs() / 3600;
    let minutes = (duration.as_secs() / 60) % 60;
    let seconds = duration.as_secs() % 60;

    if hours == 1 {
        parts.push("1 hour".to_string());
    } else if hours != 0 {
        parts.push(format!("{hours} hours"));
    }

    if hours != 0 && (minutes != 0 || seconds != 0) {
        parts.push(String::new());
    }

    if minutes == 1 {
        parts.push("1 minute".to_string());
    } else if minutes != 0 {
        parts.push(format!("{minutes} minutes"));
    }

    if minutes != 0 && seconds != 0 {
        parts.push(String::new());
    }

    if seconds == 1 {
        parts.push("1 second".to_string());
    } else if seconds != 0 || (hours == 0 && minutes == 0) {
        parts.push(format!("{seconds} seconds"));
    }

    parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        AppConfig {
            notification_types: vec!["email".to_string(), "slack".to_string()],
            no_startup_message: false,
            trace_enabled: false,
            schedule: None,
            interval: None,
            run_once: false,
            enable_http_update_api: true,
            scope: Some("prod".to_string()),
            ..AppConfig::default()
        }
    }

    #[test]
    fn format_duration_matches_legacy_style() {
        assert_eq!(format_duration(Duration::from_secs(1)), "1 second");
        assert_eq!(format_duration(Duration::from_secs(61)), "1 minute, 1 second");
        assert_eq!(
            format_duration(Duration::from_secs(3661)),
            "1 hour, 1 minute, 1 second"
        );
    }

    #[test]
    fn build_startup_messages_covers_notifications_scope_and_api() {
        let messages = build_startup_messages(&config());

        assert_eq!(messages[1], "Using notifications: email, slack");
        assert_eq!(messages[2], "Only checking containers in scope \"prod\"");
        assert!(messages.iter().any(|message| message == "The HTTP API is enabled at :8080."));
    }
}
