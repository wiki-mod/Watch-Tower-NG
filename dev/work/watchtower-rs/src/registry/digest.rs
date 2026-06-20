#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use thiserror::Error;

use super::manifest;

/// Docker registry response header containing the image digest.
pub const CONTENT_DIGEST_HEADER: &str = "Docker-Content-Digest";

/// Historical Watchtower user agent used for registry requests.
pub const USER_AGENT: &str = "Watchtower/v0.0.0-unknown";

/// Result type used by the digest helpers.
pub type Result<T> = std::result::Result<T, DigestError>;

/// Errors raised by the digest helpers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DigestError {
    #[error("registry auth payload could not be base64-decoded")]
    InvalidRegistryAuthEncoding,
    #[error("container image info missing")]
    MissingImageInfo,
    #[error("could not fetch token")]
    MissingToken,
    #[error("invalid image reference `{image_ref}`: {reason}")]
    InvalidImageReference { image_ref: String, reason: String },
    #[error("registry responded to head request with {status:?}, auth: {auth}")]
    RegistryHeadRejected { status: String, auth: String },
    #[error("registry response did not include `{0}`")]
    MissingDigestHeader(String),
    #[error("registry request failed: {0}")]
    RequestFailed(String),
    #[error("invalid digest entry `{digest}`")]
    InvalidLocalDigest { digest: String },
}

/// Minimal token source surface used by `compare_digest`.
pub trait TokenSource {
    fn get_token(&self, image_ref: &str, registry_auth: &str) -> Result<String>;
}

/// Transform a base64-encoded JSON auth blob into a base64 `username:password`
/// payload. Inputs that are already a basic-auth payload are returned unchanged.
pub fn transform_auth(registry_auth: &str) -> Result<String> {
    if registry_auth.is_empty() {
        return Ok(String::new());
    }

    let decoded = match decode_base64_standard(registry_auth) {
        Ok(decoded) => decoded,
        Err(_) => return Ok(registry_auth.to_string()),
    };

    let Ok(decoded) = String::from_utf8(decoded) else {
        return Ok(registry_auth.to_string());
    };

    let Some(username) = extract_json_string_field(&decoded, "username") else {
        return Ok(registry_auth.to_string());
    };
    let Some(password) = extract_json_string_field(&decoded, "password") else {
        return Ok(registry_auth.to_string());
    };

    if username.is_empty() || password.is_empty() {
        return Ok(registry_auth.to_string());
    }

    Ok(encode_base64_standard(format!("{username}:{password}").as_bytes()))
}

/// Compare a registry digest against local repository digests.
///
/// The caller passes the image reference and repo digests directly so this
/// module stays independent from the yet-to-land auth/container wiring.
pub fn compare_digest<D, T>(
    image_ref: &str,
    repo_digests: &[D],
    registry_auth: &str,
    token_source: &T,
) -> Result<bool>
where
    D: AsRef<str>,
    T: TokenSource,
{
    let digest_url = manifest::build_manifest_url(image_ref).map_err(|err| {
        DigestError::InvalidImageReference {
            image_ref: image_ref.to_string(),
            reason: err.to_string(),
        }
    })?;

    compare_digest_with_url(repo_digests, &digest_url, registry_auth, token_source)
}

