#![forbid(unsafe_code)]

use thiserror::Error;
use url::form_urlencoded::byte_serialize;

/// Errors returned while building legacy notification service URLs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NotificationUrlError {
    /// The supplied URL is structurally invalid for the requested provider.
    #[error("invalid notification url: {0}")]
    InvalidUrl(String),

    /// A required field was empty.
    #[error("missing notification field: {0}")]
    MissingField(&'static str),
}

/// Notification settings for the email provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailSettings<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub server: &'a str,
    pub user: &'a str,
    pub password: &'a str,
    pub port: u16,
    pub tls_skip_verify: bool,
}

/// Build a legacy email notification URL.
pub fn build_email_url(settings: &EmailSettings<'_>) -> Result<String, NotificationUrlError> {
    if settings.from.is_empty() {
        return Err(NotificationUrlError::MissingField("from"));
    }
    if settings.to.is_empty() {
        return Err(NotificationUrlError::MissingField("to"));
    }
    if settings.server.is_empty() {
        return Err(NotificationUrlError::MissingField("server"));
    }

    let auth = if settings.user.is_empty() { "None" } else { "Plain" };
    let mut url = String::from("smtp://");

    if !settings.user.is_empty() {
        url.push_str(&encode_component(settings.user));
        url.push(':');
        url.push_str(&encode_component(settings.password));
        url.push('@');
    }

    url.push_str(settings.server);
    url.push(':');
    url.push_str(&settings.port.to_string());
    url.push_str("/?auth=");
    url.push_str(auth);
    if settings.tls_skip_verify {
        url.push_str("&encryption=None");
    }
    url.push_str("&fromaddress=");
    url.push_str(&encode_component(settings.from));
    url.push_str("&fromname=Watchtower&subject=&toaddresses=");
    url.push_str(&encode_component(settings.to));
    if settings.tls_skip_verify {
        url.push_str("&usestarttls=No");
    }

    Ok(url)
}

/// URL percent-encode a string component.
pub(super) fn encode_component(value: &str) -> String {
    byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_email_url_matches_legacy_format() {
        let settings = EmailSettings {
            from: "lala@example.com",
            to: "mail@example.com",
            server: "mail.containrrr.dev",
            user: "containrrrbot",
            password: "secret-password",
            port: 25,
            tls_skip_verify: false,
        };

        let url = build_email_url(&settings).expect("email url should build");

        assert_eq!(
            url,
            "smtp://containrrrbot:secret-password@mail.containrrr.dev:25/?auth=Plain&fromaddress=lala%40example.com&fromname=Watchtower&subject=&toaddresses=mail%40example.com"
        );
    }

    #[test]
    fn build_email_url_matches_legacy_format_when_tls_skip_verify_is_enabled() {
        let settings = EmailSettings {
            from: "lala@example.com",
            to: "mail@example.com",
            server: "mail.containrrr.dev",
            user: "containrrrbot",
            password: "secret-password",
            port: 25,
            tls_skip_verify: true,
        };

        let url = build_email_url(&settings).expect("email url should build");

        assert_eq!(
            url,
            "smtp://containrrrbot:secret-password@mail.containrrr.dev:25/?auth=Plain&encryption=None&fromaddress=lala%40example.com&fromname=Watchtower&subject=&toaddresses=mail%40example.com&usestarttls=No"
        );
    }
}
