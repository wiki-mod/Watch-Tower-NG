#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use thiserror::Error;
use tracing::debug;

use super::{auth, manifest};
use crate::meta;
use crate::types::RegistryCredentials;

/// Docker registry response header containing the image digest.
pub const CONTENT_DIGEST_HEADER: &str = "Docker-Content-Digest";

/// Result type used by the digest helpers.
pub type Result<T> = std::result::Result<T, DigestError>;

/// Errors raised by the digest helpers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DigestError {
    #[error("container image info missing")]
    MissingImageInfo,
    #[error("could not fetch token")]
    MissingToken,
    #[error("{0}")]
    AuthFailed(String),
    #[error("invalid image reference `{image_ref}`: {reason}")]
    InvalidImageReference { image_ref: String, reason: String },
    #[error("registry responded to head request with {status:?}, auth: {auth}")]
    RegistryHeadRejected { status: String, auth: String },
    #[error("registry request failed: {0}")]
    RequestFailed(String),
}

/// Transform a base64-encoded JSON auth blob into a base64 `username:password`
/// payload. Inputs that are already a basic-auth payload are returned unchanged.
pub fn transform_auth(registry_auth: &str) -> String {
    if registry_auth.is_empty() {
        return String::new();
    }

    let decoded = match decode_base64_standard(registry_auth) {
        Ok(decoded) => decoded,
        Err(_) => return registry_auth.to_string(),
    };

    let Ok(decoded) = String::from_utf8(decoded) else {
        return registry_auth.to_string();
    };

    let Ok(credentials) = serde_json::from_str::<RegistryCredentials>(&decoded) else {
        return registry_auth.to_string();
    };

    if credentials.username.is_empty() || credentials.password.is_empty() {
        return registry_auth.to_string();
    }

    encode_base64_standard(format!("{}:{}", credentials.username, credentials.password).as_bytes())
}

/// Compare a registry digest against local repository digests.
pub fn compare_digest<D: AsRef<str>>(
    image_ref: &str,
    repo_digests: &[D],
    registry_auth: &str,
) -> Result<bool> {
    let digest_url = manifest::build_manifest_url(image_ref).map_err(|err| {
        DigestError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: err.to_string(),
        }
    })?;

    compare_digest_impl(
        image_ref,
        repo_digests,
        &digest_url,
        registry_auth,
        |image_ref, registry_auth| {
            auth::get_token(image_ref, registry_auth)
                .map_err(|err| DigestError::AuthFailed(err.to_string()))
        },
    )
}

/// Fetch the remote digest via a `HEAD` request.
pub fn get_digest(url: &str, token: &str) -> Result<String> {
    if token.is_empty() {
        return Err(DigestError::MissingToken);
    }

    let parsed = ParsedUrl::parse(url)?;
    let response = match parsed.scheme.as_str() {
        "http" => head_request_http(&parsed, token)?,
        "https" => head_request_curl(url, token)?,
        _ => {
            return Err(DigestError::RequestFailed(format!(
                "unsupported URL scheme: {}",
                parsed.scheme
            )));
        }
    };

    digest_from_response(&response.status_line, response.headers)
}

fn compare_digest_impl<D, F>(
    token_target: &str,
    repo_digests: &[D],
    digest_url: &str,
    registry_auth: &str,
    token_resolver: F,
) -> Result<bool>
where
    D: AsRef<str>,
    F: FnOnce(&str, &str) -> Result<String>,
{
    let registry_auth = transform_auth(registry_auth);
    let token = token_resolver(token_target, &registry_auth)?;
    if token.is_empty() {
        return Err(DigestError::MissingToken);
    }

    let remote_digest = get_digest(digest_url, &token)?;
    debug!(remote = %remote_digest, "Found a remote digest to compare with");

    // Legacy behavior returns `false` when the container has no repo digests,
    // so an empty slice is not a hard error here.
    for digest in repo_digests {
        let digest = digest.as_ref();
        let local_digest = digest.split('@').nth(1).unwrap();
        debug!(local = %local_digest, remote = %remote_digest, "Comparing");

        if local_digest == remote_digest {
            debug!("Found a match");
            return Ok(true);
        }
    }

    Ok(false)
}

/// Evaluate a registry HEAD response without performing the transport call.
fn digest_from_response(status_line: &str, headers: HashMap<String, String>) -> Result<String> {
    let response = HttpResponse {
        status_line: status_line.to_string(),
        status_code: parse_status_code(status_line)?,
        headers,
    };

    if response.status_code != 200 {
        let auth = response
            .headers
            .get("www-authenticate")
            .cloned()
            .unwrap_or_else(|| "not present".to_string());
        return Err(DigestError::RegistryHeadRejected {
            status: response.status_line,
            auth,
        });
    }

    Ok(response
        .headers
        .get(&CONTENT_DIGEST_HEADER.to_ascii_lowercase())
        .cloned()
        .unwrap_or_default())
}

