#![forbid(unsafe_code)]

use std::sync::OnceLock;

use regex::Regex;

use super::email::{NotificationUrlError, encode_component};
use super::notifier::COLOR_HEX;
use super::notifier::COLOR_INT;

/// Notification settings for Slack-compatible hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackSettings<'a> {
    pub hook_url: &'a str,
    pub username: &'a str,
    pub icon_emoji: &'a str,
    pub icon_url: &'a str,
}

/// Build a legacy Slack or Discord notification URL.
pub fn build_slack_url(settings: &SlackSettings<'_>) -> Result<String, NotificationUrlError> {
    let trimmed = settings.hook_url.trim_end_matches('/');
    let stripped = trimmed.strip_prefix("https://").unwrap_or(trimmed);
    let parts: Vec<&str> = stripped.split('/').collect();

    match parts.first().copied() {
        Some("discord.com") | Some("discordapp.com") => build_discord_url(&parts, settings),
        _ => build_slack_webhook_url(settings),
    }
}

fn build_discord_url(
    parts: &[&str],
    settings: &SlackSettings<'_>,
) -> Result<String, NotificationUrlError> {
    if parts.len() < 5 {
        return Err(NotificationUrlError::InvalidUrl(
            settings.hook_url.to_string(),
        ));
    }

    let channel = parts[parts.len() - 3];
    let token = parts[parts.len() - 2];
    let mut query = String::new();
    if !settings.icon_url.is_empty() {
        query.push_str("avatar=");
        query.push_str(&encode_component(settings.icon_url));
        query.push('&');
    }
    query.push_str("color=0x");
    query.push_str(&format!("{:x}", COLOR_INT));
    query.push_str("&colordebug=0x0&colorerror=0x0&colorinfo=0x0&colorwarn=0x0&username=");
    query.push_str(&encode_component(settings.username));

    Ok(format!("discord://{}@{}?{}", token, channel, query))
}

fn build_slack_webhook_url(settings: &SlackSettings<'_>) -> Result<String, NotificationUrlError> {
    let webhook_token = normalize_slack_token(settings.hook_url)
        .ok_or_else(|| NotificationUrlError::InvalidUrl(settings.hook_url.to_string()))?;

    let mut url = String::from("slack://");
    url.push_str(&webhook_token);
    url.push_str("@webhook?botname=");
    url.push_str(&encode_component(settings.username));
    url.push_str("&color=");
    url.push_str(&encode_component(COLOR_HEX));

    if !settings.icon_url.is_empty() {
        url.push_str("&icon=");
        url.push_str(&encode_component(settings.icon_url));
    } else if !settings.icon_emoji.is_empty() {
        url.push_str("&icon=");
        url.push_str(&encode_component(settings.icon_emoji));
    }

    Ok(url)
}

fn normalize_slack_token(hook_url: &str) -> Option<String> {
    static SLACK_TOKEN_PATTERN: OnceLock<Regex> = OnceLock::new();

    let token = hook_url
        .strip_prefix("https://hooks.slack.com/services/")
        .unwrap_or(hook_url);

    let captures = SLACK_TOKEN_PATTERN
        .get_or_init(|| {
            Regex::new(r"^(?:(xox.|hook)[-:]|:?)([A-Z0-9]{9,})([-/,])([A-Z0-9]{9,})([-/,])([A-Za-z0-9]{24,})$")
                .expect("legacy slack token regex is valid")
        })
        .captures(token)?;

    if captures.get(3)?.as_str() != captures.get(5)?.as_str() {
        return None;
    }

    let type_identifier = captures
        .get(1)
        .map(|capture| capture.as_str())
        .filter(|capture| !capture.is_empty())
        .unwrap_or("hook");

    Some(format!(
        "{}:{}-{}-{}",
        type_identifier,
        captures.get(2)?.as_str(),
        captures.get(4)?.as_str(),
        captures.get(6)?.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_discord_slack_url_matches_legacy_format() {
        let settings = SlackSettings {
            hook_url: "https://discord.com/api/webhooks/123456789/abcdef/slack",
            username: "containrrrbot",
            icon_emoji: "",
            icon_url: "https://containrrr.dev/watchtower-sq180.png",
        };

        let url = build_slack_url(&settings).expect("discord url should build");

        assert_eq!(
            url,
            "discord://abcdef@123456789?avatar=https%3A%2F%2Fcontainrrr.dev%2Fwatchtower-sq180.png&color=0x406170&colordebug=0x0&colorerror=0x0&colorinfo=0x0&colorwarn=0x0&username=containrrrbot"
        );
    }

    #[test]
    fn build_slack_webhook_url_matches_legacy_format() {
        let settings = SlackSettings {
            hook_url: concat!(
                "https://hooks.",
                "slack.com/services/",
                "AAAAAAAAA/BBBBBBBBB/123456789123456789123456",
            ),
            username: "containrrrbot",
            icon_emoji: "whale",
            icon_url: "",
        };

        let url = build_slack_url(&settings).expect("slack url should build");

        assert_eq!(
            url,
            "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook?botname=containrrrbot&color=%23406170&icon=whale"
        );
    }

    #[test]
    fn build_slack_webhook_url_accepts_raw_hook_tokens() {
        let settings = SlackSettings {
            hook_url: "AAAAAAAAA/BBBBBBBBB/123456789123456789123456",
            username: "containrrrbot",
            icon_emoji: "",
            icon_url: "https://containrrr.dev/watchtower-sq180.png",
        };

        let url = build_slack_url(&settings).expect("slack token should build");

        assert_eq!(
            url,
            "slack://hook:AAAAAAAAA-BBBBBBBBB-123456789123456789123456@webhook?botname=containrrrbot&color=%23406170&icon=https%3A%2F%2Fcontainrrr.dev%2Fwatchtower-sq180.png"
        );
    }

    #[test]
    fn build_slack_webhook_url_rejects_mismatched_token_separators() {
        let settings = SlackSettings {
            hook_url: "AAAAAAAAA/BBBBBBBBB-123456789123456789123456",
            username: "containrrrbot",
            icon_emoji: "",
            icon_url: "",
        };

        assert_eq!(
            build_slack_url(&settings),
            Err(NotificationUrlError::InvalidUrl(
                "AAAAAAAAA/BBBBBBBBB-123456789123456789123456".to_string()
            ))
        );
    }
}
