#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error};

use super::helpers;

/// Result type used by the registry trust helpers.
pub type Result<T> = std::result::Result<T, TrustError>;

/// Errors raised while processing registry authentication.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustError {
    /// The image reference could not be resolved to a registry.
    #[error(transparent)]
    Registry(#[from] helpers::RegistryError),
    /// No usable `REPO_USER`/`REPO_PASS` pair was present in the environment.
    #[error("registry auth environment variables (REPO_USER, REPO_PASS) not set")]
    MissingEnvironmentCredentials,
    /// The Docker config file could not be read.
    #[error("unable to find default config file: {0}")]
    ConfigRead(String),
    /// The Docker config file is not valid JSON.
    #[error("config parse error: {0}")]
    ConfigParse(String),
    /// An `auth` entry could not be decoded.
    #[error("auth decode error: {0}")]
    AuthDecode(String),
}

/// Auth configuration matching the Docker API types.
/// Field order is significant for JSON serialization to match Go's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthConfig {
    username: String,
    password: String,
}

/// Return whether the given image reference is hosted on a registry that
/// rate-limits API usage (Docker Hub or GHCR).
///
/// Returns `Ok(true)` for Docker Hub (`index.docker.io`, `registry-1.docker.io`)
/// and GitHub Container Registry (`ghcr.io`). Returns `Ok(false)` for all other
/// registries. Returns `Err` if the registry address cannot be resolved from the
/// image name, and callers should `.unwrap_or(true)` for fail-closed behavior.
///
/// Mirrors Go's `WarnOnAPIConsumption` helper in `pkg/registry/registry.go`.
pub fn warn_on_api_consumption(image_name: &str) -> Result<bool> {
    let registry = helpers::get_registry_address(image_name)?;
    Ok(matches!(
        registry.as_str(),
        "index.docker.io" | "registry-1.docker.io" | "ghcr.io"
    ))
}

/// Returns an encoded auth config for the given registry
/// loaded from environment variables or docker config
/// as available in that order.
pub fn encoded_auth(ref_: &str) -> Result<String> {
    match encoded_env_auth() {
        Ok(auth) => Ok(auth),
        Err(_) => encoded_config_auth(ref_),
    }
}

/// Returns an encoded auth config for the given registry
/// loaded from environment variables.
/// Returns an error if authentication environment variables have not been set.
pub fn encoded_env_auth() -> Result<String> {
    let username = env::var("REPO_USER").unwrap_or_default();
    let password = env::var("REPO_PASS").unwrap_or_default();

    if !username.is_empty() && !password.is_empty() {
        debug!(username = %username, "Loaded auth credentials for registry user from environment");
        // CREDENTIAL: Uncomment to log REPO_PASS environment variable
        // debug!(password = %password, "Using auth password");

        let auth = AuthConfig { username, password };
        return encode_auth(auth);
    }

    Err(TrustError::MissingEnvironmentCredentials)
}

/// Returns an encoded auth config for the given registry
/// loaded from the docker config.
/// Returns an empty string if credentials cannot be found for the referenced server.
/// The docker config must be mounted on the container.
pub fn encoded_config_auth(image_ref: &str) -> Result<String> {
    let server = helpers::get_registry_address(image_ref).map_err(|e| {
        error!(image_ref = %image_ref, "Could not get registry from image ref");
        TrustError::Registry(e)
    })?;

    let config_dir = env::var("DOCKER_CONFIG").unwrap_or_else(|_| "/".to_string());
    let config_path = resolve_config_path(&config_dir);

    let config_file = load_config_file(&config_path).map_err(|e| {
        error!(error = %e, "Unable to find default config file");
        TrustError::ConfigRead(e)
    })?;

    let credentials_store = CredentialsStore::from_config(&config_file)?;
    let auth = credentials_store.get(&server);

    if auth.username.is_empty() && auth.password.is_empty() {
        debug!(config_file = ?config_path, server = %server, "No credentials found");
        return Ok(String::new());
    }

    debug!(
        username = %auth.username,
        server = %server,
        config_file = ?config_path,
        "Loaded auth credentials for user on registry from file"
    );
    // CREDENTIAL: Uncomment to log docker config password
    // debug!(password = %auth.password, "Using auth password");

    encode_auth(auth)
}

/// Resolves the config path: mirrors Go's cliconfig.Load behavior which always
/// treats the directory as a directory and appends config.json.
/// Only exception: when DOCKER_CONFIG is not set, defaults to /config.json.
fn resolve_config_path(docker_config: &str) -> PathBuf {
    if docker_config.is_empty() || docker_config == "/" {
        return PathBuf::from("/config.json");
    }

    // Go's cliconfig.Load always appends config.json to directories
    let path = PathBuf::from(docker_config);
    path.join("config.json")
}

/// Returns a credentials store based on the settings in the Docker config file.
/// This mirrors the Go `CredentialsStore` function behavior.
struct CredentialsStore {
    // The Go version supports both native credential stores (via credsStore)
    // and file-based stores. This Rust port only supports inline "auths" entries.
    // Supporting native stores would require additional dependencies not yet in Cargo.toml.
    auths: BTreeMap<String, serde_json::Value>,
    _creds_store: Option<String>,
}

impl CredentialsStore {
    fn from_config(config: &serde_json::Value) -> Result<Self> {
        let auths = config
            .get("auths")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();

        let creds_store = config
            .get("credsStore")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(CredentialsStore {
            auths,
            _creds_store: creds_store,
        })
    }