/// Compare a registry digest against local repository digests using an already
/// resolved manifest URL.
pub fn compare_digest_with_url<D, T>(
    repo_digests: &[D],
    digest_url: &str,
    registry_auth: &str,
    token_source: &T,
) -> Result<bool>
where
    D: AsRef<str>,
    T: TokenSource,
{
    if repo_digests.is_empty() {
        return Err(DigestError::MissingImageInfo);
    }

    let registry_auth = transform_auth(registry_auth)?;
    let token = token_source.get_token(digest_url, &registry_auth)?;
    if token.trim().is_empty() {
        return Err(DigestError::MissingToken);
    }

    let remote_digest = get_digest(&digest_url, &token)?;

    for digest in repo_digests {
        let digest = digest.as_ref();
        let Some((_, local_digest)) = digest.split_once('@') else {
            return Err(DigestError::InvalidLocalDigest {
                digest: digest.to_string(),
            });
        };

        if local_digest == remote_digest {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Fetch the remote digest via a `HEAD` request.
pub fn get_digest(url: &str, token: &str) -> Result<String> {
    if token.trim().is_empty() {
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

/// Evaluate a registry HEAD response without performing the transport call.
pub fn digest_from_response(status_line: &str, headers: HashMap<String, String>) -> Result<String> {
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

    response
        .headers
        .get(&CONTENT_DIGEST_HEADER.to_ascii_lowercase())
        .cloned()
        .ok_or_else(|| DigestError::MissingDigestHeader(CONTENT_DIGEST_HEADER.to_string()))
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
        user_agent = USER_AGENT,
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
            &format!("User-Agent: {USER_AGENT}"),
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
            _ => return Err(DigestError::InvalidRegistryAuthEncoding),
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
        return Err(DigestError::InvalidRegistryAuthEncoding);
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
        _ => return Err(DigestError::InvalidRegistryAuthEncoding),
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

fn extract_json_string_field(input: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = input.find(&needle)? + needle.len();
    let remainder = input[start..].trim_start();
    let remainder = remainder.strip_prefix(':')?.trim_start();
    let remainder = remainder.strip_prefix('"')?;

    let mut value = String::new();
    let mut chars = remainder.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(value),
            '\\' => {
                let escaped = chars.next()?;
                match escaped {
                    '"' | '\\' | '/' => value.push(escaped),
                    'b' => value.push('\u{0008}'),
                    'f' => value.push('\u{000c}'),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'u' => {
                        let mut codepoint = String::with_capacity(4);
                        for _ in 0..4 {
                            codepoint.push(chars.next()?);
                        }
                        let scalar = u16::from_str_radix(&codepoint, 16).ok()?;
                        let ch = char::from_u32(scalar as u32)?;
                        value.push(ch);
                    }
                    _ => return None,
                }
            }
            other => value.push(other),
        }
    }

    None
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
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn transform_auth_rewrites_base64_json_credentials() {
        let auth = encode_base64_standard(br#"{"username":"alice","password":"secret"}"#);

        let transformed = transform_auth(&auth).expect("transform should succeed");

        assert_eq!(transformed, encode_base64_standard(b"alice:secret"));
    }

    #[test]
    fn transform_auth_leaves_basic_auth_payloads_unchanged() {
        let auth = encode_base64_standard(b"alice:secret");

        let transformed = transform_auth(&auth).expect("transform should succeed");

        assert_eq!(transformed, auth);
    }

    #[test]
    fn transform_auth_rejects_invalid_base64() {
        let transformed = transform_auth("not-base64").expect("should preserve malformed payload");

        assert_eq!(transformed, "not-base64");
    }

    #[test]
    fn get_digest_uses_head_and_returns_content_digest() {
        let server = spawn_test_server(|request| {
            assert!(request.starts_with("HEAD /v2/library/watchtower/manifests/latest HTTP/1.1"));
            assert!(request.contains("User-Agent: Watchtower/v0.0.0-unknown"));
            assert!(request.contains("Authorization: Bearer token"));
            assert!(request.contains(
                "Accept: application/vnd.docker.distribution.manifest.v2+json"
            ));
        }, |response| {
            response.push_str("HTTP/1.1 200 OK\r\n");
            response.push_str("Docker-Content-Digest: sha256:deadbeef\r\n");
            response.push_str("Content-Length: 0\r\n\r\n");
        });

        let digest = get_digest(
            &format!("http://{}/v2/library/watchtower/manifests/latest", server.addr),
            "Bearer token",
        )
        .expect("digest should be returned");

        assert_eq!(digest, "sha256:deadbeef");
    }

    #[test]
    fn get_digest_reports_registry_error_details() {
        let server = spawn_test_server(|request| {
            assert!(request.starts_with("HEAD /v2/library/watchtower/manifests/latest HTTP/1.1"));
        }, |response| {
            response.push_str("HTTP/1.1 401 Unauthorized\r\n");
            response.push_str("Www-Authenticate: Bearer realm=\"https://example.invalid\"\r\n");
            response.push_str("Content-Length: 0\r\n\r\n");
        });

        let err = get_digest(
            &format!("http://{}/v2/library/watchtower/manifests/latest", server.addr),
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
        let server = spawn_test_server(move |request| {
            assert!(request.contains(&format!("Authorization: Basic {request_basic}")));
        }, |response| {
            response.push_str("HTTP/1.1 200 OK\r\n");
            response.push_str("Docker-Content-Digest: sha256:abc123\r\n");
            response.push_str("Content-Length: 0\r\n\r\n");
        });

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
            &format!("http://{}/v2/library/watchtower/manifests/latest", server.addr),
            &registry_auth,
            &MockTokenSource {
                expected_basic,
            },
        )
        .expect("comparison should succeed");

        assert!(matches);
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
