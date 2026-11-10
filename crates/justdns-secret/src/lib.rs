// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Encryption at rest for the on-disk secret files.
//!
//! JustDNS keeps its credentials (password hashes, TOTP secrets, recovery codes)
//! and its secret settings (webhook/bot tokens) in JSON files on the data volume.
//! On their own those files are plaintext, so a copied volume, a backup or a VM
//! snapshot hands an attacker the whole credential database. This module seals
//! each such file under a machine-held [`Sealer`] so the file at rest is useless
//! without the key.
//!
//! The key is a 32-byte random master key, generated on first boot and kept in a
//! **separate** key file (`JUSTDNS_SECRET_KEY_PATH`, a volume of its own in the
//! container) so that stealing the data volume alone does not also yield the key.
//! From the master key a per-purpose subkey is derived with HKDF-SHA256, and the
//! payload is sealed with ChaCha20-Poly1305. The on-disk envelope is
//! `MAGIC || nonce (12) || ciphertext || tag`.
//!
//! This protects data **at rest** — a stolen volume, backup or snapshot. It does
//! not defend a fully compromised, running host (which can read the key file the
//! service itself must read unattended); that is the deliberate price of needing
//! no passphrase at boot.

use std::io;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{self, HKDF_SHA256, KeyType};
use ring::rand::{SecureRandom, SystemRandom};

/// Marks a file as a sealed JustDNS secret. Versioned so the scheme can change
/// without a new format being mistaken for an old one.
const MAGIC: &[u8] = b"JDSEC1";

/// Marks a **password-encrypted backup** file. Distinct from [`MAGIC`] (the
/// machine-key at-rest format) because a backup is keyed by a user password, not
/// the master key, and carries its own salt.
const BACKUP_MAGIC: &[u8] = b"JDBAK1";

/// Length of the per-backup random salt fed to the password KDF.
const SALT_LEN: usize = 16;

/// Non-secret HKDF salt, versioned alongside [`MAGIC`].
const HKDF_SALT: &[u8] = b"justdns-secret-at-rest-salt-v1";

/// The length of the master key and of each derived subkey.
const KEY_LEN: usize = 32;

/// What can go wrong setting up the sealer.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret key file io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("secret key file ({path}) is {len} bytes, expected {KEY_LEN}")]
    BadKeyLength { path: PathBuf, len: usize },
}

/// The length the AEAD key is expanded to by HKDF.
struct KeyLen;
impl KeyType for KeyLen {
    fn len(&self) -> usize {
        KEY_LEN
    }
}

/// Holds the master key and derives per-purpose AEAD keys from it. Cheap to
/// clone (just copies 32 bytes), so every store can keep its own handle.
#[derive(Clone)]
pub struct Sealer {
    master: [u8; KEY_LEN],
}

impl Sealer {
    /// Load the master key from `path`, or generate and persist one if the file
    /// does not exist yet. A fresh key is written with owner-only permissions and
    /// flushed to disk before it is used, so a crash cannot leave a half-written
    /// key that would make already-sealed files unreadable.
    ///
    /// A present file of the wrong length is an error rather than a silent
    /// regeneration: regenerating would change the key and make every sealed file
    /// undecryptable, so we stop loudly instead.
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, SecretError> {
        let path = path.as_ref();
        match std::fs::read(path) {
            Ok(bytes) => {
                let len = bytes.len();
                let master: [u8; KEY_LEN] =
                    bytes.try_into().map_err(|_| SecretError::BadKeyLength {
                        path: path.to_path_buf(),
                        len,
                    })?;
                Ok(Self { master })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::generate(path),
            Err(source) => Err(SecretError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// A sealer with a throwaway in-memory key. For tests: it encrypts for real,
    /// but the key is never persisted, so it is useless beyond the test process.
    pub fn ephemeral() -> Self {
        let mut master = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut master)
            .expect("system RNG is available");
        Self { master }
    }

    fn generate(path: &Path) -> Result<Self, SecretError> {
        let io_err = |source| SecretError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(io_err)?;
            }
        }
        let mut master = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut master)
            .expect("system RNG is available");
        write_key_file(path, &master).map_err(io_err)?;
        Ok(Self { master })
    }

