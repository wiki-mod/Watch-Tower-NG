#![forbid(unsafe_code)]

use url::Url;

use super::email::{encode_component, NotificationUrlError};
use super::notifier::COLOR_HEX;

/// Legacy notification type string for Microsoft Teams.
pub const MS_TEAMS_TYPE: &str = "msteams";

/// Notification settings for Microsoft Teams hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamsSettings<'a> {
    pub hook_url: &'a str,
}

/// Pure input bundle that replaces the legacy Cobra-backed Teams notifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsTeamsNotifierInput<'a> {
    pub hook_url: &'a str,
    pub data: bool,
}

/// Typed translation of the legacy Microsoft Teams notifier state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsTeamsNotifier {
    pub webhook_url: String,
    pub data: bool,
}

impl MsTeamsNotifier {
    /// Build the final Shoutrrr URL for this notifier.
    pub fn get_url(&self) -> Result<String, NotificationUrlError> {
        build_teams_url(&TeamsSettings {
            hook_url: self.webhook_url.as_str(),
        })
    }
}

/// Build the legacy Microsoft Teams notifier from typed inputs.
pub fn new_msteams_notifier(
    input: &MsTeamsNotifierInput<'_>,
) -> Result<MsTeamsNotifier, NotificationUrlError> {
    if input.hook_url.is_empty() {
        return Err(NotificationUrlError::MissingField(
            "notification-msteams-hook",
        ));
    }

    Ok(MsTeamsNotifier {
        webhook_url: input.hook_url.to_string(),
        data: input.data,
    })
}

/// Build a legacy Microsoft Teams notification URL.
pub fn build_teams_url(settings: &TeamsSettings<'_>) -> Result<String, NotificationUrlError> {
    let parsed = Url::parse(settings.hook_url)
        .map_err(|err| NotificationUrlError::InvalidUrl(err.to_string()))?;
    let segments: Vec<_> = parsed
        .path_segments()
        .ok_or_else(|| NotificationUrlError::InvalidUrl(settings.hook_url.to_string()))?
        .collect();

    if segments.len() != 5 || segments[0] != "webhook" || segments[2] != "IncomingWebhook" {
        return Err(NotificationUrlError::InvalidUrl(
            settings.hook_url.to_string(),
        ));
    }

    Ok(format!(
        "teams://{}/{}/{}?color={}",
        segments[1],
        segments[3],
        segments[4],
        encode_component(COLOR_HEX)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_teams_url_matches_legacy_format() {
        let settings = TeamsSettings {
            hook_url: "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc",
        };

        let url = build_teams_url(&settings).expect("teams url should build");

        assert_eq!(url, "teams://aaa/bbb/ccc?color=%23406170");
    }

    #[test]
    fn new_msteams_notifier_requires_webhook() {
        let error = new_msteams_notifier(&MsTeamsNotifierInput {
            hook_url: "",
            data: false,
        })
        .expect_err("missing hook should fail");

        assert_eq!(
            error,
            NotificationUrlError::MissingField("notification-msteams-hook")
        );
    }

    #[test]
    fn new_msteams_notifier_preserves_data_and_builds_legacy_url() {
        let notifier = new_msteams_notifier(&MsTeamsNotifierInput {
            hook_url: "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc",
            data: true,
        })
        .expect("msteams notifier should build");

        assert_eq!(MS_TEAMS_TYPE, "msteams");
        assert!(notifier.data);
        assert_eq!(
            notifier.webhook_url,
            "https://outlook.office.com/webhook/aaa/IncomingWebhook/bbb/ccc"
        );
        assert_eq!(
            notifier.get_url().expect("teams url should build"),
            "teams://aaa/bbb/ccc?color=%23406170"
        );
    }
}
