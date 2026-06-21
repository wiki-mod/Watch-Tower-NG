#![forbid(unsafe_code)]

//! Random SHA-256 hash generation.
//!
//! Translated from `internal/util/rand_sha256.go`.

use rand::RngCore;
use std::fmt::Write as _;

const SHA256_HEX_BYTES: usize = 32;
const SHA256_PREFIX: &str = "sha256:";
const SHA256_PREFIX_LEN: usize = SHA256_PREFIX.len();

fn read_random_bytes() -> [u8; SHA256_HEX_BYTES] {
    let mut bytes = [0u8; SHA256_HEX_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

fn format_prefixed_sha256(bytes: &[u8; SHA256_HEX_BYTES]) -> String {
    let mut output = String::with_capacity(SHA256_PREFIX_LEN + SHA256_HEX_BYTES * 2);
    output.push_str(SHA256_PREFIX);

    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }

    output
}

/// Generate a random 64-character SHA-256 hash string.
pub fn generate_random_sha256() -> String {
    generate_random_prefixed_sha256()[SHA256_PREFIX_LEN..].to_string()
}

/// Generate a random 64-character SHA-256 hash string prefixed with `sha256:`.
pub fn generate_random_prefixed_sha256() -> String {
    let bytes = read_random_bytes();
    format_prefixed_sha256(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_prefixed_sha256_matches_go_layout() {
        let bytes = [0x12u8; SHA256_HEX_BYTES];
        let formatted = format_prefixed_sha256(&bytes);

        assert_eq!(
            formatted,
            "sha256:1212121212121212121212121212121212121212121212121212121212121212"
        );
    }

    #[test]
    fn generate_random_prefixed_sha256_returns_prefixed_hex_string() {
        let value = generate_random_prefixed_sha256();

        assert_eq!(value.len(), SHA256_PREFIX_LEN + SHA256_HEX_BYTES * 2);
        assert!(value.starts_with(SHA256_PREFIX));
        assert!(
            value[SHA256_PREFIX_LEN..]
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        );
    }

    #[test]
    fn generate_random_sha256_returns_unprefixed_hex_string() {
        let value = generate_random_sha256();

        assert_eq!(value.len(), SHA256_HEX_BYTES * 2);
        assert!(
            value
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        );
    }
}