    fn get(&self, server: &str) -> AuthConfig {
        // Try to find credentials for the given server
        if let Some(entry) = self.auths.get(server) {
            if let Some(auth_str) = entry.get("auth").and_then(|v| v.as_str()) {
                if !auth_str.is_empty() {
                    if let Ok(decoded) = decode_base64_auth(auth_str) {
                        return decoded;
                    }
                }
            }
            // Also check for explicit username/password fields
            let username = entry
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let password = entry
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !username.is_empty() && !password.is_empty() {
                return AuthConfig { username, password };
            }
        }

        // Return empty auth if not found
        AuthConfig {
            username: String::new(),
            password: String::new(),
        }
    }
}

/// Base64 encode an AuthConfig struct for transmission over HTTP.
/// Mirrors the Go EncodeAuth function.
fn encode_auth(auth: AuthConfig) -> Result<String> {
    let json_bytes = serde_json::to_vec(&auth)
        .map_err(|e| TrustError::ConfigParse(e.to_string()))?;

    Ok(encode_base64_urlsafe(&json_bytes))
}

/// Load and parse a Docker config file.
fn load_config_file(config_path: &Path) -> std::result::Result<serde_json::Value, String> {
    let contents = fs::read_to_string(config_path)
        .map_err(|e| format!("{}: {}", config_path.display(), e))?;

    serde_json::from_str(&contents)
        .map_err(|e| format!("failed to parse config JSON: {}", e))
}

/// Decode a base64-encoded auth string in the format "username:password".
fn decode_base64_auth(encoded: &str) -> std::result::Result<AuthConfig, TrustError> {
    let decoded = decode_base64(encoded)
        .map_err(|e| TrustError::AuthDecode(format!("base64 decode: {}", e)))?;

    let auth_str = String::from_utf8(decoded)
        .map_err(|e| TrustError::AuthDecode(format!("utf8: {}", e)))?;

    let (username, password) = auth_str
        .split_once(':')
        .ok_or_else(|| {
            TrustError::AuthDecode("invalid auth format (expected username:password)".to_string())
        })?;

    Ok(AuthConfig {
        username: username.to_string(),
        password: password.to_string(),
    })
}

/// Decode base64 URL-safe encoded data.
fn decode_base64(input: &str) -> std::result::Result<Vec<u8>, String> {
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
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                padding += 1;
                0
            }
            _ => return Err(format!("invalid base64 character: {}", byte as char)),
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
        return Err("incomplete base64 chunk".to_string());
    }

    Ok(output)
}

/// Decode a single 4-byte base64 chunk.
fn decode_base64_chunk(chunk: &[u8; 4], padding: usize, output: &mut Vec<u8>) -> std::result::Result<(), String> {
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
        _ => return Err(format!("invalid padding: {}", padding)),
    }

    Ok(())
}

/// Encode to base64 URL-safe format (- and _ instead of + and /).
fn encode_base64_urlsafe(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut output = String::with_capacity(input.len().saturating_mul(4) / 3 + 4);
    let mut index = 0usize;

    while index < input.len() {
        let remaining = input.len() - index;
        let chunk = &input[index..std::cmp::min(index + 3, input.len())];

        let first = chunk[0] >> 2;
        let second = ((chunk[0] & 0b0000_0011) << 4) | (chunk.get(1).copied().unwrap_or(0) >> 4);
        let third =
            ((chunk.get(1).copied().unwrap_or(0) & 0b0000_1111) << 2)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_auth_matches_expected_output() {
        // Test against a known vector from credentials.rs tests (line 330-331)
        let auth = AuthConfig {
            username: "containrrr-user".to_string(),
            password: "containrrr-pass".to_string(),
        };

        let encoded = encode_auth(auth).expect("should encode");
        assert_eq!(
            encoded,
            "eyJ1c2VybmFtZSI6ImNvbnRhaW5ycnItdXNlciIsInBhc3N3b3JkIjoiY29udGFpbnJyci1wYXNzIn0="
        );
    }

    #[test]
    fn credentials_store_returns_empty_for_missing_server() {
        let config = serde_json::json!({
            "auths": {}
        });

        let store = CredentialsStore::from_config(&config).expect("should create store");
        let auth = store.get("registry.example.com");

        assert_eq!(auth.username, "");
        assert_eq!(auth.password, "");
    }

    #[test]
    fn resolve_config_path_appends_config_json_to_directories() {
        assert_eq!(
            resolve_config_path("/etc/docker"),
            PathBuf::from("/etc/docker/config.json")
        );
    }

    #[test]
    fn resolve_config_path_appends_config_json_even_for_direct_files() {
        // Go's cliconfig.Load always treats as directory and appends config.json
        assert_eq!(
            resolve_config_path("/etc/docker/config.json"),
            PathBuf::from("/etc/docker/config.json/config.json")
        );
    }

    #[test]
    fn resolve_config_path_defaults_to_root_config() {
        assert_eq!(
            resolve_config_path(""),
            PathBuf::from("/config.json")
        );
    }

    #[test]
    fn base64_round_trip() {
        let original = b"hello world";
        let encoded = encode_base64_urlsafe(original);
        let decoded = decode_base64(&encoded).expect("should decode");
        assert_eq!(decoded, original);
    }
}
