// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persisted account list.
//!
//! Users are kept in a JSON file next to the configuration, each with an Argon2
//! password hash — never a plaintext password. The file holds a list even
//! though Slice 3 only ever creates one administrator, so growing into real
//! multi-user management later needs no migration.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::password;

/// A single account. The hash is a PHC string (algorithm, parameters and salt
/// baked in); the plaintext password is never stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// The login name, compared case-sensitively.
    pub username: String,
    /// The Argon2 PHC hash of the password.
    pub password_hash: String,
}

/// What the stored file deserialises into. Wrapped in its own struct so extra
/// top-level fields can be added later without breaking the format.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Document {
    #[serde(default)]
    users: Vec<User>,
}

/// Things that can go wrong while loading, saving or using the account store.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("auth io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("auth parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// Setup was attempted when an administrator already exists.
    #[error("an administrator already exists")]
    AlreadyConfigured,
    /// A username or password failed a basic validity check.
    #[error("{0}")]
    Invalid(String),
    /// Hashing the password failed.
    #[error("cannot hash password: {0}")]
    Hash(String),
}

struct Inner {
    path: PathBuf,
    users: RwLock<Vec<User>>,
}

/// A cheap, cloneable handle to the persistent account list.
#[derive(Clone)]
pub struct UserStore {
    inner: Arc<Inner>,
}

impl UserStore {
    /// Load the account list from `path`. A missing file is treated as "no
    /// users yet" and is not written until the first account is created; a file
    /// that exists but cannot be parsed is reported as an error rather than
    /// silently discarded, so a damaged file never locks everyone out by
    /// looking empty.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, AuthError> {
        let path = path.into();
        let users = match std::fs::read(&path) {
            Ok(bytes) => {
                let doc: Document =
                    serde_json::from_slice(&bytes).map_err(|source| AuthError::Parse {
                        path: path.clone(),
                        source,
                    })?;
                doc.users
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(source) => {
                return Err(AuthError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };

        Ok(Self {
            inner: Arc::new(Inner {
                path,
                users: RwLock::new(users),
            }),
        })
    }

    /// Whether at least one account exists. The login screen uses this to decide
    /// between asking for a password and offering first-run setup.
    pub fn is_configured(&self) -> bool {
        !self
            .inner
            .users
            .read()
            .expect("user lock poisoned")
            .is_empty()
    }

    /// Create the first administrator. Fails if an account already exists, so
    /// the setup endpoint cannot be used to overwrite the admin once the box is
    /// configured. On success the new list is persisted before returning.
    pub fn create_admin(&self, username: &str, password: &str) -> Result<(), AuthError> {
        let username = username.trim();
        if username.is_empty() {
            return Err(AuthError::Invalid("username must not be empty".into()));
        }
        if password.len() < 8 {
            return Err(AuthError::Invalid(
                "password must be at least 8 characters".into(),
            ));
        }

        let hash = password::hash(password).map_err(AuthError::Hash)?;

        let mut users = self.inner.users.write().expect("user lock poisoned");
        if !users.is_empty() {
            return Err(AuthError::AlreadyConfigured);
        }
        users.push(User {
            username: username.to_owned(),
            password_hash: hash,
        });
        // Persist while still holding the lock so the file and memory agree.
        write_atomic(&self.inner.path, &users)
    }

    /// Check a login. Returns the matched username on success so the caller can
    /// start a session, or `None` for an unknown user or a wrong password. The
    /// password is always verified against *some* hash to keep the timing of a
    /// wrong username and a wrong password alike.
    pub fn verify(&self, username: &str, password: &str) -> Option<String> {
        let users = self.inner.users.read().expect("user lock poisoned");
        match users.iter().find(|user| user.username == username) {
            Some(user) if password::verify(password, &user.password_hash) => {
                Some(user.username.clone())
            }
            Some(_) => None,
            None => {
                // Verify against a throwaway hash so a missing user costs the
                // same time as a present one with a wrong password.
                password::verify(password, DUMMY_HASH);
                None
            }
        }
    }
}

/// A valid Argon2 PHC hash (of a random value) used only to equalise the timing
/// of a login for a user that does not exist.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$cnxHBvex1fxUfB7TBjNOAw$SNdG2C5F+eV/ccnN/9oCtBZiK+YMaMuFUFeUBIucL1E";

/// Write the account list to `path` atomically: serialise to a sibling temp
/// file, then rename over the target so a crash mid-write never leaves a
/// half-written file that could lock everyone out.
fn write_atomic(path: &Path, users: &[User]) -> Result<(), AuthError> {
    let io_err = |source| AuthError::Io {
        path: path.to_path_buf(),
        source,
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
    }

    let doc = Document {
        users: users.to_vec(),
    };
    let json = serde_json::to_vec_pretty(&doc).map_err(|source| AuthError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| AuthError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(io_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        (dir, path)
    }

    #[test]
    fn starts_unconfigured_without_a_file() {
        let (_dir, path) = temp_path();
        let store = UserStore::load(&path).unwrap();
        assert!(!store.is_configured());
        assert!(!path.exists(), "no file is written until an account exists");
    }

    #[test]
    fn create_admin_persists_and_logs_in() {
        let (_dir, path) = temp_path();
        let store = UserStore::load(&path).unwrap();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        assert!(store.is_configured());

        // The hash is on disk, not the plaintext password.
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("admin"));
        assert!(!written.contains("hunter2hunter2"));
        assert!(written.contains("$argon2"));

        // A reload sees the account and accepts the login.
        let reopened = UserStore::load(&path).unwrap();
        assert_eq!(
            reopened.verify("admin", "hunter2hunter2").as_deref(),
            Some("admin")
        );
        assert_eq!(reopened.verify("admin", "wrong"), None);
        assert_eq!(reopened.verify("ghost", "hunter2hunter2"), None);
    }

    #[test]
    fn second_admin_is_refused() {
        let (_dir, path) = temp_path();
        let store = UserStore::load(&path).unwrap();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        assert!(matches!(
            store.create_admin("eve", "hunter2hunter2"),
            Err(AuthError::AlreadyConfigured)
        ));
    }

    #[test]
    fn weak_or_empty_credentials_are_rejected() {
        let (_dir, path) = temp_path();
        let store = UserStore::load(&path).unwrap();
        assert!(matches!(
            store.create_admin("", "hunter2hunter2"),
            Err(AuthError::Invalid(_))
        ));
        assert!(matches!(
            store.create_admin("admin", "short"),
            Err(AuthError::Invalid(_))
        ));
        // A rejected attempt leaves the store unconfigured.
        assert!(!store.is_configured());
    }

    #[test]
    fn dummy_hash_is_a_valid_phc_string() {
        // If this ever fails, the wrong-username path skips real work and the
        // timing-equalisation comment on verify() stops being true.
        assert!(
            argon2::password_hash::PasswordHash::new(DUMMY_HASH).is_ok(),
            "DUMMY_HASH must parse as a real Argon2 hash"
        );
    }

    #[test]
    fn unparseable_file_is_an_error() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            UserStore::load(&path),
            Err(AuthError::Parse { .. })
        ));
    }
}
