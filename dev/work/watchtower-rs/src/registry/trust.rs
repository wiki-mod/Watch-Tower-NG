#![forbid(unsafe_code)]

//! Registry credential helpers ported from `old-source/pkg/registry/trust.go`.

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

/// Errors raised while resolving registry credentials.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustError {
    /// The image reference could not be resolved to a registry.
    #[error(transparent)]
    Registry(#[from] helpers::RegistryError),
    /// No usable `REPO_USER`/`REPO_PASS` pair was present in the environment.
    #[error("registry auth environment variables (REPO_USER, REPO_PASS) not set")]
    MissingEnvironmentCredentials,
    /// The Docker config file could not be read.
    #[error("could not read Docker config file {path:?}: {message}")]
    ConfigRead { path: PathBuf, message: String },
    /// The Docker config file is not valid JSON.
    #[error("Docker config file is not valid JSON: {message}")]
    ConfigParse { message: String },
    /// An `auth` entry was present but could not be decoded into credentials.
    #[error("registry auth payload could not be decoded")]
    InvalidAuthEncoding,
}

/// Resolve credentials from the environment first and then fall back to the
/// Docker config file.
///
/// Mirrors Go's `EncodedAuth`.
pub fn encoded_auth(image_ref: &str) -> Result<String> {
    match encoded_env_auth() {
        Ok(auth) => Ok(auth),
        Err(_) => encoded_config_auth(image_ref),
    }
}

/// Encode registry credentials from `REPO_USER` and `REPO_PASS`.
///
/// Mirrors Go's `EncodedEnvAuth`.
pub fn encoded_env_auth() -> Result<String> {
    let username = env::var("REPO_USER").ok().filter(|v| !v.is_empty());
    let password = env::var("REPO_PASS").ok().filter(|v| !v.is_empty());

    encoded_env_auth_from_values(username.as_deref(), password.as_deref())
}

/// Encode registry credentials from a Docker `config.json`-style file.
///
/// Mirrors Go's `EncodedConfigAuth`.
pub fn encoded_config_auth(image_ref: &str) -> Result<String> {
    let server = helpers::get_registry_address(image_ref).map_err(|e| {
        error!(image_ref = %image_ref, "Could not get registry from image ref");
        TrustError::Registry(e)
    })?;

    let config_path = docker_config_path();
    debug!(config_file = ?config_path, server = %server, "Loading Docker config");

    encoded_config_auth_from_path(image_ref, &config_path)
}

