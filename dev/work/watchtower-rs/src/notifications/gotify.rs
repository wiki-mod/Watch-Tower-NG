#![forbid(unsafe_code)]

use url::Url;

use super::email::NotificationUrlError;

/// Notification settings for Gotify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotifySettings<'a> {
    pub api_url: &'a str,
    pub token: &'a str,
    pub tls_skip_verify: bool,
}

/// Build a legacy Gotify notification URL.
pub fn build_gotify_url(settings: &GotifySettings<'_>) -> Result<String, NotificationUrlError> {
    let parsed = Url::parse(settings.api_url)
        .map_err(|err| NotificationUrlError::InvalidUrl(err.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(NotificationUrlError::InvalidUrl(
                settings.api_url.to_string(),
            ))
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| NotificationUrlError::InvalidUrl(settings.api_url.to_string()))?;
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
}
