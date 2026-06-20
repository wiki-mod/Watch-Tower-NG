use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

/// `WWW-Authenticate` header name used by registry challenge responses.
pub const CHALLENGE_HEADER: &str = "WWW-Authenticate";

/// Authentication scheme advertised by a registry challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthScheme {
    Basic,
    Bearer(BearerChallenge),
}

/// Parsed `Bearer` challenge parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerChallenge {
    pub realm: String,
    pub service: String,
}

/// Errors raised while evaluating registry challenges or building auth data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    EmptyImageReference,
    UnsupportedChallenge,
    MissingRegistryCredentials,
    MissingChallengeField(&'static str),
    InvalidImageReference(String),
    InvalidRealm(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImageReference => f.write_str("image reference must not be empty"),
            Self::UnsupportedChallenge => {
                f.write_str("unsupported challenge type from registry")
            }
            Self::MissingRegistryCredentials => f.write_str("no credentials available"),
            Self::MissingChallengeField(field) => {
                write!(f, "challenge header did not include required field `{field}`")
            }
            Self::InvalidImageReference(image_ref) => {
                write!(f, "invalid image reference `{image_ref}`")
            }
            Self::InvalidRealm(realm) => write!(f, "invalid challenge realm `{realm}`"),
        }
    }
}

impl Error for AuthError {}

/// Return the registry auth scheme advertised by a challenge header.
pub fn classify_challenge(challenge: &str) -> Result<AuthScheme, AuthError> {
    let challenge = challenge.trim_start();

    if starts_with_ascii_case_insensitive(challenge, "basic") {
        return Ok(AuthScheme::Basic);
    }

    if starts_with_ascii_case_insensitive(challenge, "bearer") {
        let bearer = parse_bearer_challenge(challenge)?;
        return Ok(AuthScheme::Bearer(bearer));
    }

    Err(AuthError::UnsupportedChallenge)
}

/// Build the `Authorization` header used when credentials are available.
pub fn build_basic_authorization_header(registry_auth: &str) -> Result<String, AuthError> {
    let registry_auth = registry_auth.trim();
    if registry_auth.is_empty() {
        return Err(AuthError::MissingRegistryCredentials);
    }

    Ok(format!("Basic {registry_auth}"))
}

/// Build the `Authorization` header used for the token request.
///
/// Bearer challenges do not require a request header when no registry
/// credentials are available, so this helper returns `Ok(None)` in that case.
pub fn build_token_request_authorization_header(
    registry_auth: Option<&str>,
) -> Result<Option<String>, AuthError> {
    match registry_auth {
        Some(value) => Ok(Some(build_basic_authorization_header(value)?)),
        None => Ok(None),
    }
}

/// Build the bearer token request URL from a registry challenge and image ref.
pub fn build_bearer_auth_url(challenge: &str, image_ref: &str) -> Result<String, AuthError> {
    let bearer = match classify_challenge(challenge)? {
        AuthScheme::Bearer(bearer) => bearer,
        AuthScheme::Basic => return Err(AuthError::UnsupportedChallenge),
    };

    let scope_image = scope_image_from_reference(image_ref)?;
    let scope = format!("repository:{scope_image}:pull");

    let (realm_base, mut pairs) = split_realm_query(&bearer.realm)?;
    pairs.push(("service".to_string(), bearer.service));
    pairs.push(("scope".to_string(), scope));
    pairs.sort_by(|left, right| left.0.cmp(&right.0));

    let mut query = String::new();
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index > 0 {
            query.push('&');
        }
        query.push_str(&percent_encode_query_component(key));
        query.push('=');
        query.push_str(&percent_encode_query_component(value));
    }

    if query.is_empty() {
        return Err(AuthError::InvalidRealm(bearer.realm));
    }

    Ok(format!("{realm_base}?{query}"))
}

fn parse_bearer_challenge(challenge: &str) -> Result<BearerChallenge, AuthError> {
    let mut fields = Vec::new();
    let mut remainder = challenge.trim_start();

    remainder = strip_ascii_case_insensitive_prefix(remainder, "bearer").unwrap_or(remainder);

    for raw_field in remainder.split(',') {
        let field = raw_field.trim();
        if field.is_empty() {
            continue;
        }

        let Some((key, value)) = field.split_once('=') else {
            continue;
        };

        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"').to_string();
        if !key.is_empty() {
            fields.push((key, value));
        }
    }

    let realm = fields
        .iter()
        .find(|(key, value)| key == "realm" && !value.is_empty())
        .map(|(_, value)| value.clone())
        .ok_or(AuthError::MissingChallengeField("realm"))?;

    let service = fields
        .iter()
        .find(|(key, value)| key == "service" && !value.is_empty())
        .map(|(_, value)| value.clone())
        .ok_or(AuthError::MissingChallengeField("service"))?;

    Ok(BearerChallenge { realm, service })
}

fn scope_image_from_reference(image_ref: &str) -> Result<String, AuthError> {
    let trimmed = image_ref.trim();
    if trimmed.is_empty() {
        return Err(AuthError::EmptyImageReference);
    }

    if trimmed != image_ref {
        return Err(AuthError::InvalidImageReference(image_ref.to_string()));
    }

    let name_ref = trimmed.split_once('@').map_or(trimmed, |(left, _)| left);
    let (host, remainder) = split_registry_and_path(name_ref)?;

    let scope = if host == "index.docker.io" {
        normalize_docker_hub_path(&remainder)
    } else {
        remainder
    };

    if scope.is_empty() {
        return Err(AuthError::InvalidImageReference(image_ref.to_string()));
    }

    Ok(scope)
}

