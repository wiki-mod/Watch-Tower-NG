#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::process::Command;

use tracing::debug;
use url::Url;

use crate::registry::helpers;
use crate::types::TokenResponse;

/// HTTP header containing registry challenge instructions.
pub const CHALLENGE_HEADER: &str = "WWW-Authenticate";

/// Challenge request description mirroring the Go `http.Request` setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRequest {
    pub url: String,
    pub accept: String,
    pub user_agent: String,
    pub authorization: Option<String>,
}

/// Authentication failures raised by the registry helper surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    InvalidImageReference(String),
    InvalidRealm(String),
    InvalidChallengeHeader,
    NoCredentialsAvailable,
    UnsupportedChallenge,
    ChallengeRequestFailed(String),
    InvalidTokenResponse(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImageReference(reason) => {
                write!(f, "invalid image reference: {reason}")
            }
            Self::InvalidRealm(reason) => write!(f, "invalid auth realm: {reason}"),
            Self::InvalidChallengeHeader => {
                f.write_str("challenge header did not include all values needed to construct an auth url")
            }
            Self::NoCredentialsAvailable => f.write_str("no credentials available"),
            Self::UnsupportedChallenge => {
                f.write_str("unsupported challenge type from registry")
            }
            Self::ChallengeRequestFailed(reason) => {
                write!(f, "challenge request failed: {reason}")
            }
            Self::InvalidTokenResponse(reason) => {
                write!(f, "invalid token response: {reason}")
            }
        }
    }
}

impl Error for AuthError {}

/// Fetch a token for the registry hosting the provided image.
pub fn get_token(image_ref: &str, registry_auth: &str) -> Result<String, AuthError> {
    let challenge_url = get_challenge_url(image_ref)?;
    let request = get_challenge_request(&challenge_url);

    debug!(url = %challenge_url, "Built challenge URL");

    let response = execute_challenge_request(&request)?;
    let challenge = response
        .headers
        .get(CHALLENGE_HEADER)
        .cloned()
        .unwrap_or_default();

    debug!(
        status = %response.status,
        header = %challenge,
        "Got response to challenge request"
    );

    let challenge = challenge.to_ascii_lowercase();
    if challenge.starts_with("basic") {
        if registry_auth.is_empty() {
            return Err(AuthError::NoCredentialsAvailable);
        }

        return build_basic_authorization_header(registry_auth);
    }

    if challenge.starts_with("bearer") {
        return get_bearer_header(&challenge, image_ref, registry_auth);
    }

    Err(AuthError::UnsupportedChallenge)
}

/// Create the request used to retrieve challenge instructions.
pub fn get_challenge_request(url: &Url) -> ChallengeRequest {
    ChallengeRequest {
        url: url.as_str().to_string(),
        accept: "*/*".to_string(),
        user_agent: "Watchtower (Docker)".to_string(),
        authorization: None,
    }
}

/// Fetch a bearer token from the registry based on challenge instructions.
pub fn get_bearer_header(
    challenge: &str,
    image_ref: &str,
    registry_auth: &str,
) -> Result<String, AuthError> {
    let auth_url = get_auth_url(challenge, image_ref)?;
    let mut request = get_challenge_request(&auth_url);

    if !registry_auth.is_empty() {
        debug!("Credentials found.");
        request.authorization = Some(build_basic_authorization_header(registry_auth)?);
    } else {
        debug!("No credentials found.");
    }

    let response = execute_challenge_request(&request)?;
    let token_response: TokenResponse = serde_json::from_slice(&response.body)
        .map_err(|err| AuthError::InvalidTokenResponse(err.to_string()))?;

    Ok(format!("Bearer {}", token_response.token))
}

