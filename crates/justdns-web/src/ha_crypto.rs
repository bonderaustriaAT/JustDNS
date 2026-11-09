// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Authenticated encryption for the HA peer transport.
//!
//! The two nodes already share a sync key. From it both derive the same symmetric
//! key (HKDF-SHA256) and seal every sensitive peer payload with ChaCha20-Poly1305,
//! so the sync's secrets — certificate private keys, password hashes, MFA secrets —
//! never cross the wire in clear even when the transport is plain `http` on a LAN.
//!
//! Authenticity rides on the same key: only a holder can produce a payload that
//! opens, which replaces sending the key as a bearer token (so the key itself never
//! travels). The envelope is `nonce (12 bytes) || ciphertext || tag`.

use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{self, HKDF_SHA256, KeyType};
use ring::rand::{SecureRandom, SystemRandom};

/// Non-secret HKDF salt and info, versioned so the derivation can change without
/// silently interoperating with a different scheme.
const HKDF_SALT: &[u8] = b"justdns-ha-aead-salt-v1";
const HKDF_INFO: &[u8] = b"justdns-ha-aead-v1";

/// The 256-bit length the AEAD key is expanded to.
struct KeyLen;
impl KeyType for KeyLen {
    fn len(&self) -> usize {
        32
    }
}

/// Derive the per-cluster AEAD key from the shared sync key. Deterministic, so both
/// nodes derive the same key from the same sync key.
pub fn derive_key(sync_key: &str) -> LessSafeKey {
    let prk = hkdf::Salt::new(HKDF_SHA256, HKDF_SALT).extract(sync_key.as_bytes());
    let okm = prk
        .expand(&[HKDF_INFO], KeyLen)
        .expect("hkdf expand to a fixed length cannot fail");
    let mut key_bytes = [0u8; 32];
    okm.fill(&mut key_bytes)
        .expect("hkdf fill of a matching-length buffer cannot fail");
    let unbound = UnboundKey::new(&aead::CHACHA20_POLY1305, &key_bytes)
        .expect("a 32-byte ChaCha20-Poly1305 key is always valid");
    LessSafeKey::new(unbound)
}

/// Seal `plaintext` under `key`, returning `nonce || ciphertext || tag`. The nonce
/// is fresh OS randomness per call — safe here because the peer traffic is far below
/// the birthday bound for a 96-bit random nonce.
pub fn seal(key: &LessSafeKey, plaintext: &[u8]) -> Vec<u8> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .expect("system RNG is available");
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .expect("in-place seal cannot fail");
    let mut out = Vec::with_capacity(NONCE_LEN + in_out.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&in_out);
    out
}

/// Open an envelope produced by [`seal`]. Returns `None` for anything too short, a
/// wrong key, or a tampered payload — all indistinguishable on purpose.
pub fn open(key: &LessSafeKey, data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < NONCE_LEN {
        return None;
    }
    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce = Nonce::assume_unique_for_key(nonce_bytes.try_into().ok()?);
    let mut in_out = ciphertext.to_vec();
    let plaintext = key.open_in_place(nonce, Aad::empty(), &mut in_out).ok()?;
    Some(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_under_the_same_key() {
        let key = derive_key("a-shared-sync-key");
        let sealed = seal(&key, b"certificate private key material");
        assert_ne!(&sealed, b"certificate private key material");
        assert!(sealed.len() > NONCE_LEN + 16, "nonce + tag present");
        assert_eq!(
            open(&key, &sealed).as_deref(),
            Some(&b"certificate private key material"[..])
        );
    }

    #[test]
    fn a_different_key_cannot_open() {
        let sealed = seal(&derive_key("key-one"), b"secret");
        assert_eq!(open(&derive_key("key-two"), &sealed), None);
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        let key = derive_key("k");
        let mut sealed = seal(&key, b"secret");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert_eq!(open(&key, &sealed), None);
        assert_eq!(open(&key, b"short"), None);
    }

    #[test]
    fn two_seals_differ_by_nonce() {
        let key = derive_key("k");
        assert_ne!(seal(&key, b"x"), seal(&key, b"x"), "fresh nonce per seal");
    }
}
