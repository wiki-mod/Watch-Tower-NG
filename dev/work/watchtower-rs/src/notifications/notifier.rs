#![forbid(unsafe_code)]

use std::time::Duration;

use super::model::{StaticData, TemplateDataInput};

/// Default notification color used by providers that support a CSS hex value.
pub const COLOR_HEX: &str = "#406170";

/// Default notification color used by providers that prefer a numeric value.
pub const COLOR_INT: u32 = 0x406170;

/// Format the notification title the same way as the legacy notifier.
pub fn get_title(hostname: &str, tag: &str) -> String {
    let mut title = String::new();

    if !tag.is_empty() {
        title.push('[');
        title.push_str(tag);
        title.push_str("] ");
    }

    title.push_str("Watchtower updates");

    if !hostname.is_empty() {
        title.push_str(" on ");
        title.push_str(hostname);
    }

    title
}

/// Resolve the static notification data without depending on CLI parsing.
pub fn get_template_data(input: &TemplateDataInput) -> StaticData {
    let host = input.hostname();
    let title = if input.skip_title {
        String::new()
    } else {
        get_title(&host, &input.title_tag())
    };

    StaticData { title, host }
}

/// Return the legacy delay when present, otherwise the explicitly configured delay.
pub fn get_delay(configured_delay: Option<Duration>, legacy_delay: Duration) -> Duration {
    if legacy_delay > Duration::ZERO {
        legacy_delay
    } else {
        configured_delay
            .filter(|delay| *delay > Duration::ZERO)
            .unwrap_or(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_title_uses_simple_default_without_host_or_tag() {
        assert_eq!(get_title("", ""), "Watchtower updates");
    }

    #[test]
    fn get_title_applies_tag_and_hostname() {
        assert_eq!(
            get_title("test.host", "PREFIX"),
            "[PREFIX] Watchtower updates on test.host"
        );
    }

    #[test]
    fn get_template_data_prefers_explicit_hostname_and_tag() {
        let input = TemplateDataInput {
            configured_hostname: Some("test.host".to_string()),
            fallback_hostname: Some("machine.local".to_string()),
            skip_title: false,
            title_tag: Some("PREFIX".to_string()),
            legacy_email_subject_tag: Some("LEGACY".to_string()),
        };

        assert_eq!(
            get_template_data(&input),
            StaticData {
                title: "[PREFIX] Watchtower updates on test.host".to_string(),
                host: "test.host".to_string(),
            }
        );
    }

    #[test]
    fn get_template_data_uses_legacy_tag_fallback() {
        let input = TemplateDataInput {
            fallback_hostname: Some("machine.local".to_string()),
            legacy_email_subject_tag: Some("LEGACY".to_string()),
            ..TemplateDataInput::default()
        };

        assert_eq!(
            get_template_data(&input),
            StaticData {
                title: "[LEGACY] Watchtower updates on machine.local".to_string(),
                host: "machine.local".to_string(),
            }
        );
    }

    #[test]
    fn get_template_data_can_skip_title() {
        let input = TemplateDataInput {
            configured_hostname: Some("test.host".to_string()),
            skip_title: true,
            title_tag: Some("PREFIX".to_string()),
            ..TemplateDataInput::default()
        };

        assert_eq!(
            get_template_data(&input),
            StaticData {
                title: String::new(),
                host: "test.host".to_string(),
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

    #[test]
    fn get_delay_uses_configured_delay_when_no_legacy_delay_exists() {
        assert_eq!(
            get_delay(Some(Duration::from_secs(5)), Duration::ZERO),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn get_delay_falls_back_to_zero() {
        assert_eq!(get_delay(None, Duration::ZERO), Duration::ZERO);
        assert_eq!(
            get_delay(Some(Duration::ZERO), Duration::ZERO),
            Duration::ZERO
        );
    }
}
