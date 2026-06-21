#![forbid(unsafe_code)]

use thiserror::Error;

pub const DEFAULT_REGISTRY_DOMAIN: &str = "docker.io";
pub const DEFAULT_REGISTRY_HOST: &str = "index.docker.io";
pub const LEGACY_DEFAULT_REGISTRY_DOMAIN: &str = "index.docker.io";

pub type Result<T> = std::result::Result<T, RegistryError>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("image reference must not be empty")]
    EmptyReference,
    #[error("invalid image reference `{0}`")]
    InvalidReference(String),
}

pub fn get_registry_address(image_ref: &str) -> Result<String> {
    let image_ref = normalize_image_reference(image_ref)?;
    let address = registry_domain(image_ref)?;

    if address == DEFAULT_REGISTRY_DOMAIN {
        Ok(DEFAULT_REGISTRY_HOST.to_string())
    } else {
        Ok(address.to_string())
    }
}

fn normalize_image_reference(image_ref: &str) -> Result<&str> {
    let trimmed = image_ref.trim();

    if trimmed.is_empty() {
        return Err(RegistryError::EmptyReference);
    }

    if trimmed != image_ref {
        return Err(RegistryError::InvalidReference(image_ref.to_string()));
    }

    Ok(trimmed)
}

fn registry_domain(image_ref: &str) -> Result<&str> {
    let name = image_ref
        .split_once('@')
        .map_or(image_ref, |(name, _)| name);

    let Some((first_segment, _)) = name.split_once('/') else {
        return Ok(DEFAULT_REGISTRY_DOMAIN);
    };

    if first_segment.is_empty() {
        return Err(RegistryError::InvalidReference(image_ref.to_string()));
    }

    if first_segment == "localhost" || first_segment.contains('.') || first_segment.contains(':') {
        validate_registry_candidate(first_segment, image_ref)?;
        return Ok(first_segment);
    }

    Ok(DEFAULT_REGISTRY_DOMAIN)
}

fn validate_registry_candidate(candidate: &str, original: &str) -> Result<()> {
    if candidate.starts_with('[') {
        return validate_bracketed_host(candidate, original);
    }

    let colon_count = candidate.matches(':').count();
    if colon_count > 1 {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    if let Some((host, port)) = candidate.rsplit_once(':') {
        if host.is_empty() || port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(RegistryError::InvalidReference(original.to_string()));
        }
    }

    if candidate.contains('/') || candidate.contains('@') || candidate.contains(' ') {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    Ok(())
}

fn validate_bracketed_host(candidate: &str, original: &str) -> Result<()> {
    let closing = candidate
        .find(']')
        .ok_or_else(|| RegistryError::InvalidReference(original.to_string()))?;

    let host = &candidate[1..closing];
    if host.is_empty() {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    let remainder = &candidate[closing + 1..];
    if remainder.is_empty() {
        return Ok(());
    }

    let port = remainder
        .strip_prefix(':')
        .ok_or_else(|| RegistryError::InvalidReference(original.to_string()))?;

    if port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(RegistryError::InvalidReference(original.to_string()));
    }

    Ok(())
}