    /// Derive the AEAD key for a named purpose (e.g. `"auth-v1"`), so distinct
    /// files never share the exact key even though they share the master.
    fn subkey(&self, purpose: &str) -> LessSafeKey {
        let prk = hkdf::Salt::new(HKDF_SHA256, HKDF_SALT).extract(&self.master);
        let info = [purpose.as_bytes()];
        let okm = prk
            .expand(&info, KeyLen)
            .expect("hkdf expand to a fixed length cannot fail");
        let mut key_bytes = [0u8; KEY_LEN];
        okm.fill(&mut key_bytes)
            .expect("hkdf fill of a matching-length buffer cannot fail");
        let unbound = UnboundKey::new(&aead::CHACHA20_POLY1305, &key_bytes)
            .expect("a 32-byte ChaCha20-Poly1305 key is always valid");
        LessSafeKey::new(unbound)
    }

    /// Seal `plaintext` for `purpose`, returning `MAGIC || nonce || ciphertext ||
    /// tag`. A fresh random nonce per call keeps reuse far below the birthday
    /// bound for the handful of writes these files see.
    pub fn seal(&self, purpose: &str, plaintext: &[u8]) -> Vec<u8> {
        let key = self.subkey(purpose);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .expect("system RNG is available");
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = plaintext.to_vec();
        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .expect("in-place seal cannot fail");
        let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + in_out.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&in_out);
        out
    }

    /// Open an envelope produced by [`seal`](Self::seal) for the same purpose.
    /// Returns `None` for a missing magic, a short payload, a wrong key or a
    /// tampered payload — all indistinguishable on purpose.
    pub fn open(&self, purpose: &str, data: &[u8]) -> Option<Vec<u8>> {
        let body = data.strip_prefix(MAGIC)?;
        if body.len() < NONCE_LEN {
            return None;
        }
        let (nonce_bytes, ciphertext) = body.split_at(NONCE_LEN);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes.try_into().ok()?);
        let key = self.subkey(purpose);
        let mut in_out = ciphertext.to_vec();
        let plaintext = key.open_in_place(nonce, Aad::empty(), &mut in_out).ok()?;
        Some(plaintext.to_vec())
    }
}

/// Whether `data` carries the sealed-file magic. Used by a store to tell an
/// already-encrypted file from a legacy plaintext one it should migrate.
pub fn is_sealed(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

// ---- Password-encrypted backups ------------------------------------------

/// What can go wrong opening a password-encrypted backup.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// The file is not a password-encrypted JustDNS backup (no magic).
    #[error("not a password-encrypted backup")]
    NotEncrypted,
    /// The file carries the magic but is truncated or structurally broken.
    #[error("the backup file is damaged")]
    Malformed,
    /// The password did not open the backup (wrong password or tampered file).
    #[error("wrong backup password")]
    WrongPassword,
}

/// Derive a 32-byte key from a user password and per-backup salt with Argon2id.
/// The parameters are fixed and versioned by [`BACKUP_MAGIC`], so they need not be
/// stored in the file; they match the memory-hard cost used for login hashing.
fn derive_backup_key(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let params = Params::new(19_456, 2, 1, Some(KEY_LEN)).expect("valid argon2 params");
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("argon2 derivation into a fixed-length buffer cannot fail");
    key
}

/// Encrypt `plaintext` (a serialized backup) under `password`, returning
/// `BACKUP_MAGIC || salt || nonce || ciphertext || tag`. A random salt per backup
/// means the same password never yields the same key twice.
pub fn seal_backup(password: &str, plaintext: &[u8]) -> Vec<u8> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    rng.fill(&mut salt).expect("system RNG is available");
    let key_bytes = derive_backup_key(password, &salt);
    let unbound = UnboundKey::new(&aead::CHACHA20_POLY1305, &key_bytes)
        .expect("a 32-byte ChaCha20-Poly1305 key is always valid");
    let key = LessSafeKey::new(unbound);

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes).expect("system RNG is available");
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .expect("in-place seal cannot fail");

    let mut out = Vec::with_capacity(BACKUP_MAGIC.len() + SALT_LEN + NONCE_LEN + in_out.len());
    out.extend_from_slice(BACKUP_MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&in_out);
    out
}

/// Whether `data` is a password-encrypted backup produced by [`seal_backup`]. Lets
/// a restore accept a legacy plaintext backup while still requiring the password
/// for an encrypted one.
pub fn is_backup_encrypted(data: &[u8]) -> bool {
    data.starts_with(BACKUP_MAGIC)
}