/// Build the auth URL from the challenge instructions.
pub fn get_auth_url(challenge: &str, image_ref: &str) -> Result<Url, AuthError> {
    let lowered = challenge.to_ascii_lowercase();
    let raw = lowered.strip_prefix("bearer").unwrap_or(&lowered);

    let mut values = HashMap::new();
    for pair in raw.split(',') {
        let trimmed = pair.trim_matches(' ');
        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };

        values.insert(key.to_string(), val.trim_matches('"').to_string());
    }

    debug!(
        realm = values.get("realm").cloned().unwrap_or_default(),
        service = values.get("service").cloned().unwrap_or_default(),
        "Checking challenge header content"
    );

    let Some(realm) = values.get("realm") else {
        return Err(AuthError::InvalidChallengeHeader);
    };
    let Some(service) = values.get("service") else {
        return Err(AuthError::InvalidChallengeHeader);
    };
    if realm.is_empty() || service.is_empty() {
        return Err(AuthError::InvalidChallengeHeader);
    }

    let scope_image = scope_image_from_reference(image_ref)?;
    let scope = format!("repository:{scope_image}:pull");

    debug!(scope = %scope, image = %image_ref, "Setting scope for auth token");

    let auth_url = Url::parse(realm).map_err(|err| AuthError::InvalidRealm(err.to_string()))?;
    let (realm_base, mut pairs) = split_realm_query(&auth_url);
    pairs.push(("service".to_string(), service.clone()));
    pairs.push(("scope".to_string(), scope));
    pairs.sort_by(|left, right| left.0.cmp(&right.0));

    let mut rebuilt = String::new();
    for (index, (key, value)) in pairs.iter().enumerate() {
        if index > 0 {
            rebuilt.push('&');
        }
        rebuilt.push_str(&percent_encode_query_component(key));
        rebuilt.push('=');
        rebuilt.push_str(&percent_encode_query_component(value));
    }

    let final_url = if rebuilt.is_empty() {
        realm_base
    } else {
        format!("{realm_base}?{rebuilt}")
    };

    Url::parse(&final_url).map_err(|err| AuthError::InvalidRealm(err.to_string()))
}

/// Return the URL used to check registry auth requirements.
pub fn get_challenge_url(image_ref: &str) -> Result<Url, AuthError> {
    let host = helpers::get_registry_address(image_ref)
        .map_err(|err| AuthError::InvalidImageReference(err.to_string()))?;

    Url::parse(&format!("https://{host}/v2/"))
        .map_err(|err| AuthError::InvalidImageReference(err.to_string()))
}