fn head_request_http(parsed: &ParsedUrl, token: &str) -> Result<HttpResponse> {
    let port = parsed.port.unwrap_or(80);
    let addr = format!("{}:{port}", parsed.host);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|err| DigestError::RequestFailed(format!("{addr}: {err}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|err| DigestError::RequestFailed(err.to_string()))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|err| DigestError::RequestFailed(err.to_string()))?;

    let host_header = parsed.host_header();
    let path = if parsed.path.is_empty() {
        "/"
    } else {
        &parsed.path
    };

    let request = format!(
        concat!(
            "HEAD {path} HTTP/1.1\r\n",
            "Host: {host}\r\n",
            "User-Agent: {user_agent}\r\n",
            "Authorization: {token}\r\n",
            "Accept: application/vnd.docker.distribution.manifest.v2+json\r\n",
            "Accept: application/vnd.docker.distribution.manifest.list.v2+json\r\n",
            "Accept: application/vnd.docker.distribution.manifest.v1+json\r\n",
            "Accept: application/vnd.oci.image.index.v1+json\r\n",
            "Connection: close\r\n",
            "\r\n"
        ),
        path = path,
        host = host_header,
        user_agent = meta::user_agent(),
        token = token
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| DigestError::RequestFailed(err.to_string()))?;

    parse_http_response(stream)
}

fn head_request_curl(url: &str, token: &str) -> Result<HttpResponse> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--head",
            "--insecure",
            "--dump-header",
            "-",
            "--output",
            "/dev/null",
            "--request",
            "HEAD",
            "-H",
            &format!("User-Agent: {}", meta::user_agent()),
            "-H",
            &format!("Authorization: {token}"),
            "-H",
            "Accept: application/vnd.docker.distribution.manifest.v2+json",
            "-H",
            "Accept: application/vnd.docker.distribution.manifest.list.v2+json",
            "-H",
            "Accept: application/vnd.docker.distribution.manifest.v1+json",
            "-H",
            "Accept: application/vnd.oci.image.index.v1+json",
            url,
        ])
        .output()
        .map_err(|err| DigestError::RequestFailed(format!("curl: {err}")))?;

    if !output.status.success() && output.stdout.is_empty() {
        return Err(DigestError::RequestFailed(format!(
            "curl exited with {status}",
            status = output.status
        )));
    }

    parse_http_response(output.stdout.as_slice())
}

fn parse_http_response(mut stream: impl Read) -> Result<HttpResponse> {
    let mut reader = BufReader::new(&mut stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|err| DigestError::RequestFailed(err.to_string()))?;

    if status_line.trim().is_empty() {
        return Err(DigestError::RequestFailed(
            "empty response from registry".to_string(),
        ));
    }

    let status_line = status_line.trim_end_matches(['\r', '\n']).to_string();
    let status_code = parse_status_code(&status_line)?;
    let mut headers = HashMap::new();

    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|err| DigestError::RequestFailed(err.to_string()))?;

        if line.is_empty() {
            break;
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        let Some((name, value)) = trimmed.split_once(':') else {
            continue;
        };

        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    Ok(HttpResponse {
        status_line,
        status_code,
        headers,
    })
}

fn parse_status_code(status_line: &str) -> Result<u16> {
    let mut parts = status_line.splitn(3, ' ');
    let _protocol = parts.next();
    let Some(code) = parts.next() else {
        return Err(DigestError::RequestFailed(format!(
            "invalid response status line: {status_line}"
        )));
    };

    code.parse::<u16>().map_err(|err| {
        DigestError::RequestFailed(format!("invalid response status code {code:?}: {err}"))
    })
}

fn decode_base64_standard(input: &str) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;
    let mut padding = 0usize;

    for byte in input.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }

        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                0
            }
            _ => return Err(DigestError::RequestFailed("invalid base64".to_string())),
        };

        chunk[chunk_len] = value;
        chunk_len += 1;

        if chunk_len == 4 {
            decode_base64_chunk(&chunk, padding, &mut output)?;
            chunk_len = 0;
            padding = 0;
        }
    }

    if chunk_len != 0 {
        return Err(DigestError::RequestFailed("invalid base64".to_string()));
    }

    Ok(output)
}