fn split_registry_and_path(image_ref: &str) -> Result<(String, String), AuthError> {
    if let Some((registry, remainder)) = image_ref.split_once('/') {
        if is_registry_component(registry) {
            Ok((normalize_registry_host(registry), remainder.to_string()))
        } else {
            Ok(("index.docker.io".to_string(), image_ref.to_string()))
        }
    } else {
        Ok(("index.docker.io".to_string(), format!("library/{image_ref}")))
    }
}

fn is_registry_component(component: &str) -> bool {
    component.contains('.') || component.contains(':') || component == "localhost"
}

fn normalize_registry_host(host: &str) -> String {
    if host == "docker.io" {
        "index.docker.io".to_string()
    } else {
        host.to_string()
    }
}

fn normalize_docker_hub_path(path: &str) -> String {
    if path.matches('/').count() == 0 {
        format!("library/{path}")
    } else {
        path.to_string()
    }
}

fn split_realm_query(realm: &str) -> Result<(String, Vec<(String, String)>), AuthError> {
    let realm = realm.trim();
    if realm.is_empty() {
        return Err(AuthError::InvalidRealm(realm.to_string()));
    }

    let (base, raw_query) = realm.split_once('?').map_or((realm, ""), |(left, right)| (left, right));
    if base.is_empty() {
        return Err(AuthError::InvalidRealm(realm.to_string()));
    }

    let mut pairs = Vec::new();
    if !raw_query.is_empty() {
        for raw_pair in raw_query.split('&') {
            if raw_pair.is_empty() {
                continue;
            }

            let (key, value) = raw_pair.split_once('=').map_or((raw_pair, ""), |(left, right)| (left, right));
            if !key.is_empty() {
                pairs.push((key.to_string(), value.to_string()));
            }
        }
    }

    Ok((base.to_string(), pairs))
}

fn percent_encode_query_component(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());

    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                let _ = write!(encoded, "%{:02X}", byte);
            }
        }
    }

    encoded
}

fn starts_with_ascii_case_insensitive(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn strip_ascii_case_insensitive_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if starts_with_ascii_case_insensitive(value, prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_basic_and_bearer_challenges() {
        assert_eq!(CHALLENGE_HEADER, "WWW-Authenticate");
        assert_eq!(
            classify_challenge("Basic realm=\"registry\"").unwrap(),
            AuthScheme::Basic
        );

        let bearer = match classify_challenge(
            "bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\"",
        )
        .unwrap()
        {
            AuthScheme::Bearer(bearer) => bearer,
            AuthScheme::Basic => panic!("expected bearer challenge"),
        };

        assert_eq!(bearer.realm, "https://ghcr.io/token");
        assert_eq!(bearer.service, "ghcr.io");
    }

    #[test]
    fn rejects_unsupported_challenges() {
        assert_eq!(
            classify_challenge("Digest realm=\"registry\"").unwrap_err(),
            AuthError::UnsupportedChallenge
        );
    }

    #[test]
    fn builds_basic_authorization_header() {
        assert_eq!(
            build_basic_authorization_header("dXNlcjpwYXNz").unwrap(),
            "Basic dXNlcjpwYXNz"
        );
    }

    #[test]
    fn rejects_empty_basic_credentials() {
        assert_eq!(
            build_basic_authorization_header("   ").unwrap_err(),
            AuthError::MissingRegistryCredentials
        );
    }

    #[test]
    fn returns_none_for_missing_token_request_credentials() {
        assert_eq!(
            build_token_request_authorization_header(None).unwrap(),
            None
        );
    }

    #[test]
    fn builds_token_request_header_when_credentials_exist() {
        assert_eq!(
            build_token_request_authorization_header(Some("dXNlcjpwYXNz"))
                .unwrap()
                .as_deref(),
            Some("Basic dXNlcjpwYXNz")
        );
    }

    #[test]
    fn builds_bearer_auth_url_with_docker_hub_scope_encoding() {
        let url = build_bearer_auth_url(
            "bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\",scope=\"repository:user/image:pull\"",
            "marrrrrrrrry/watchtower",
        )
        .unwrap();

        assert_eq!(
            url,
            "https://ghcr.io/token?scope=repository%3Amarrrrrrrrry%2Fwatchtower%3Apull&service=ghcr.io"
        );
    }

    #[test]
    fn builds_bearer_auth_url_for_explicit_registry_references() {
        let url = build_bearer_auth_url(
            "bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\"",
            "ghcr.io/watchtower",
        )
        .unwrap();

        assert_eq!(
            url,
            "https://ghcr.io/token?scope=repository%3Awatchtower%3Apull&service=ghcr.io"
        );
    }

    #[test]
    fn ignores_empty_fields_and_valueless_keys_in_challenge_parsing() {
        let parsed = classify_challenge(
            "bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\",scope=\"repository:user/image:pull\",,valuelesskey",
        )
        .unwrap();

        assert_eq!(
            parsed,
            AuthScheme::Bearer(BearerChallenge {
                realm: "https://ghcr.io/token".to_string(),
                service: "ghcr.io".to_string(),
            })
        );
    }

    #[test]
    fn rejects_missing_bearer_fields() {
        assert_eq!(
            classify_challenge("bearer realm=\"https://ghcr.io/token\"").unwrap_err(),
            AuthError::MissingChallengeField("service")
        );
    }

    #[test]
    fn rejects_blank_image_references() {
        assert_eq!(
            build_bearer_auth_url(
                "bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\"",
                "   ",
            )
            .unwrap_err(),
            AuthError::EmptyImageReference
        );
    }
}