fn build_basic_authorization_header(registry_auth: &str) -> Result<String, AuthError> {
    if registry_auth.is_empty() {
        return Err(AuthError::NoCredentialsAvailable);
    }

    Ok(format!("Basic {registry_auth}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChallengeResponse {
    status: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn execute_challenge_request(request: &ChallengeRequest) -> Result<ChallengeResponse, AuthError> {
    let mut command = Command::new("curl");
    command.args([
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--request",
        "GET",
        "--header",
        &format!("Accept: {}", request.accept),
        "--header",
        &format!("User-Agent: {}", request.user_agent),
        "--dump-header",
        "-",
        "--output",
        "-",
    ]);

    if let Some(authorization) = request.authorization.as_deref() {
        command.args(["--header", &format!("Authorization: {authorization}")]);
    }

    command.arg(&request.url);

    let output = command
        .output()
        .map_err(|err| AuthError::ChallengeRequestFailed(format!("curl: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let reason = if stderr.is_empty() {
            format!("curl exited with {}", output.status)
        } else {
            format!("curl exited with {}: {stderr}", output.status)
        };
        return Err(AuthError::ChallengeRequestFailed(reason));
    }

    parse_curl_response(&output.stdout)
}

fn parse_curl_response(output: &[u8]) -> Result<ChallengeResponse, AuthError> {
    let response = String::from_utf8_lossy(output);
    let (header_block, body) = response
        .rsplit_once("\r\n\r\n")
        .or_else(|| response.rsplit_once("\n\n"))
        .ok_or_else(|| {
            AuthError::ChallengeRequestFailed("unexpected challenge response format".to_string())
        })?;

    let mut headers = HashMap::new();
    let mut status = String::new();

    for line in header_block.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }

        if line.starts_with("HTTP/") {
            status = line.to_string();
            continue;
        }

        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Ok(ChallengeResponse {
        status,
        headers,
        body: body.as_bytes().to_vec(),
    })
}

fn scope_image_from_reference(image_ref: &str) -> Result<String, AuthError> {
    let trimmed = image_ref.trim();
    if trimmed.is_empty() {
        return Err(AuthError::InvalidImageReference(
            "image reference must not be empty".to_string(),
        ));
    }

    if trimmed != image_ref {
        return Err(AuthError::InvalidImageReference(image_ref.to_string()));
    }

    let name_ref = strip_tag_and_digest(trimmed);
    let (host, remainder) = split_registry_and_path(name_ref);

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

fn strip_tag_and_digest(image_ref: &str) -> &str {
    let name_ref = image_ref.split_once('@').map_or(image_ref, |(left, _)| left);
    let slash_pos = name_ref.rfind('/');
    let tag_pos = name_ref.rfind(':');

    if let Some(tag_pos) = tag_pos {
        if slash_pos.is_none_or(|slash_pos| tag_pos > slash_pos) {
            return &name_ref[..tag_pos];
        }
    }

    name_ref
}

fn split_registry_and_path(image_ref: &str) -> (String, String) {
    if let Some((registry, remainder)) = image_ref.split_once('/') {
        if is_registry_component(registry) {
            (normalize_registry_host(registry), remainder.to_string())
        } else {
            ("index.docker.io".to_string(), image_ref.to_string())
        }
    } else {
        ("index.docker.io".to_string(), format!("library/{image_ref}"))
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

fn split_realm_query(url: &Url) -> (String, Vec<(String, String)>) {
    let realm = url.as_str();
    let (base, raw_query) = realm
        .split_once('?')
        .map_or((realm, ""), |(left, right)| (left, right));

    let mut pairs = Vec::new();
    if !raw_query.is_empty() {
        for raw_pair in raw_query.split('&') {
            if raw_pair.is_empty() {
                continue;
            }

            let (key, value) = raw_pair
                .split_once('=')
                .map_or((raw_pair, ""), |(left, right)| (left, right));
            if !key.is_empty() {
                pairs.push((key.to_string(), value.to_string()));
            }
        }
    }

    (base.to_string(), pairs)
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
                use std::fmt::Write as _;
                let _ = write!(encoded, "%{:02X}", byte);
            }
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn builds_the_challenge_request() {
        let url = Url::parse("https://ghcr.io/v2/").expect("url");
        let request = get_challenge_request(&url);

        assert_eq!(request.url, "https://ghcr.io/v2/");
        assert_eq!(request.accept, "*/*");
        assert_eq!(request.user_agent, "Watchtower (Docker)");
        assert_eq!(request.authorization, None);
    }

    #[test]
    fn builds_bearer_auth_url_like_the_legacy_flow() {
        let url = get_auth_url(
            "bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\"",
            "marrrrrrrrry/watchtower:latest",
        )
        .expect("auth url");

        assert_eq!(
            url.as_str(),
            "https://ghcr.io/token?scope=repository%3Amarrrrrrrrry%2Fwatchtower%3Apull&service=ghcr.io"
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
    fn rejects_missing_basic_credentials() {
        assert_eq!(
            build_basic_authorization_header("").unwrap_err(),
            AuthError::NoCredentialsAvailable
        );
    }

    #[test]
    fn fetches_a_bearer_header_from_a_registry_token_response() {
        let server = spawn_test_server(
            |request| {
                assert!(request.starts_with(
                    "GET /token?scope=repository%3Awatchtower%3Apull&service=ghcr.io HTTP/1.1"
                ));
                assert!(request.contains("User-Agent: Watchtower (Docker)"));
                assert!(request.contains("Accept: */*"));
                assert!(request.contains("Authorization: Basic dXNlcjpwYXNz"));
            },
            |response| {
                response.push_str("HTTP/1.1 200 OK\r\n");
                response.push_str("Content-Type: application/json\r\n");
                response.push_str("Content-Length: 18\r\n\r\n");
                response.push_str(r#"{"token":"abc123"}"#);
            },
        );

        let challenge = format!(
            "bearer realm=\"http://{}/token\",service=\"ghcr.io\"",
            server.addr
        );
        let header = get_bearer_header(&challenge, "ghcr.io/watchtower", "dXNlcjpwYXNz")
            .expect("bearer header should resolve");

        assert_eq!(header, "Bearer abc123");
    }

    #[derive(Debug)]
    struct TestServer {
        addr: String,
        join: Option<thread::JoinHandle<()>>,
    }

    fn spawn_test_server(
        verify: impl Fn(&str) + Send + 'static,
        write_response: impl Fn(&mut String) + Send + 'static,
    ) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr").to_string();
        let (ready_tx, ready_rx) = mpsc::channel();

        let join = thread::spawn(move || {
            ready_tx.send(()).expect("signal ready");
            let (mut stream, _) = listener.accept().expect("accept client");
            let request = read_http_request(&mut stream);
            verify(&request);

            let mut response = String::new();
            write_response(&mut response);
            stream.write_all(response.as_bytes()).expect("write response");
        });

        ready_rx.recv().expect("wait for ready");
        TestServer {
            addr,
            join: Some(join),
        }
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut reader = BufReader::new(stream);
        let mut request = String::new();

        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read request line");
            request.push_str(&line);
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }

        request
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }
}
