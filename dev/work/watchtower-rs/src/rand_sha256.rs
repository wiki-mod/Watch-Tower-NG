#![forbid(unsafe_code)]

//! Random SHA-256 hash generation.
//!
//! Translated from `internal/util/rand_sha256.go`.

use rand::RngCore;
use std::fmt::Write as _;

/// Generate a random 64-character SHA-256 hash string.
pub fn generate_random_sha256() -> String {
    generate_random_prefixed_sha256()[7..].to_string()
}

/// Generate a random 64-character SHA-256 hash string prefixed with `sha256:`.
pub fn generate_random_prefixed_sha256() -> String {
    let mut hash = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut hash);

    let mut output = String::with_capacity(7 + 64);
    output.push_str("sha256:");

    for byte in hash {
        let _ = write!(&mut output, "{byte:02x}");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_random_prefixed_sha256_returns_prefixed_hex_string() {
        let value = generate_random_prefixed_sha256();

        assert_eq!(value.len(), 7 + 64);
        assert!(value.starts_with("sha256:"));
        assert!(
            value[7..]
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        );
    }

    #[test]
    fn generate_random_sha256_returns_unprefixed_hex_string() {
        let value = generate_random_sha256();

        assert_eq!(value.len(), 64);
        assert!(
            value
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        );
    }
}
