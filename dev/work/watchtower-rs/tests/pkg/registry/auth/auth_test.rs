#![forbid(unsafe_code)]

use std::env;

use url::Url;
use watchtower_rs::registry::auth::{self, AuthError};

const GHCR_USERNAME_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_GH_USERNAME";
const GHCR_PASSWORD_ENV: &str = "CI_INTEGRATION_TEST_REGISTRY_GH_PASSWORD";

#[test]
fn get_token_parses_a_registry_token_when_credentials_are_available() {
    // Keep the legacy gate: this is an integration-style check and is skipped
    // when the registry credentials are not present in the environment.
    let Some(credentials) = credentials_from_env(GHCR_USERNAME_ENV, GHCR_PASSWORD_ENV) else {
        eprintln!("Username missing. Skipping integration test");
        return;
    };

    let token = auth::get_token("ghcr.io/k6io/operator", &credentials)
        .expect("registry token should resolve");

    assert!(!token.is_empty());
}

#[test]
fn get_auth_url_creates_expected_url_for_the_challenge_header() {
    let challenge = r#"bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:user/image:pull""#;
    let url = auth::get_auth_url(challenge, "marrrrrrrrry/watchtower")
        .expect("auth url should resolve");

    assert_eq!(
        url,
        Url::parse(
            "https://ghcr.io/token?scope=repository%3Amarrrrrrrrry%2Fwatchtower%3Apull&service=ghcr.io",
        )
        .expect("expected url"),
    );
}

#[test]
fn get_auth_url_returns_an_error_for_an_invalid_challenge_header() {
    let challenge = r#"bearer realm="https://ghcr.io/token""#;

    let err = auth::get_auth_url(challenge, "marrrrrrrrry/watchtower")
        .expect_err("invalid challenge should fail");

    assert_eq!(err, AuthError::InvalidChallengeHeader);
}

#[test]
fn get_auth_url_prepends_library_for_docker_hub_images() {
    assert_eq!(
        scope_from_image_auth_url("registry"),
        "library/registry",
    );
    assert_eq!(
        scope_from_image_auth_url("docker.io/registry"),
        "library/registry",
    );
    assert_eq!(
        scope_from_image_auth_url("index.docker.io/registry"),
        "library/registry",
    );
}

#[test]
fn get_auth_url_keeps_vanity_hosts_out_of_the_scope() {
    assert_eq!(
        scope_from_image_auth_url("docker.io/marrrrrrrrry/watchtower"),
        "marrrrrrrrry/watchtower",
    );
    assert_eq!(
        scope_from_image_auth_url("index.docker.io/marrrrrrrrry/watchtower"),
        "marrrrrrrrry/watchtower",
    );
}

#[test]
fn get_auth_url_does_not_destroy_three_segment_image_names() {
    assert_eq!(
        scope_from_image_auth_url("piksel/marrrrrrrrry/watchtower"),
        "piksel/marrrrrrrrry/watchtower",
    );
    assert_eq!(
        scope_from_image_auth_url("ghcr.io/piksel/marrrrrrrrry/watchtower"),
        "piksel/marrrrrrrrry/watchtower",
    );
}

#[test]
fn get_auth_url_does_not_prepend_library_for_non_docker_hub_images() {
    assert_eq!(scope_from_image_auth_url("ghcr.io/watchtower"), "watchtower");
    assert_eq!(
        scope_from_image_auth_url("ghcr.io/marrrrrrrrry/watchtower"),
        "marrrrrrrrry/watchtower",
    );
}

#[test]
fn get_auth_url_ignores_empty_fields_without_crashing() {
    let input = r#"bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:user/image:pull","#;
    let url = auth::get_auth_url(input, "marrrrrrrrry/watchtower")
        .expect("empty fields should be ignored");

    assert!(!url.as_str().is_empty());
}

#[test]
fn get_auth_url_ignores_valueless_fields_without_crashing() {
    let input = r#"bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:user/image:pull",valuelesskey"#;
    let url = auth::get_auth_url(input, "marrrrrrrrry/watchtower")
        .expect("valueless fields should be ignored");

    assert!(!url.as_str().is_empty());
}

#[test]
fn get_challenge_url_creates_expected_registry_urls() {
    assert_eq!(
        auth::get_challenge_url("ghcr.io/marrrrrrrrry/watchtower:latest")
            .expect("challenge url should resolve"),
        Url::parse("https://ghcr.io/v2/").expect("expected url"),
    );
    assert_eq!(
        auth::get_challenge_url("marrrrrrrrry/watchtower:latest")
            .expect("challenge url should resolve"),
        Url::parse("https://index.docker.io/v2/").expect("expected url"),
    );
    assert_eq!(
        auth::get_challenge_url("docker.io/marrrrrrrrry/watchtower:latest")
            .expect("challenge url should resolve"),
        Url::parse("https://index.docker.io/v2/").expect("expected url"),
    );
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

fn scope_from_image_auth_url(image_name: &str) -> String {
    let challenge = r#"bearer realm="https://dummy.host/token",service="dummy.host",scope="repository:user/image:pull""#;
    let url = auth::get_auth_url(challenge, image_name).expect("auth url should resolve");
    let scope = url.query_pairs().find_map(|(key, value)| {
        if key == "scope" {
            Some(value.into_owned())
        } else {
            None
        }
    });

    let scope = scope.expect("scope should be present");
    assert!(
        scope.starts_with("repository:"),
        "scope should keep the repository prefix"
    );
    assert!(
        scope.ends_with(":pull"),
        "scope should keep the pull suffix"
    );

    scope
        .strip_prefix("repository:")
        .and_then(|value| value.strip_suffix(":pull"))
        .expect("scope should strip cleanly")
        .to_string()
}