/// Decrypt a backup produced by [`seal_backup`] with `password`. Distinguishes an
/// unencrypted file ([`BackupError::NotEncrypted`]) from a wrong password, so the
/// caller can give a precise message.
pub fn open_backup(password: &str, data: &[u8]) -> Result<Vec<u8>, BackupError> {
    let body = data
        .strip_prefix(BACKUP_MAGIC)
        .ok_or(BackupError::NotEncrypted)?;
    if body.len() < SALT_LEN + NONCE_LEN {
        return Err(BackupError::Malformed);
    }
    let (salt, rest) = body.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);
    let key_bytes = derive_backup_key(password, salt);
    let unbound = UnboundKey::new(&aead::CHACHA20_POLY1305, &key_bytes)
        .map_err(|_| BackupError::Malformed)?;
    let key = LessSafeKey::new(unbound);
    let nonce =
        Nonce::assume_unique_for_key(nonce_bytes.try_into().map_err(|_| BackupError::Malformed)?);
    let mut in_out = ciphertext.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| BackupError::WrongPassword)?;
    Ok(plaintext.to_vec())
}

/// Write the 32-byte key with owner-only permissions where the platform supports
/// it, then flush to disk.
fn write_key_file(path: &Path, key: &[u8; KEY_LEN]) -> io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(key)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_under_the_same_sealer() {
        let sealer = Sealer::ephemeral();
        let sealed = sealer.seal("auth-v1", b"password hash and totp secret");
        assert!(is_sealed(&sealed));
        assert_ne!(&sealed[MAGIC.len()..], b"password hash and totp secret");
        assert_eq!(
            sealer.open("auth-v1", &sealed).as_deref(),
            Some(&b"password hash and totp secret"[..])
        );
    }

    #[test]
    fn a_different_purpose_or_key_cannot_open() {
        let sealer = Sealer::ephemeral();
        let sealed = sealer.seal("auth-v1", b"secret");
        assert_eq!(sealer.open("config-v1", &sealed), None, "purpose is bound");
        assert_eq!(
            Sealer::ephemeral().open("auth-v1", &sealed),
            None,
            "key is bound"
        );
    }

    #[test]
    fn tampering_and_junk_are_rejected() {
        let sealer = Sealer::ephemeral();
        let mut sealed = sealer.seal("auth-v1", b"secret");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert_eq!(sealer.open("auth-v1", &sealed), None);
        assert_eq!(sealer.open("auth-v1", b"not sealed at all"), None);
        assert_eq!(sealer.open("auth-v1", MAGIC), None, "magic but no body");
        assert!(!is_sealed(b"{\"plain\":true}"));
    }

    #[test]
    fn two_seals_differ_by_nonce() {
        let sealer = Sealer::ephemeral();
        assert_ne!(sealer.seal("p", b"x"), sealer.seal("p", b"x"));
    }

    #[test]
    fn key_is_generated_once_then_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("master.key");
        let first = Sealer::load_or_create(&path).unwrap();
        assert!(path.exists(), "key persisted on first use");
        let sealed = first.seal("auth-v1", b"secret");

        // A second load reads the same key, so it opens what the first sealed.
        let second = Sealer::load_or_create(&path).unwrap();
        assert_eq!(
            second.open("auth-v1", &sealed).as_deref(),
            Some(&b"secret"[..])
        );
    }

    #[test]
    fn backup_round_trips_under_the_right_password() {
        let sealed = seal_backup("correct horse battery staple", b"{\"settings\":1}");
        assert!(is_backup_encrypted(&sealed));
        assert!(!is_backup_encrypted(b"{\"plain\":true}"));
        assert_eq!(
            open_backup("correct horse battery staple", &sealed).unwrap(),
            b"{\"settings\":1}"
        );
    }

    #[test]
    fn backup_rejects_a_wrong_password_and_plaintext() {
        let sealed = seal_backup("right", b"secret config");
        assert!(matches!(
            open_backup("wrong", &sealed),
            Err(BackupError::WrongPassword)
        ));
        assert!(matches!(
            open_backup("right", b"{\"not\":\"encrypted\"}"),
            Err(BackupError::NotEncrypted)
        ));
        assert!(matches!(
            open_backup("right", BACKUP_MAGIC),
            Err(BackupError::Malformed)
        ));
    }

    #[test]
    fn two_backups_of_the_same_data_differ() {
        // Different salt + nonce each time, so identical input never produces an
        // identical file.
        assert_ne!(seal_backup("pw", b"x"), seal_backup("pw", b"x"));
    }

    #[test]
    fn a_truncated_key_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.key");
        std::fs::write(&path, b"too short").unwrap();
        assert!(matches!(
            Sealer::load_or_create(&path),
            Err(SecretError::BadKeyLength { .. })
        ));
    }
}