fn decode_base64_chunk(chunk: &[u8; 4], padding: usize, output: &mut Vec<u8>) -> Result<()> {
    match padding {
        0 => {
            output.push((chunk[0] << 2) | (chunk[1] >> 4));
            output.push((chunk[1] << 4) | (chunk[2] >> 2));
            output.push((chunk[2] << 6) | chunk[3]);
        }
        1 => {
            output.push((chunk[0] << 2) | (chunk[1] >> 4));
            output.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        2 => {
            output.push((chunk[0] << 2) | (chunk[1] >> 4));
        }
        _ => return Err(DigestError::RequestFailed("invalid base64".to_string())),
    }

    Ok(())
}

fn encode_base64_standard(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut index = 0usize;

    while index < input.len() {
        let remaining = input.len() - index;
        let chunk = &input[index..usize::min(index + 3, input.len())];

        let first = chunk[0] >> 2;
        let second = ((chunk[0] & 0b0000_0011) << 4) | (chunk.get(1).copied().unwrap_or(0) >> 4);
        let third = ((chunk.get(1).copied().unwrap_or(0) & 0b0000_1111) << 2)
            | (chunk.get(2).copied().unwrap_or(0) >> 6);
        let fourth = chunk.get(2).copied().unwrap_or(0) & 0b0011_1111;

        output.push(TABLE[first as usize] as char);
        output.push(TABLE[second as usize] as char);
        if remaining > 1 {
            output.push(TABLE[third as usize] as char);
        } else {
            output.push('=');
        }
        if remaining > 2 {
            output.push(TABLE[fourth as usize] as char);
        } else {
            output.push('=');
        }

        index += 3;
    }

    output
}

#[derive(Debug)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
}

impl ParsedUrl {
    fn parse(url: &str) -> Result<Self> {
        let (scheme, remainder) = url.split_once("://").ok_or_else(|| {
            DigestError::RequestFailed(format!("invalid URL `{url}`: missing scheme"))
        })?;

        let (authority, path) = match remainder.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (remainder, "/".to_string()),
        };

        let (host, port) = parse_authority(authority).ok_or_else(|| {
            DigestError::RequestFailed(format!("invalid URL `{url}`: malformed authority"))
        })?;

        Ok(Self {
            scheme: scheme.to_string(),
            host,
            port,
            path,
        })
    }

    fn host_header(&self) -> String {
        match self.port {
            Some(port) if !is_default_port(&self.scheme, port) => {
                format!("{}:{port}", self.host)
            }
            _ => self.host.clone(),
        }
    }
}

fn parse_authority(authority: &str) -> Option<(String, Option<u16>)> {
    if authority.starts_with('[') {
        let closing = authority.find(']')?;
        let host = authority[..=closing].to_string();
        let remainder = &authority[closing + 1..];
        if remainder.is_empty() {
            return Some((host, None));
        }

        let port = remainder.strip_prefix(':')?.parse::<u16>().ok()?;
        return Some((host, Some(port)));
    }

    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
            Some((host.to_string(), port.parse::<u16>().ok()))
        }
        _ if !authority.is_empty() => Some((authority.to_string(), None)),
        _ => None,
    }
}

fn is_default_port(scheme: &str, port: u16) -> bool {
    (scheme == "http" && port == 80) || (scheme == "https" && port == 443)
}

#[derive(Debug)]
struct HttpResponse {
    status_line: String,
    status_code: u16,
    headers: std::collections::HashMap<String, String>,
}

#[cfg(test)]
trait TokenSource {
    fn get_token(&self, image_ref: &str, registry_auth: &str) -> Result<String>;
}