/// Return whether the given registry rate-limits API usage.
///
/// Returns `true` for Docker Hub and GitHub Container Registry.
/// Callers should `.unwrap_or(true)` for fail-closed behavior.
///
/// Mirrors Go's `WarnOnAPIConsumption` in `pkg/registry/registry.go`.
pub fn warn_on_api_consumption(image_name: &str) -> Result<bool> {
    let registry = helpers::get_registry_address(image_name)?;
    Ok(matches!(
        registry.as_str(),
        "index.docker.io" | "registry-1.docker.io" | "ghcr.io"
    ))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn encoded_env_auth_from_values(username: Option<&str>, password: Option<&str>) -> Result<String> {
    let Some(username) = username.filter(|v| !v.is_empty()) else {
        return Err(TrustError::MissingEnvironmentCredentials);
    };
    let Some(password) = password.filter(|v| !v.is_empty()) else {
        return Err(TrustError::MissingEnvironmentCredentials);
    };

    debug!(username = %username, "Loaded auth credentials for registry user from environment");
    encode_auth_json(username, password)
}

fn encoded_config_auth_from_path(image_ref: &str, config_path: &Path) -> Result<String> {
    let registry = helpers::get_registry_address(image_ref)?;
    let config = DockerConfigFile::load(config_path)?;
    let auth = config
        .encoded_auth_for_registry(&registry)?
        .unwrap_or_default();

    if !auth.is_empty() {
        debug!(
            server = %registry,
            config_file = ?config_path,
            "Loaded auth credentials from Docker config file"
        );
    } else {
        debug!(
            config_file = ?config_path,
            server = %registry,
            "No credentials found in Docker config file"
        );
    }

    Ok(auth)
}

/// Resolve the Docker config file path.
///
/// Mirrors Go's `cliconfig.Load` behavior: `DOCKER_CONFIG` is always treated
/// as a directory and `config.json` is appended. When the env var is absent or
/// equals `"/"`, defaults to `/config.json`.
fn docker_config_path() -> PathBuf {
    let config_dir = env::var("DOCKER_CONFIG").unwrap_or_default();
    resolve_config_path(&config_dir)
}

fn resolve_config_path(docker_config: &str) -> PathBuf {
    if docker_config.is_empty() || docker_config == "/" {
        return PathBuf::from("/config.json");
    }

    PathBuf::from(docker_config).join("config.json")
}

fn encode_auth_json(username: &str, password: &str) -> Result<String> {
    let payload = AuthPayload { username, password };
    let serialized =
        serde_json::to_vec(&payload).map_err(|e| TrustError::ConfigParse { message: e.to_string() })?;
    Ok(encode_base64_urlsafe(&serialized))
}

fn decode_auth_json(auth: &str) -> Result<(String, String)> {
    let decoded = decode_base64(auth)?;
    let decoded = String::from_utf8(decoded).map_err(|_| TrustError::InvalidAuthEncoding)?;
    let Some((username, password)) = decoded.split_once(':') else {
        return Err(TrustError::InvalidAuthEncoding);
    };

    if username.is_empty() || password.is_empty() {
        return Err(TrustError::InvalidAuthEncoding);
    }

    Ok((username.to_string(), password.to_string()))
}

/// Parsed representation of a Docker config file.
///
/// Inline `auths` entries are supported. External credential helpers
/// (`credsStore`) are parsed but not resolved.
#[derive(Debug, Default, Deserialize)]
struct DockerConfigFile {
    #[serde(default)]
    auths: BTreeMap<String, DockerAuthEntry>,
    #[serde(default, rename = "credsStore")]
    _creds_store: Option<String>,
}

impl DockerConfigFile {
    fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|e| TrustError::ConfigRead {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        serde_json::from_str(&contents).map_err(|e| TrustError::ConfigParse {
            message: e.to_string(),
        })
    }

    fn encoded_auth_for_registry(&self, registry: &str) -> Result<Option<String>> {
        let registry_key = canonical_registry_key(registry);

        for (config_key, entry) in &self.auths {
            if canonical_registry_key(config_key) != registry_key {
                continue;
            }
            return entry.encoded_auth().map(Some);
        }

        Ok(None)
    }
}

/// Parsed Docker registry auth entry.
#[derive(Debug, Default, Deserialize)]
struct DockerAuthEntry {
    auth: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

impl DockerAuthEntry {
    fn encoded_auth(&self) -> Result<String> {
        if let Some(auth) = self.auth.as_deref().filter(|v| !v.is_empty()) {
            let (username, password) = decode_auth_json(auth)?;
            return encode_auth_json(&username, &password);
        }

        let Some(username) = self.username.as_deref().filter(|v| !v.is_empty()) else {
            return Ok(String::new());
        };
        let Some(password) = self.password.as_deref().filter(|v| !v.is_empty()) else {
            return Ok(String::new());
        };

        encode_auth_json(username, password)
    }
}

#[derive(Debug, Serialize)]
struct AuthPayload<'a> {
    username: &'a str,
    password: &'a str,
}

/// Normalize a registry key for comparison.
///
/// Strips scheme, trailing slashes, and the Docker Hub alias so that
/// `https://index.docker.io/v1/` and `index.docker.io` compare equal.
fn canonical_registry_key(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_v1 = without_scheme.strip_suffix("/v1").unwrap_or(without_scheme);
    let normalized = without_v1.trim_end_matches('/');

    if normalized == helpers::DEFAULT_REGISTRY_DOMAIN {
        helpers::DEFAULT_REGISTRY_HOST.to_string()
    } else {
        normalized.to_string()
    }
}

fn decode_base64(input: &str) -> Result<Vec<u8>> {
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
            _ => return Err(TrustError::InvalidAuthEncoding),
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
        return Err(TrustError::InvalidAuthEncoding);
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
        _ => return Err(TrustError::InvalidAuthEncoding),
    }

    Ok(())
}

