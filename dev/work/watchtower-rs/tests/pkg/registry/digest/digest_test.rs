#![forbid(unsafe_code)]

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use watchtower_rs::meta;
use watchtower_rs::registry::digest::{
    compare_digest_with_url, get_digest, DigestError, TokenSource, CONTENT_DIGEST_HEADER,
};

const GHCR_USERNAME_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_GH_USERNAME";
const GHCR_PASSWORD_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_GH_PASSWORD";
const DOCKERHUB_USERNAME_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_DH_USERNAME";
const DOCKERHUB_PASSWORD_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_DH_PASSWORD";

const MOCK_DIGEST: &str = "ghcr.io/k6io/operator@sha256:d68e1e532088964195ad3a0a71526bc2f11a78de0def85629beb75e2265f0547";

struct StaticTokenSource {
    expected_registry_auth: String,
    token: String,
}

impl TokenSource for StaticTokenSource {
    fn get_token(&self, _image_ref: &str, registry_auth: &str) -> watchtower_rs::registry::digest::Result<String> {
        assert_eq!(registry_auth, self.expected_registry_auth);
        Ok(self.token.clone())
    }
}

#[test]
fn test_digest_compare_returns_true_if_digests_match() {
    let Some(credentials) = credentials_from_env(GHCR_USERNAME_ENV, GHCR_PASSWORD_ENV) else {
        return;
    };
    let expected_registry_auth = credentials.clone();

    let server = spawn_test_server(
        move |request| {
            assert!(request.starts_with("HEAD /v2/library/watchtower/manifests/latest HTTP/1.1"));
            assert!(request.contains("Authorization: Bearer token"));
        },
        |response| {
            response.push_str("HTTP/1.1 200 OK\r\n");
            response.push_str("Docker-Content-Digest: sha256:d68e1e532088964195ad3a0a71526bc2f11a78de0def85629beb75e2265f0547\r\n");
            response.push_str("Content-Length: 0\r\n\r\n");
        },
    );

    let repo_digests = vec![
        "ghcr.io/k6io/operator@sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        MOCK_DIGEST.to_string(),
    ];

    let matches = compare_digest_with_url(
        &repo_digests,
        &format!("http://{}/v2/library/watchtower/manifests/latest", server.addr),
        &credentials,
        &StaticTokenSource {
            expected_registry_auth,
            token: "Bearer token".to_string(),
        },
    )
    .expect("comparison should succeed");

    assert!(matches);
}

#[test]
fn test_digest_compare_returns_false_if_digests_differ() {}

#[test]
fn test_digest_compare_returns_error_if_the_registry_isnt_available() {}

#[test]
fn test_digest_compare_returns_error_when_container_contains_no_image_info() {
    let err = compare_digest_with_url(
        &[] as &[&str],
        "http://127.0.0.1:1/v2/library/watchtower/manifests/latest",
        "user:pass",
        &StaticTokenSource {
            expected_registry_auth: "user:pass".to_string(),
            token: "Bearer token".to_string(),
        },
    )
    .expect_err("missing image info should fail");

    assert_eq!(err, DigestError::MissingImageInfo);
}

#[test]
fn test_digest_works_with_dockerhub() {
    let Some(_credentials) = credentials_from_env(DOCKERHUB_USERNAME_ENV, DOCKERHUB_PASSWORD_ENV) else {
        return;
    };
}

#[test]
fn test_digest_works_with_github_container_registry() {
    let Some(_credentials) = credentials_from_env(GHCR_USERNAME_ENV, GHCR_PASSWORD_ENV) else {
        return;
    };
}

#[test]
fn test_digest_uses_a_custom_user_agent() {
    let server = spawn_test_server(
        |request| {
            assert!(request.contains(&format!("User-Agent: {}", meta::user_agent())));
        },
        |response| {
            response.push_str("HTTP/1.1 200 OK\r\n");
            response.push_str(&format!("{CONTENT_DIGEST_HEADER}: {MOCK_DIGEST}\r\n"));
            response.push_str("Content-Length: 0\r\n\r\n");
        },
    );

    let digest = get_digest(&format!("http://{}/v2/library/watchtower/manifests/latest", server.addr), "token")
        .expect("digest should be returned");

    assert_eq!(digest, MOCK_DIGEST);
}

fn credentials_from_env(username_key: &str, password_key: &str) -> Option<String> {
    let username = env::var(username_key).ok()?.trim().to_string();
    if username.is_empty() {
        return None;
    }

    let password = env::var(password_key).ok()?.trim().to_string();
    if password.is_empty() {
        return None;
    }

    Some(format!("{username}:{password}"))
}

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

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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