#[cfg(test)]
fn compare_digest_with_url<D, T>(
    repo_digests: &[D],
    digest_url: &str,
    registry_auth: &str,
    token_source: &T,
) -> Result<bool>
where
    D: AsRef<str>,
    T: TokenSource,
{
    compare_digest_impl(
        digest_url,
        repo_digests,
        digest_url,
        registry_auth,
        |image_ref, auth| token_source.get_token(image_ref, auth),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn transform_auth_rewrites_base64_json_credentials() {
        let auth = encode_base64_standard(br#"{"username":"alice","password":"secret"}"#);

        let transformed = transform_auth(&auth);

        assert_eq!(transformed, encode_base64_standard(b"alice:secret"));
    }

    #[test]
    fn transform_auth_leaves_basic_auth_payloads_unchanged() {
        let auth = encode_base64_standard(b"alice:secret");

        let transformed = transform_auth(&auth);

        assert_eq!(transformed, auth);
    }

    #[test]
    fn transform_auth_rejects_invalid_base64() {
        let transformed = transform_auth("not-base64");

        assert_eq!(transformed, "not-base64");
    }

    #[test]
    fn get_digest_uses_head_and_returns_content_digest() {
        let server = spawn_test_server(
            |request| {
                assert!(
                    request.starts_with("HEAD /v2/library/watchtower/manifests/latest HTTP/1.1")
                );
                assert!(request.contains(&format!("User-Agent: {}", meta::user_agent())));
                assert!(request.contains("Authorization: Bearer token"));
                assert!(
                    request
                        .contains("Accept: application/vnd.docker.distribution.manifest.v2+json")
                );
            },
            |response| {
                response.push_str("HTTP/1.1 200 OK\r\n");
                response.push_str("Docker-Content-Digest: sha256:deadbeef\r\n");
                response.push_str("Content-Length: 0\r\n\r\n");
            },
        );

        let digest = get_digest(
            &format!(
                "http://{}/v2/library/watchtower/manifests/latest",
                server.addr
            ),
            "Bearer token",
        )
        .expect("digest should be returned");

        assert_eq!(digest, "sha256:deadbeef");
    }

    #[test]
    fn get_digest_reports_registry_error_details() {
        let server = spawn_test_server(
            |request| {
                assert!(
                    request.starts_with("HEAD /v2/library/watchtower/manifests/latest HTTP/1.1")
                );
            },
            |response| {
                response.push_str("HTTP/1.1 401 Unauthorized\r\n");
                response.push_str("Www-Authenticate: Bearer realm=\"https://example.invalid\"\r\n");
                response.push_str("Content-Length: 0\r\n\r\n");
            },
        );

        let err = get_digest(
            &format!(
                "http://{}/v2/library/watchtower/manifests/latest",
                server.addr
            ),
            "Bearer token",
        )
        .expect_err("should surface the status code");

        assert_eq!(
            err,
            DigestError::RegistryHeadRejected {
                status: "HTTP/1.1 401 Unauthorized".to_string(),
                auth: "Bearer realm=\"https://example.invalid\"".to_string(),
            }
        );
    }

    #[test]
    fn compare_digest_matches_remote_digest() {
        let registry_auth = encode_base64_standard(br#"{"username":"alice","password":"secret"}"#);
        let expected_basic = encode_base64_standard(b"alice:secret");
        let request_basic = expected_basic.clone();
        let server = spawn_test_server(
            move |request| {
                assert!(request.contains(&format!("Authorization: Basic {request_basic}")));
            },
            |response| {
                response.push_str("HTTP/1.1 200 OK\r\n");
                response.push_str("Docker-Content-Digest: sha256:abc123\r\n");
                response.push_str("Content-Length: 0\r\n\r\n");
            },
        );

        struct MockTokenSource {
            expected_basic: String,
        }

        impl TokenSource for MockTokenSource {
            fn get_token(&self, _image_ref: &str, registry_auth: &str) -> Result<String> {
                assert_eq!(registry_auth, self.expected_basic);
                Ok(format!("Basic {registry_auth}"))
            }
        }

        let repo_digests = vec![
            "library/watchtower@sha256:000000".to_string(),
            "library/watchtower@sha256:abc123".to_string(),
        ];

        let matches = compare_digest_with_url(
            &repo_digests,
            &format!(
                "http://{}/v2/library/watchtower/manifests/latest",
                server.addr
            ),
            &registry_auth,
            &MockTokenSource { expected_basic },
        )
        .expect("comparison should succeed");

        assert!(matches);
    }

    #[test]
    fn compare_digest_returns_false_when_repo_digests_are_empty() {
        let registry_auth = encode_base64_standard(br#"{"username":"alice","password":"secret"}"#);
        let expected_basic = encode_base64_standard(b"alice:secret");
        let request_basic = expected_basic.clone();
        let server = spawn_test_server(
            move |request| {
                assert!(request.contains(&format!("Authorization: Basic {request_basic}")));
            },
            |response| {
                response.push_str("HTTP/1.1 200 OK\r\n");
                response.push_str("Docker-Content-Digest: sha256:abc123\r\n");
                response.push_str("Content-Length: 0\r\n\r\n");
            },
        );

        struct MockTokenSource {
            expected_basic: String,
        }

        impl TokenSource for MockTokenSource {
            fn get_token(&self, _image_ref: &str, registry_auth: &str) -> Result<String> {
                assert_eq!(registry_auth, self.expected_basic);
                Ok(format!("Basic {registry_auth}"))
            }
        }

        let repo_digests: Vec<String> = Vec::new();

        let matches = compare_digest_with_url(
            &repo_digests,
            &format!(
                "http://{}/v2/library/watchtower/manifests/latest",
                server.addr
            ),
            &registry_auth,
            &MockTokenSource { expected_basic },
        )
        .expect("comparison should succeed");

        assert!(!matches);
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
            stream
                .write_all(response.as_bytes())
                .expect("write response");
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
