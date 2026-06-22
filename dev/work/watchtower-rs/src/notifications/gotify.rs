#![forbid(unsafe_code)]

use url::Url;

/// Notification settings for Gotify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotifySettings<'a> {
    pub api_url: &'a str,
    pub token: &'a str,
    pub tls_skip_verify: bool,
}

/// Notifier for Gotify notifications.
pub struct GotifyTypeNotifier {
    gotify_url: String,
    gotify_app_token: String,
    gotify_insecure_skip_verify: bool,
}

impl GotifyTypeNotifier {
    /// Create a new Gotify notifier from validated settings.
    ///
    /// # Arguments
    ///
    /// * `url` - The Gotify API URL (must start with http:// or https://)
    /// * `token` - The Gotify application token (required, cannot be empty)
    /// * `skip_verify` - Whether to skip TLS verification
    ///
    /// # Errors
    ///
    /// Returns an error if URL or token are invalid/empty.
    pub fn new(url: &str, token: &str, skip_verify: bool) -> Result<Self, String> {
        let api_url = Self::get_gotify_url(url)?;
        let app_token = Self::get_gotify_token(token)?;

        Ok(GotifyTypeNotifier {
            gotify_url: api_url,
            gotify_app_token: app_token,
            gotify_insecure_skip_verify: skip_verify,
        })
    }

    /// Validate and return the Gotify token.
    ///
    /// # Errors
    ///
    /// Returns an error if token is empty.
    fn get_gotify_token(token: &str) -> Result<String, String> {
        if token.is_empty() {
            return Err("Required argument --notification-gotify-token(cli) or \
                 WATCHTOWER_NOTIFICATION_GOTIFY_TOKEN(env) is empty."
                .to_string());
        }
        Ok(token.to_string())
    }

    /// Validate and return the Gotify URL.
    ///
    /// # Errors
    ///
    /// Returns an error if URL is empty or does not start with http:// or https://.
    fn get_gotify_url(url: &str) -> Result<String, String> {
        if url.is_empty() {
            return Err("Required argument --notification-gotify-url(cli) or \
                 WATCHTOWER_NOTIFICATION_GOTIFY_URL(env) is empty."
                .to_string());
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Gotify URL must start with \"http://\" or \"https://\"".to_string());
        }

        if url.starts_with("http://") {
            // Warn about insecure HTTP URL
            // Note: In production, use log::warn! after adding log dependency
            // log::warn!("Using an HTTP url for Gotify is insecure");
        }

        Ok(url.to_string())
    }

    /// Get the notification URL for use with Shoutrrr.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be parsed.
    pub fn get_url(&self) -> Result<String, String> {
        build_gotify_url(&GotifySettings {
            api_url: &self.gotify_url,
            token: &self.gotify_app_token,
            tls_skip_verify: self.gotify_insecure_skip_verify,
        })
    }
}

/// Build a legacy Gotify notification URL for Shoutrrr.
pub fn build_gotify_url(settings: &GotifySettings<'_>) -> Result<String, String> {
    let parsed = Url::parse(settings.api_url)
        .map_err(|err| format!("Invalid URL: {err}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(format!(
                "Invalid URL scheme: {}",
                settings.api_url
            ));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("No host in URL: {}", settings.api_url))?;
    let path = parsed.path().trim_end_matches('/');

    let mut url = String::from("gotify://");
    url.push_str(host);
    if !path.is_empty() {
        url.push_str(path);
    }
    url.push('/');
    url.push_str(settings.token);
    url.push_str("?title=");

    // tls_skip_verify is preserved in the settings but not encoded in the URL
    // (Shoutrrr handles TLS skip via separate configuration)
    let _ = settings.tls_skip_verify;

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_gotify_url_matches_legacy_format() {
        let settings = GotifySettings {
            api_url: "https://shoutrrr.local",
            token: "aaa",
            tls_skip_verify: false,
        };

        let url = build_gotify_url(&settings).expect("gotify url should build");

        assert_eq!(url, "gotify://shoutrrr.local/aaa?title=");
    }

    #[test]
    fn build_gotify_url_with_path() {
        let settings = GotifySettings {
            api_url: "https://shoutrrr.local/gotify",
            token: "token123",
            tls_skip_verify: false,
        };

        let url = build_gotify_url(&settings).expect("gotify url should build");

        assert_eq!(url, "gotify://shoutrrr.local/gotify/token123?title=");
    }

    #[test]
    fn build_gotify_url_rejects_invalid_scheme() {
        let settings = GotifySettings {
            api_url: "ftp://shoutrrr.local",
            token: "token",
            tls_skip_verify: false,
        };

        let result = build_gotify_url(&settings);
        assert!(result.is_err());
    }

    #[test]
    fn get_gotify_token_rejects_empty() {
        let result = GotifyTypeNotifier::get_gotify_token("");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("WATCHTOWER_NOTIFICATION_GOTIFY_TOKEN")
        );
    }

    #[test]
    fn get_gotify_token_accepts_valid() {
        let result = GotifyTypeNotifier::get_gotify_token("valid_token");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "valid_token");
    }

    #[test]
    fn get_gotify_url_rejects_empty() {
        let result = GotifyTypeNotifier::get_gotify_url("");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("WATCHTOWER_NOTIFICATION_GOTIFY_URL")
        );
    }

    #[test]
    fn get_gotify_url_rejects_missing_schema() {
        let result = GotifyTypeNotifier::get_gotify_url("shoutrrr.local");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("http://\" or \"https://"));
    }

    #[test]
    fn get_gotify_url_accepts_https() {
        let result = GotifyTypeNotifier::get_gotify_url("https://shoutrrr.local");
        assert!(result.is_ok());
    }

    #[test]
    fn get_gotify_url_accepts_http_with_warning() {
        // Note: This would produce a warning log, but we still accept it
        let result = GotifyTypeNotifier::get_gotify_url("http://shoutrrr.local");
        assert!(result.is_ok());
    }

    #[test]
    fn new_creates_valid_notifier() {
        let result = GotifyTypeNotifier::new("https://shoutrrr.local", "test_token", false);
        assert!(result.is_ok());
        let notifier = result.unwrap();
        assert_eq!(notifier.gotify_url, "https://shoutrrr.local");
        assert_eq!(notifier.gotify_app_token, "test_token");
        assert!(!notifier.gotify_insecure_skip_verify);
    }

    #[test]
    fn new_rejects_empty_token() {
        let result = GotifyTypeNotifier::new("https://shoutrrr.local", "", false);
        assert!(result.is_err());
    }

    #[test]
    fn new_rejects_empty_url() {
        let result = GotifyTypeNotifier::new("", "token", false);
        assert!(result.is_err());
    }

    #[test]
    fn get_url_returns_valid_shoutrrr_url() {
        let notifier = GotifyTypeNotifier::new("https://shoutrrr.local", "abc123", false)
            .expect("should create notifier");

        let url = notifier.get_url().expect("should build url");
        assert_eq!(url, "gotify://shoutrrr.local/abc123?title=");
    }
}
