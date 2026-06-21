#![forbid(unsafe_code)]

/// Return the first colon-delimited segment of a Shoutrrr URL.
pub fn get_scheme(url: &str) -> &str {
    match url.find(':') {
        Some(index) if index > 0 => &url[..index],
        _ => "invalid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_scheme_rejects_missing_or_empty_schemes() {
        assert_eq!(get_scheme("shoutrrr://example"), "shoutrrr");
        assert_eq!(get_scheme("example"), "invalid");
        assert_eq!(get_scheme("://example"), "invalid");
    }
}