fn encode_base64_urlsafe(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut index = 0usize;

    while index < input.len() {
        let remaining = input.len() - index;
        let chunk = &input[index..usize::min(index + 3, input.len())];

        let first = chunk[0] >> 2;
        let second =
            ((chunk[0] & 0b0000_0011) << 4) | (chunk.get(1).copied().unwrap_or(0) >> 4);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn encoded_env_auth_serializes_env_credentials() {
        let auth = encoded_env_auth_from_values(Some("containrrr-user"), Some("containrrr-pass"))
            .expect("environment credentials should encode");

        assert_eq!(
            auth,
            "eyJ1c2VybmFtZSI6ImNvbnRhaW5ycnItdXNlciIsInBhc3N3b3JkIjoiY29udGFpbnJyci1wYXNzIn0="
        );
    }

    #[test]
    fn encoded_config_auth_returns_empty_string_when_credentials_are_missing() {
        let config_path = write_temp_config("{}");

        let auth =
            encoded_config_auth_from_path("registry.example.com/team/image:latest", &config_path)
                .expect("config should be readable");

        assert_eq!(auth, "");
    }

    #[test]
    fn encoded_config_auth_returns_error_when_config_file_is_missing() {
        let path = unique_temp_path("missing-config.json");

        let err =
            encoded_config_auth_from_path("registry.example.com/team/image:latest", &path)
                .expect_err("missing config file should fail");

        assert!(
            matches!(err, TrustError::ConfigRead { path: missing_path, .. } if missing_path == path)
        );
    }

    #[test]
    fn resolve_config_path_appends_config_json_to_directories() {
        assert_eq!(
            resolve_config_path("/etc/docker"),
            PathBuf::from("/etc/docker/config.json")
        );
    }

    #[test]
    fn resolve_config_path_defaults_to_root_config() {
        assert_eq!(resolve_config_path(""), PathBuf::from("/config.json"));
        assert_eq!(resolve_config_path("/"), PathBuf::from("/config.json"));
    }

    #[test]
    fn encoded_config_auth_reads_inline_auths_entries() {
        let config_path = write_temp_config(
            r#"{
                "auths": {
                    "https://registry.example.com/v1/": {
                        "auth": "YWxpY2U6c2VjcmV0"
                    }
                }
            }"#,
        );

        let auth =
            encoded_config_auth_from_path("registry.example.com/team/image:latest", &config_path)
                .expect("config should decode");

        assert_eq!(
            auth,
            "eyJ1c2VybmFtZSI6ImFsaWNlIiwicGFzc3dvcmQiOiJzZWNyZXQifQ=="
        );
    }

    #[test]
    fn encoded_config_auth_reads_explicit_username_password() {
        let config_path = write_temp_config(
            r#"{
                "auths": {
                    "registry.example.com": {
                        "username": "alice",
                        "password": "secret"
                    }
                }
            }"#,
        );

        let auth =
            encoded_config_auth_from_path("registry.example.com/team/image:latest", &config_path)
                .expect("config fallback should succeed");

        assert_eq!(
            auth,
            "eyJ1c2VybmFtZSI6ImFsaWNlIiwicGFzc3dvcmQiOiJzZWNyZXQifQ=="
        );
    }

    #[test]
    fn base64_round_trip() {
        let original = b"hello world";
        let encoded = encode_base64_urlsafe(original);
        let decoded = decode_base64(&encoded).expect("should decode");
        assert_eq!(decoded, original.to_vec());
    }

    fn write_temp_config(contents: &str) -> PathBuf {
        let dir = unique_temp_path("docker-config");
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("config.json");
        fs::write(&path, contents).expect("write temp config");
        path
    }

    fn unique_temp_path(stem: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be monotonic enough for tests")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "watchtower-ng-trust-{}-{}-{}",
            std::process::id(),
            stem,
            nanos
        ))
    }
}
