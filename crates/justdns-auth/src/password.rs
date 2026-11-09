// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Password hashing and verification with Argon2id (the library default).

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

/// Hash `password` into a PHC string (algorithm, parameters and salt included),
/// suitable for storing. A fresh random salt is drawn per call.
pub(crate) fn hash(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| error.to_string())
}

/// Whether `password` matches the stored PHC `hash`. A malformed stored hash
/// verifies as false rather than erroring, so a corrupted record cannot be used
/// to sign in.
pub(crate) fn verify(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrips() {
        let stored = hash("correct horse battery staple").unwrap();
        assert!(verify("correct horse battery staple", &stored));
        assert!(!verify("wrong password", &stored));
    }

    #[test]
    fn hashes_are_salted_and_differ() {
        let a = hash("same").unwrap();
        let b = hash("same").unwrap();
        assert_ne!(a, b, "a fresh salt makes each hash unique");
        assert!(verify("same", &a) && verify("same", &b));
    }

    #[test]
    fn malformed_hash_never_verifies() {
        assert!(!verify("anything", "not-a-phc-string"));
        assert!(!verify("anything", ""));
    }
}
