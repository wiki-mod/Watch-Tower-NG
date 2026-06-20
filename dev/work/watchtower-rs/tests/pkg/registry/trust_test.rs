use std::env;

use watchtower_rs::registry::credentials;

#[test]
fn encoded_auth_returns_repo_credentials_from_env_when_set() {
    let old_user = env::var_os("REPO_USER");
    let old_pass = env::var_os("REPO_PASS");

    // The test needs process-local environment mutation, which is unsafe in
    // Rust 2024 because it can race with other threads.
    unsafe {
        env::set_var("REPO_USER", "containrrr-user");
        env::set_var("REPO_PASS", "containrrr-pass");
    }

    let result = credentials::encoded_env_auth();

    if let Some(value) = old_user {
        unsafe {
            env::set_var("REPO_USER", value);
        }
    } else {
        unsafe {
            env::remove_var("REPO_USER");
        }
    }
    if let Some(value) = old_pass {
        unsafe {
            env::set_var("REPO_PASS", value);
        }
    } else {
        unsafe {
            env::remove_var("REPO_PASS");
        }
    }

    assert_eq!(
        result.expect("environment credentials should encode"),
        "eyJ1c2VybmFtZSI6ImNvbnRhaW5ycnItdXNlciIsInBhc3N3b3JkIjoiY29udGFpbnJyci1wYXNzIn0="
    );
}

#[test]
fn encoded_env_auth_returns_an_error_if_repo_envs_are_unset() {
    let old_user = env::var_os("REPO_USER");
    let old_pass = env::var_os("REPO_PASS");

    unsafe {
        env::remove_var("REPO_USER");
        env::remove_var("REPO_PASS");
    }

    let result = credentials::encoded_env_auth();

    if let Some(value) = old_user {
        unsafe {
            env::set_var("REPO_USER", value);
        }
    }
    if let Some(value) = old_pass {
        unsafe {
            env::set_var("REPO_PASS", value);
        }
    }

    assert!(result.is_err());
}

#[test]
fn encoded_config_auth_returns_an_error_if_file_is_not_present() {
    let old_docker_config = env::var_os("DOCKER_CONFIG");
    unsafe {
        env::set_var("DOCKER_CONFIG", "/dev/null/should-fail");
    }

    let result = credentials::encoded_config_auth("");

    if let Some(value) = old_docker_config {
        unsafe {
            env::set_var("DOCKER_CONFIG", value);
        }
    } else {
        unsafe {
            env::remove_var("DOCKER_CONFIG");
        }
    }

    assert!(result.is_err());
}
