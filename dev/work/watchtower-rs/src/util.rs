#![forbid(unsafe_code)]

//! Small utility helpers translated from the legacy Go `internal/util` package.
//!
//! This module keeps the original semantics for:
//! - slice equality and subtraction helpers from `util.go`
//! - random container-name generation from `rand_name.go`
//! - random SHA-256 helpers from `rand_sha256.go`
//!
//! The Go random helpers ignored read errors from the random source; this
//! implementation keeps that behavior by falling back to zeroed bytes when
//! `/dev/urandom` cannot be read.

use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;

const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const SHA256_HEX_BYTES: usize = 32;
const SHA256_PREFIX: &str = "sha256:";
const SHA256_PREFIX_LEN: usize = SHA256_PREFIX.len();

/// Compare two string slices for exact positional equality.
pub fn slice_equal(s1: &[String], s2: &[String]) -> bool {
    s1 == s2
}

/// Subtract the contents of `a2` from `a1` without mutating either input.
///
/// The result preserves the order from `a1`.
pub fn slice_subtract(a1: &[String], a2: &[String]) -> Vec<String> {
    let remove: HashSet<&str> = a2.iter().map(String::as_str).collect();
    a1.iter()
        .filter(|value| !remove.contains(value.as_str()))
        .cloned()
        .collect()
}

/// Subtract entries from `m1` when `m2` contains the same key and value.
pub fn string_map_subtract(
    m1: &HashMap<String, String>,
    m2: &HashMap<String, String>,
) -> HashMap<String, String> {
    m1.iter()
        .filter_map(|(key, value)| match m2.get(key) {
            Some(other) if other == value => None,
            _ => Some((key.clone(), value.clone())),
        })
        .collect()
}

/// Subtract entries from `m1` when `m2` contains the same key.
pub fn struct_map_subtract(
    m1: &HashMap<String, ()>,
    m2: &HashMap<String, ()>,
) -> HashMap<String, ()> {
    m1.iter()
        .filter_map(|(key, value)| {
            if m2.contains_key(key) {
                None
            } else {
                Some((key.clone(), *value))
            }
        })
        .collect()
}

/// Generate a random 32-character, Docker-compatible container name.
///
/// The legacy Go helper used `math/rand` and drew each character from the same
/// ASCII letter set. This Rust version keeps the same shape and length.
pub fn rand_name() -> String {
    let mut rng = rand::thread_rng();
    let mut name = String::with_capacity(32);

    for _ in 0..32 {
        let idx = rng.gen_range(0..LETTERS.len());
        name.push(LETTERS[idx] as char);
    }

    name
}

fn read_random_bytes() -> [u8; SHA256_HEX_BYTES] {
    let mut bytes = [0u8; SHA256_HEX_BYTES];

    if let Ok(mut file) = File::open("/dev/urandom") {
        let _ = file.read_exact(&mut bytes);
    }

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
    fn slice_equal_matches_exact_order_and_length() {
        let s1 = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let s2 = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        assert!(slice_equal(&s1, &s2));
    }

    #[test]
    fn slice_equal_rejects_different_lengths() {
        let s1 = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let s2 = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];

        assert!(!slice_equal(&s1, &s2));
    }

    #[test]
    fn slice_equal_rejects_different_content() {
        let s1 = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let s2 = vec!["a".to_string(), "b".to_string(), "d".to_string()];

        assert!(!slice_equal(&s1, &s2));
    }

    #[test]
    fn slice_subtract_removes_matching_values_and_keeps_order() {
        let a1 = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let a2 = vec!["a".to_string(), "c".to_string()];

        let result = slice_subtract(&a1, &a2);

        assert_eq!(vec!["b".to_string()], result);
        assert_eq!(vec!["a".to_string(), "b".to_string(), "c".to_string()], a1);
        assert_eq!(vec!["a".to_string(), "c".to_string()], a2);
    }

    #[test]
    fn string_map_subtract_keeps_different_values() {
        let mut m1 = HashMap::new();
        m1.insert("a".to_string(), "a".to_string());
        m1.insert("b".to_string(), "b".to_string());
        m1.insert("c".to_string(), "sea".to_string());

        let mut m2 = HashMap::new();
        m2.insert("a".to_string(), "a".to_string());
        m2.insert("c".to_string(), "c".to_string());

        let result = string_map_subtract(&m1, &m2);

        let mut expected = HashMap::new();
        expected.insert("b".to_string(), "b".to_string());
        expected.insert("c".to_string(), "sea".to_string());

        assert_eq!(expected, result);
        assert_eq!(m1.get("a").map(String::as_str), Some("a"));
        assert_eq!(m2.get("a").map(String::as_str), Some("a"));
    }

    #[test]
    fn struct_map_subtract_keeps_keys_missing_from_rhs() {
        let mut m1 = HashMap::new();
        m1.insert("a".to_string(), ());
        m1.insert("b".to_string(), ());
        m1.insert("c".to_string(), ());

        let mut m2 = HashMap::new();
        m2.insert("a".to_string(), ());
        m2.insert("c".to_string(), ());

        let result = struct_map_subtract(&m1, &m2);

        let mut expected = HashMap::new();
        expected.insert("b".to_string(), ());

        assert_eq!(expected, result);
        assert_eq!(m1.len(), 3);
        assert_eq!(m2.len(), 2);
    }

    #[test]
    fn rand_name_has_expected_length() {
        assert_eq!(rand_name().len(), 32);
    }

    #[test]
    fn rand_name_uses_only_letters() {
        let name = rand_name();

        assert!(name
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() && ch.is_ascii()));
    }

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
        assert!(value[SHA256_PREFIX_LEN..]
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
    }

    #[test]
    fn generate_random_sha256_returns_unprefixed_hex_string() {
        let value = generate_random_sha256();

        assert_eq!(value.len(), SHA256_HEX_BYTES * 2);
        assert!(value.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
    }
}
