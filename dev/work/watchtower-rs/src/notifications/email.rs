#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::time::Duration;

use crate::types::{ConvertibleNotifier, DelayNotifier};
use url::form_urlencoded::byte_serialize;

/// Error type for notification URL building failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationUrlError {
    /// A required field is missing.
    MissingField(&'static str),
    /// The URL is structurally invalid.
    InvalidUrl(String),
}

impl fmt::Display for NotificationUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationUrlError::MissingField(field) => {
                write!(f, "missing notification field: {}", field)
            }
            NotificationUrlError::InvalidUrl(msg) => write!(f, "invalid notification url: {}", msg),
        }
    }
}

impl Error for NotificationUrlError {}

/// Email notifier implementation.
#[derive(Debug, Clone)]
pub struct EmailTypeNotifier {
    pub from: String,
    pub to: String,
    pub server: String,
    pub user: String,
    pub password: String,
    pub port: u16,
    pub tls_skip_verify: bool,
    pub delay: Duration,
}

impl EmailTypeNotifier {
    /// Create a new email notifier with the provided configuration.
    ///
    /// # Arguments
    ///
    /// * `from` - Email from address (required)
    /// * `to` - Email to address (required)
    /// * `server` - SMTP server hostname (required)
    /// * `user` - SMTP username (optional)
    /// * `password` - SMTP password (optional)
    /// * `port` - SMTP port (default: 25)
    /// * `tls_skip_verify` - Whether to skip TLS verification
    /// * `delay` - Delay before sending notification
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        server: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
        port: u16,
        tls_skip_verify: bool,
        delay: Duration,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            server: server.into(),
            user: user.into(),
            password: password.into(),
            port,
            tls_skip_verify,
            delay,
        }
    }

    /// Build the SMTP URL for this notifier.
    ///
    /// # Returns
    ///
    /// A URL string suitable for use with the shoutrrr SMTP service,
    /// or an error if required fields are missing.
    fn build_url(&self) -> Result<String, Box<dyn Error + Send + Sync + 'static>> {
        if self.from.is_empty() {
            return Err("missing notification field: from".into());
        }
        if self.to.is_empty() {
            return Err("missing notification field: to".into());
        }
        if self.server.is_empty() {
            return Err("missing notification field: server".into());
        }

        let auth = if self.user.is_empty() {
            "None"
        } else {
            "Plain"
        };

        let mut url = String::from("smtp://");

        if !self.user.is_empty() {
            url.push_str(&encode_component(&self.user));
            url.push(':');
            url.push_str(&encode_component(&self.password));
            url.push('@');
        }

        url.push_str(&self.server);
        url.push(':');
        url.push_str(&self.port.to_string());
        url.push_str("/?auth=");
        url.push_str(auth);

        if self.tls_skip_verify {
            url.push_str("&encryption=None");
        }

        url.push_str("&fromaddress=");
        url.push_str(&encode_component(&self.from));
        url.push_str("&fromname=Watchtower&subject=&toaddresses=");
        url.push_str(&encode_component(&self.to));

        if self.tls_skip_verify {
            url.push_str("&usestarttls=No");
        }

        Ok(url)
    }
}

impl ConvertibleNotifier for EmailTypeNotifier {
    fn get_url(
        &self,
        _command: &clap::Command,
    ) -> Result<String, Box<dyn Error + Send + Sync + 'static>> {
        self.build_url()
    }
}

impl DelayNotifier for EmailTypeNotifier {
    fn get_delay(&self) -> Duration {
        self.delay
    }
}

/// Email notification settings for legacy compatibility.
///
/// This structure replaces the old Cobra-based flag loading.
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

/// Build a legacy email notification URL from settings.
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

    let auth = if settings.user.is_empty() {
        "None"
    } else {
        "Plain"
    };

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
    fn build_url_matches_legacy_format() {
        let notifier = EmailTypeNotifier::new(
            "lala@example.com",
            "mail@example.com",
            "mail.containrrr.dev",
            "containrrrbot",
            "secret-password",
            25,
            false,
            Duration::from_secs(0),
        );

        let url = notifier.build_url().expect("email url should build");

        assert_eq!(
            url,
            "smtp://containrrrbot:secret-password@mail.containrrr.dev:25/?auth=Plain&fromaddress=lala%40example.com&fromname=Watchtower&subject=&toaddresses=mail%40example.com"
        );
    }

    #[test]
    fn build_url_matches_legacy_format_when_tls_skip_verify_is_enabled() {
        let notifier = EmailTypeNotifier::new(
            "lala@example.com",
            "mail@example.com",
            "mail.containrrr.dev",
            "containrrrbot",
            "secret-password",
            25,
            true,
            Duration::from_secs(0),
        );

        let url = notifier.build_url().expect("email url should build");

        assert_eq!(
            url,
            "smtp://containrrrbot:secret-password@mail.containrrr.dev:25/?auth=Plain&encryption=None&fromaddress=lala%40example.com&fromname=Watchtower&subject=&toaddresses=mail%40example.com&usestarttls=No"
        );
    }

    #[test]
    fn build_url_returns_error_when_from_is_missing() {
        let notifier = EmailTypeNotifier::new(
            "",
            "mail@example.com",
            "mail.containrrr.dev",
            "",
            "",
            25,
            false,
            Duration::from_secs(0),
        );

        assert!(notifier.build_url().is_err());
    }

    #[test]
    fn build_url_returns_error_when_to_is_missing() {
        let notifier = EmailTypeNotifier::new(
            "lala@example.com",
            "",
            "mail.containrrr.dev",
            "",
            "",
            25,
            false,
            Duration::from_secs(0),
        );

        assert!(notifier.build_url().is_err());
    }

    #[test]
    fn build_url_returns_error_when_server_is_missing() {
        let notifier = EmailTypeNotifier::new(
            "lala@example.com",
            "mail@example.com",
            "",
            "",
            "",
            25,
            false,
            Duration::from_secs(0),
        );

        assert!(notifier.build_url().is_err());
    }

    #[test]
    fn get_delay_returns_configured_delay() {
        let notifier = EmailTypeNotifier::new(
            "lala@example.com",
            "mail@example.com",
            "mail.containrrr.dev",
            "",
            "",
            25,
            false,
            Duration::from_secs(30),
        );

        assert_eq!(notifier.get_delay(), Duration::from_secs(30));
    }
}
