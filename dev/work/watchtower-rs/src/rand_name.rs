#![forbid(unsafe_code)]

//! Random Docker-compatible container name generation.
//!
//! Translated from `internal/util/rand_name.go`.

use rand::Rng;

const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_name_has_expected_length() {
        assert_eq!(rand_name().len(), 32);
    }

    #[test]
    fn rand_name_uses_only_letters() {
        let name = rand_name();

        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_alphabetic() && ch.is_ascii())
        );
    }
}
