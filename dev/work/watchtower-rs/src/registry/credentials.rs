#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::helpers;

/// Result type used by the registry credential helpers.
pub type Result<T> = std::result::Result<T, CredentialsError>;

/// Errors raised while resolving registry credentials.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialsError {
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
pub fn encoded_auth(image_ref: &str) -> Result<String> {
    match encoded_env_auth() {
        Ok(auth) => Ok(auth),
        Err(_) => encoded_config_auth(image_ref),
    }
}

/// Encode registry credentials from `REPO_USER` and `REPO_PASS`.
pub fn encoded_env_auth() -> Result<String> {
    let username = env::var("REPO_USER").ok().filter(|value| !value.is_empty());
    let password = env::var("REPO_PASS").ok().filter(|value| !value.is_empty());

    encoded_env_auth_from_values(username.as_deref(), password.as_deref())
}

/// Encode registry credentials from a Docker `config.json`-style file.
pub fn encoded_config_auth(image_ref: &str) -> Result<String> {
    let config_path = docker_config_path()?;

    encoded_config_auth_from_path(image_ref, &config_path)
}

/// Encode registry credentials from a parsed Docker config file.
fn encoded_config_auth_from_path(image_ref: &str, config_path: &Path) -> Result<String> {
    let registry = helpers::get_registry_address(image_ref)?;
    let config = DockerConfigFile::load(config_path)?;
    Ok(config
        .encoded_auth_for_registry(&registry)?
        .unwrap_or_default())
}

/// Encode credentials from explicit environment values.
fn encoded_env_auth_from_values(username: Option<&str>, password: Option<&str>) -> Result<String> {
    let Some(username) = username.filter(|value| !value.is_empty()) else {
        return Err(CredentialsError::MissingEnvironmentCredentials);
    };
    let Some(password) = password.filter(|value| !value.is_empty()) else {
        return Err(CredentialsError::MissingEnvironmentCredentials);
    };

    encode_auth_json(username, password)
}

/// Resolve the default Docker config path.
///
/// The legacy Go runtime fell back to `/config.json` when `DOCKER_CONFIG` was
/// not set. When `DOCKER_CONFIG` is present it is treated as either a directory
/// or a direct path to `config.json`.
fn docker_config_path() -> Result<PathBuf> {
    docker_config_path_from(env::var_os("DOCKER_CONFIG"))
}

fn docker_config_path_from(raw: Option<std::ffi::OsString>) -> Result<PathBuf> {
    if let Some(raw) = raw {
        let path = PathBuf::from(raw);
        return Ok(resolve_docker_config_file(path));
    }

    Ok(PathBuf::from("/config.json"))
}

fn resolve_docker_config_file(base: PathBuf) -> PathBuf {
    if base
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        base
    } else {
        base.join("config.json")
    }
}

fn encode_auth_json(username: &str, password: &str) -> Result<String> {
    let payload = AuthPayload { username, password };
    let serialized = serde_json::to_vec(&payload)
        .map_err(|err| CredentialsError::ConfigParse { message: err.to_string() })?;

    Ok(encode_base64_urlsafe(&serialized))
}

fn decode_auth_json(auth: &str) -> Result<(String, String)> {
    let decoded = decode_base64(auth)?;
    let decoded = String::from_utf8(decoded).map_err(|_| CredentialsError::InvalidAuthEncoding)?;
    let Some((username, password)) = decoded.split_once(':') else {
        return Err(CredentialsError::InvalidAuthEncoding);
    };

    if username.is_empty() || password.is_empty() {
        return Err(CredentialsError::InvalidAuthEncoding);
    }

    Ok((username.to_string(), password.to_string()))
}

/// Parsed representation of a Docker config file.
///
/// The Rust port intentionally supports inline `auths` entries only. External
/// credential helpers (`credsStore`) are parsed but not resolved.
#[derive(Debug, Default, Deserialize)]
struct DockerConfigFile {
    #[serde(default)]
    auths: BTreeMap<String, DockerAuthEntry>,
    #[serde(default, rename = "credsStore")]
    _creds_store: Option<String>,
}

impl DockerConfigFile {
    fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|err| CredentialsError::ConfigRead {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;

        serde_json::from_str(&contents).map_err(|err| CredentialsError::ConfigParse {
            message: err.to_string(),
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
        if let Some(auth) = self.auth.as_deref().filter(|value| !value.is_empty()) {
            let (username, password) = decode_auth_json(auth)?;
            return encode_auth_json(&username, &password);
        }

        let Some(username) = self.username.as_deref().filter(|value| !value.is_empty()) else {
            return Ok(String::new());
        };
        let Some(password) = self.password.as_deref().filter(|value| !value.is_empty()) else {
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
            _ => return Err(CredentialsError::InvalidAuthEncoding),
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
        return Err(CredentialsError::InvalidAuthEncoding);
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
        _ => return Err(CredentialsError::InvalidAuthEncoding),
    }

    Ok(())
}

fn encode_base64_urlsafe(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

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

        let auth = encoded_config_auth_from_path("registry.example.com/team/image:latest", &config_path)
            .expect("config should be readable");

        assert_eq!(auth, "");
    }

    #[test]
    fn encoded_config_auth_returns_error_when_config_file_is_missing() {
        let path = unique_temp_path("missing-config.json");

        let err = encoded_config_auth_from_path("registry.example.com/team/image:latest", &path)
            .expect_err("missing config file should fail");

        assert!(matches!(err, CredentialsError::ConfigRead { path: missing_path, .. } if missing_path == path));
    }

    #[test]
    fn docker_config_path_defaults_to_root_config_file() {
        assert_eq!(
            docker_config_path_from(None).expect("default path should resolve"),
            PathBuf::from("/config.json")
        );
    }

    #[test]
    fn docker_config_path_treats_environment_value_as_directory_or_file() {
        assert_eq!(
            docker_config_path_from(Some("/tmp/docker-config".into()))
                .expect("directory should resolve"),
            PathBuf::from("/tmp/docker-config/config.json")
        );
        assert_eq!(
            docker_config_path_from(Some("/tmp/docker-config.json".into()))
                .expect("file should resolve"),
            PathBuf::from("/tmp/docker-config.json")
        );
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

        let auth = encoded_config_auth_from_path("registry.example.com/team/image:latest", &config_path)
            .expect("config should decode");

        assert_eq!(
            auth,
            "eyJ1c2VybmFtZSI6ImFsaWNlIiwicGFzc3dvcmQiOiJzZWNyZXQifQ=="
        );
    }

    #[test]
    fn encoded_auth_falls_back_to_the_config_file() {
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

        let auth = encoded_auth_with(
            "registry.example.com/team/image:latest",
            None,
            None,
            &config_path,
        )
        .expect("config fallback should succeed");

        assert_eq!(
            auth,
            "eyJ1c2VybmFtZSI6ImFsaWNlIiwicGFzc3dvcmQiOiJzZWNyZXQifQ=="
        );
    }

    fn encoded_auth_with(
        image_ref: &str,
        username: Option<&str>,
        password: Option<&str>,
        config_path: &Path,
    ) -> Result<String> {
        match encoded_env_auth_from_values(username, password) {
            Ok(auth) => Ok(auth),
            Err(_) => encoded_config_auth_from_path(image_ref, config_path),
        }
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
            "watchtower-ng-credentials-{}-{}-{}",
            std::process::id(),
            stem,
            nanos
        ))
    }
}
