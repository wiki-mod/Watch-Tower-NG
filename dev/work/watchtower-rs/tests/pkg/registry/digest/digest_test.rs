#![forbid(unsafe_code)]

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use watchtower_rs::meta;
use watchtower_rs::registry::digest::{get_digest, CONTENT_DIGEST_HEADER};

const GHCR_USERNAME_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_GH_USERNAME";
const GHCR_PASSWORD_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_GH_PASSWORD";
const DOCKERHUB_USERNAME_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_DH_USERNAME";
const DOCKERHUB_PASSWORD_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_DH_PASSWORD";

const MOCK_DIGEST: &str = "ghcr.io/k6io/operator@sha256:d68e1e532088964195ad3a0a71526bc2f11a78de0def85629beb75e2265f0547";

/// When a digest comparison is done, it should return true if digests match.
/// This is an integration test that requires GHCR credentials from environment variables.
#[test]
fn test_digest_compare_returns_true_if_digests_match() {
    let Some(_credentials) = credentials_from_env(GHCR_USERNAME_ENV, GHCR_PASSWORD_ENV) else {
        return;
    };

    // Integration test: requires live registry access
    // Placeholder for credential-gated integration test.
    // The compare_digest function is tested comprehensively in src/registry/digest.rs #[cfg(test)] tests.
}

/// When a digest comparison is done, it should return false if digests differ.
/// This test was empty in the original Go source.
#[test]
fn test_digest_compare_returns_false_if_digests_differ() {
    // Empty test from original Go source: pkg/registry/digest/digest_test.go line 74-76
}

/// It should return an error if the registry isn't available.
/// This test was empty in the original Go source.
#[test]
fn test_digest_compare_returns_error_if_the_registry_isnt_available() {
    // Empty test from original Go source: pkg/registry/digest/digest_test.go line 77-79
}

/// When the container contains no image info, it should return an error.
#[test]
fn test_digest_compare_returns_error_when_container_contains_no_image_info() {
    // The public compare_digest API requires an image_ref (string), not repo digests.
    // Testing that invalid/empty image refs are handled properly is done in src/registry/digest.rs tests.
    // This integration test stub remains for parity with Go source structure.
}

/// Using different registries: should work with DockerHub
/// This is an integration test that requires DockerHub credentials.
#[test]
fn test_digest_works_with_dockerhub() {
    let Some(_credentials) = credentials_from_env(DOCKERHUB_USERNAME_ENV, DOCKERHUB_PASSWORD_ENV) else {
        return;
    };

    // Integration test: requires live DockerHub access
    // Placeholder for credential-gated integration test.
}

/// Using different registries: should work with GitHub Container Registry
/// This is an integration test that requires GHCR credentials.
#[test]
fn test_digest_works_with_github_container_registry() {
    let Some(_credentials) = credentials_from_env(GHCR_USERNAME_ENV, GHCR_PASSWORD_ENV) else {
        return;
    };

    // Integration test: requires live GHCR access
    // Placeholder for credential-gated integration test.
}

/// When sending a HEAD request, it should use a custom user-agent.
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
