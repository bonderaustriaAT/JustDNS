// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persisted account list.
//!
//! Users are kept in a JSON file next to the configuration, each with an Argon2
//! password hash — never a plaintext password — a [`Role`], and optional
//! second-factor ([`Mfa`]) state. Every field added after the first release has
//! a serde default, so an older `auth.json` (a lone admin with no role or MFA)
//! loads unchanged and keeps full control.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::mfa;
use crate::password;
use crate::role::{Role, default_admin};

/// Per-user second-factor state. The base32 TOTP `secret` is shared with the
/// user's authenticator app; `confirmed` is false until the user proves they can
/// produce a code; `recovery` holds Argon2 hashes of one-time recovery codes
/// (never the plaintext).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mfa {
    pub secret: String,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub recovery: Vec<String>,
}

/// A single account. The hash is a PHC string (algorithm, parameters and salt
/// baked in); the plaintext password is never stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// The login name, compared case-sensitively.
    pub username: String,
    /// The Argon2 PHC hash of the password.
    pub password_hash: String,
    /// What this account may do. Defaults to [`Role::Admin`] for a record written
    /// before roles existed, so the original admin keeps full control.
    #[serde(default = "default_admin")]
    pub role: Role,
    /// Second-factor state, or `None` when the user has not enrolled.
    #[serde(default)]
    pub mfa: Option<Mfa>,
    /// Whether this user is individually required to use MFA. The effective
    /// requirement is this OR the server-wide setting.
    #[serde(default)]
    pub mfa_required: bool,
}

/// A hash-free view of an account for the management UI and API — never carries
/// the password hash or the TOTP secret.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub username: String,
    pub role: Role,
    pub mfa_enabled: bool,
    pub mfa_required: bool,
}

impl UserInfo {
    fn of(user: &User) -> Self {
        Self {
            username: user.username.clone(),
            role: user.role,
            mfa_enabled: user.mfa.as_ref().is_some_and(|m| m.confirmed),
            mfa_required: user.mfa_required,
        }
    }
}

/// What the stored file deserialises into.
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
    /// A username is already taken.
    #[error("the username \"{0}\" is already taken")]
    Duplicate(String),
    /// No account with the given username exists.
    #[error("no account named \"{0}\"")]
    Unknown(String),
    /// The action would leave the box with no administrator.
    #[error("at least one administrator must remain")]
    LastAdmin,
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
        !self.read().is_empty()
    }

    /// Create the first administrator. Fails if an account already exists, so
    /// the setup endpoint cannot be used to overwrite the admin once the box is
    /// configured. On success the new list is persisted before returning.
    pub fn create_admin(&self, username: &str, password: &str) -> Result<(), AuthError> {
        let username = validate_username(username)?;
        validate_password(password)?;
        let hash = password::hash(password).map_err(AuthError::Hash)?;

        let mut users = self.write();
        if !users.is_empty() {
            return Err(AuthError::AlreadyConfigured);
        }
        users.push(User {
            username,
            password_hash: hash,
            role: Role::Admin,
            mfa: None,
            mfa_required: false,
        });
        self.persist(&users)
    }

    /// Check a login. Returns the matched username on success so the caller can
    /// start a session, or `None` for an unknown user or a wrong password. The
    /// password is always verified against *some* hash to keep the timing of a
    /// wrong username and a wrong password alike.
    pub fn verify(&self, username: &str, password: &str) -> Option<String> {
        let users = self.read();
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

    /// Whether `password` is correct for `username`, for self-service flows
    /// (changing one's own password, disabling one's own MFA).
    pub fn verify_password(&self, username: &str, password: &str) -> bool {
        self.verify(username, password).is_some()
    }

    // ---- Roles & user management -----------------------------------------

    /// The role of `username`, or `None` if no such account exists. Looked up
    /// live on every request, so a role change takes effect at once.
    pub fn role_of(&self, username: &str) -> Option<Role> {
        self.read()
            .iter()
            .find(|u| u.username == username)
            .map(|u| u.role)
    }

    /// Every account as a hash-free [`UserInfo`], for the management UI.
    pub fn list(&self) -> Vec<UserInfo> {
        self.read().iter().map(UserInfo::of).collect()
    }

    /// Whether an account with this username exists.
    pub fn exists(&self, username: &str) -> bool {
        self.read().iter().any(|u| u.username == username)
    }

    /// Create an account with a role and an initial password. Rejects a
    /// duplicate name or a weak password.
    pub fn add_user(
        &self,
        username: &str,
        password: &str,
        role: Role,
        mfa_required: bool,
    ) -> Result<(), AuthError> {
        let username = validate_username(username)?;
        validate_password(password)?;
        let hash = password::hash(password).map_err(AuthError::Hash)?;

        let mut users = self.write();
        if users.iter().any(|u| u.username == username) {
            return Err(AuthError::Duplicate(username));
        }
        users.push(User {
            username,
            password_hash: hash,
            role,
            mfa: None,
            mfa_required,
        });
        self.persist(&users)
    }

    /// Change a user's role. Refuses to demote the last administrator, so the
    /// box can never be left with nobody who can manage it.
    pub fn set_role(&self, username: &str, role: Role) -> Result<(), AuthError> {
        let mut users = self.write();
        let admin_count = users.iter().filter(|u| u.role == Role::Admin).count();
        let user = find_mut(&mut users, username)?;
        if user.role == Role::Admin && role != Role::Admin && admin_count <= 1 {
            return Err(AuthError::LastAdmin);
        }
        user.role = role;
        self.persist(&users)
    }

    /// Replace a user's password (an admin reset or a self-service change — the
    /// caller is responsible for authorising it). Rejects a weak password.
    pub fn set_password(&self, username: &str, new_password: &str) -> Result<(), AuthError> {
        validate_password(new_password)?;
        let hash = password::hash(new_password).map_err(AuthError::Hash)?;
        let mut users = self.write();
        find_mut(&mut users, username)?.password_hash = hash;
        self.persist(&users)
    }

    /// Set whether a user is individually required to use MFA.
    pub fn set_mfa_required(&self, username: &str, required: bool) -> Result<(), AuthError> {
        let mut users = self.write();
        find_mut(&mut users, username)?.mfa_required = required;
        self.persist(&users)
    }

    /// Delete an account. Refuses to remove the last administrator.
    pub fn remove_user(&self, username: &str) -> Result<(), AuthError> {
        let mut users = self.write();
        let Some(idx) = users.iter().position(|u| u.username == username) else {
            return Err(AuthError::Unknown(username.to_owned()));
        };
        let is_last_admin = users[idx].role == Role::Admin
            && users.iter().filter(|u| u.role == Role::Admin).count() <= 1;
        if is_last_admin {
            return Err(AuthError::LastAdmin);
        }
        users.remove(idx);
        self.persist(&users)
    }

    // ---- HA sync ----------------------------------------------------------

    /// A full copy of every account, for the HA sync snapshot. Carries the
    /// complete records — password hashes and TOTP secrets included — so a peer
    /// can adopt them unchanged; only ever sent over the key-authenticated peer
    /// channel.
    pub fn snapshot(&self) -> Vec<User> {
        self.read().clone()
    }

    /// Replace the whole account list with a merged set from an HA peer. This is a
    /// lockout guard, not an authorisation boundary: whoever can push a snapshot
    /// already holds the sync key, which Stage B trusts with the entire account
    /// database (hashes and TOTP secrets) — see the HA guide. The only thing
    /// refused here is a set with **no** administrator (returns
    /// [`AuthError::LastAdmin`] and keeps the current list), so a bad or hostile
    /// merge can never leave the node with nobody able to log in and manage it.
    /// Structurally invalid records (an empty username or password hash) and
    /// duplicate usernames (the first kept) are dropped first. On success the new
    /// list is persisted.
    pub fn replace(&self, incoming: Vec<User>) -> Result<(), AuthError> {
        let mut seen = std::collections::HashSet::new();
        let cleaned: Vec<User> = incoming
            .into_iter()
            .filter(|u| !u.username.trim().is_empty() && !u.password_hash.trim().is_empty())
            .filter(|u| seen.insert(u.username.clone()))
            .collect();
        if !cleaned.iter().any(|u| u.role == Role::Admin) {
            return Err(AuthError::LastAdmin);
        }
        let mut users = self.write();
        *users = cleaned;
        self.persist(&users)
    }

    // ---- MFA --------------------------------------------------------------

    /// Store a freshly generated (still unconfirmed) secret and the hashes of its
    /// recovery codes, replacing any prior enrolment in progress. The plaintext
    /// recovery codes are hashed here and never stored in the clear.
    pub fn begin_mfa(
        &self,
        username: &str,
        secret: &str,
        recovery_plaintext: &[String],
    ) -> Result<(), AuthError> {
        let recovery = hash_recovery(recovery_plaintext)?;
        let mut users = self.write();
        find_mut(&mut users, username)?.mfa = Some(Mfa {
            secret: secret.to_owned(),
            confirmed: false,
            recovery,
        });
        self.persist(&users)
    }

    /// Mark a user's in-progress enrolment confirmed, enabling MFA for them.
    pub fn confirm_mfa(&self, username: &str) -> Result<(), AuthError> {
        let mut users = self.write();
        let user = find_mut(&mut users, username)?;
        match user.mfa.as_mut() {
            Some(m) => m.confirmed = true,
            None => return Err(AuthError::Invalid("no enrolment to confirm".into())),
        }
        self.persist(&users)
    }

    /// Remove a user's MFA entirely (an admin reset after a lost device, or a
    /// self-service disable). If MFA is still required for them they must
    /// re-enrol before regaining access.
    pub fn clear_mfa(&self, username: &str) -> Result<(), AuthError> {
        let mut users = self.write();
        find_mut(&mut users, username)?.mfa = None;
        self.persist(&users)
    }

    /// Replace a user's recovery codes (regenerate). Only valid once MFA exists.
    pub fn replace_recovery(
        &self,
        username: &str,
        recovery_plaintext: &[String],
    ) -> Result<(), AuthError> {
        let recovery = hash_recovery(recovery_plaintext)?;
        let mut users = self.write();
        let user = find_mut(&mut users, username)?;
        match user.mfa.as_mut() {
            Some(m) => m.recovery = recovery,
            None => return Err(AuthError::Invalid("MFA is not set up".into())),
        }
        self.persist(&users)
    }

    /// Whether MFA is enrolled and confirmed for `username`.
    pub fn mfa_enabled(&self, username: &str) -> bool {
        self.read()
            .iter()
            .find(|u| u.username == username)
            .and_then(|u| u.mfa.as_ref())
            .is_some_and(|m| m.confirmed)
    }

    /// The user's individual MFA-required flag (the server-wide setting is
    /// combined with this by the caller).
    pub fn mfa_required_flag(&self, username: &str) -> bool {
        self.read()
            .iter()
            .find(|u| u.username == username)
            .is_some_and(|u| u.mfa_required)
    }

    /// Verify a TOTP `code` against the user's stored secret (confirmed or not,
    /// so the same call confirms an enrolment and authenticates a login).
    pub fn verify_totp(&self, username: &str, code: &str) -> bool {
        let users = self.read();
        match users
            .iter()
            .find(|u| u.username == username)
            .and_then(|u| u.mfa.as_ref())
        {
            Some(m) => mfa::verify_totp(&m.secret, code),
            None => false,
        }
    }

    /// Check a recovery `code`; on a match remove it (single use) and persist.
    /// Returns whether a code was consumed.
    pub fn consume_recovery(&self, username: &str, code: &str) -> bool {
        let candidate = mfa::normalise_recovery(code);
        if candidate.is_empty() {
            return false;
        }
        let mut users = self.write();
        let Some(user) = users.iter_mut().find(|u| u.username == username) else {
            return false;
        };
        let Some(m) = user.mfa.as_mut() else {
            return false;
        };
        let Some(pos) = m
            .recovery
            .iter()
            .position(|hash| password::verify(&candidate, hash))
        else {
            return false;
        };
        m.recovery.remove(pos);
        // Persist the consumption; a failed write leaves the code usable, which
        // is safer than losing it.
        let _ = self.persist(&users);
        true
    }

    // ---- internals --------------------------------------------------------

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<User>> {
        self.inner.users.read().expect("user lock poisoned")
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Vec<User>> {
        self.inner.users.write().expect("user lock poisoned")
    }

    fn persist(&self, users: &[User]) -> Result<(), AuthError> {
        write_atomic(&self.inner.path, users)
    }
}

/// Find a user by name for mutation, or report [`AuthError::Unknown`].
fn find_mut<'a>(users: &'a mut [User], username: &str) -> Result<&'a mut User, AuthError> {
    users
        .iter_mut()
        .find(|u| u.username == username)
        .ok_or_else(|| AuthError::Unknown(username.to_owned()))
}

/// Trim and validate a username; names are compared case-sensitively.
fn validate_username(username: &str) -> Result<String, AuthError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(AuthError::Invalid("username must not be empty".into()));
    }
    Ok(username.to_owned())
}

/// Enforce the minimum password length shared by every entry point.
fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.len() < 8 {
        return Err(AuthError::Invalid(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

/// Hash a batch of recovery codes (normalised) for storage.
fn hash_recovery(codes: &[String]) -> Result<Vec<String>, AuthError> {
    codes
        .iter()
        .map(|code| password::hash(&mfa::normalise_recovery(code)).map_err(AuthError::Hash))
        .collect()
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

    fn store() -> (tempfile::TempDir, UserStore) {
        let (dir, path) = temp_path();
        let store = UserStore::load(&path).unwrap();
        (dir, store)
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
        assert_eq!(store.role_of("admin"), Some(Role::Admin));

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("admin"));
        assert!(!written.contains("hunter2hunter2"));
        assert!(written.contains("$argon2"));

        let reopened = UserStore::load(&path).unwrap();
        assert_eq!(
            reopened.verify("admin", "hunter2hunter2").as_deref(),
            Some("admin")
        );
        assert_eq!(reopened.verify("admin", "wrong"), None);
    }

    #[test]
    fn old_record_without_role_loads_as_admin() {
        // A pre-roles auth.json: a user object with only name + hash.
        let (_dir, path) = temp_path();
        let hash = password::hash("hunter2hunter2").unwrap();
        let legacy = format!(r#"{{"users":[{{"username":"root","password_hash":{hash:?}}}]}}"#);
        std::fs::write(&path, legacy).unwrap();
        let store = UserStore::load(&path).unwrap();
        assert_eq!(store.role_of("root"), Some(Role::Admin));
        assert!(!store.mfa_enabled("root"));
        assert!(!store.mfa_required_flag("root"));
    }

    #[test]
    fn add_user_with_role_and_reject_duplicates() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        store
            .add_user("bob", "viewerpass12", Role::Viewer, false)
            .unwrap();
        assert_eq!(store.role_of("bob"), Some(Role::Viewer));
        assert!(matches!(
            store.add_user("bob", "another1234", Role::Editor, false),
            Err(AuthError::Duplicate(_))
        ));
        assert!(matches!(
            store.add_user("eve", "short", Role::Editor, false),
            Err(AuthError::Invalid(_))
        ));
    }

    #[test]
    fn last_admin_cannot_be_demoted_or_deleted() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        store
            .add_user("bob", "viewerpass12", Role::Viewer, false)
            .unwrap();
        assert!(matches!(
            store.set_role("admin", Role::Viewer),
            Err(AuthError::LastAdmin)
        ));
        assert!(matches!(
            store.remove_user("admin"),
            Err(AuthError::LastAdmin)
        ));
        // With a second admin, the first can be demoted.
        store.set_role("bob", Role::Admin).unwrap();
        store.set_role("admin", Role::Editor).unwrap();
        assert_eq!(store.role_of("admin"), Some(Role::Editor));
    }

    #[test]
    fn password_reset_and_self_verify() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        assert!(store.verify_password("admin", "hunter2hunter2"));
        store.set_password("admin", "brandnewpass1").unwrap();
        assert!(!store.verify_password("admin", "hunter2hunter2"));
        assert!(store.verify_password("admin", "brandnewpass1"));
    }

    #[test]
    fn mfa_enrolment_confirm_and_recovery() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        let secret = mfa::generate_secret();
        let codes = mfa::generate_recovery_codes();
        store.begin_mfa("admin", &secret, &codes).unwrap();
        assert!(!store.mfa_enabled("admin"), "unconfirmed yet");
        store.confirm_mfa("admin").unwrap();
        assert!(store.mfa_enabled("admin"));

        // A recovery code works once, then is spent.
        let one = &codes[0];
        assert!(store.consume_recovery("admin", one));
        assert!(!store.consume_recovery("admin", one));
        // A clearly wrong code never matches.
        assert!(!store.consume_recovery("admin", "ZZZZZ-ZZZZZ"));

        store.clear_mfa("admin").unwrap();
        assert!(!store.mfa_enabled("admin"));
    }

    #[test]
    fn mfa_required_flag_roundtrips() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        assert!(!store.mfa_required_flag("admin"));
        store.set_mfa_required("admin", true).unwrap();
        assert!(store.mfa_required_flag("admin"));
    }

    #[test]
    fn snapshot_and_replace_roundtrip_preserving_mfa() {
        let (_dir, node) = store();
        node.create_admin("admin", "hunter2hunter2").unwrap();
        let secret = mfa::generate_secret();
        let codes = mfa::generate_recovery_codes();
        node.begin_mfa("admin", &secret, &codes).unwrap();
        node.confirm_mfa("admin").unwrap();

        let snap = node.snapshot();
        assert_eq!(snap.len(), 1);
        assert!(snap[0].mfa.as_ref().is_some_and(|m| m.confirmed));

        // A peer's set: its own admin plus the MFA-enrolled admin above. Both
        // survive the replace, and the synced MFA record stays confirmed.
        let (_dir2, other) = store();
        other.create_admin("boss", "hunter2hunter2").unwrap();
        let mut incoming = other.snapshot();
        incoming.extend(snap.clone());
        node.replace(incoming).unwrap();
        assert!(node.exists("boss"));
        assert!(
            node.mfa_enabled("admin"),
            "a synced, confirmed MFA record survives a replace"
        );
    }

    #[test]
    fn replace_without_an_admin_is_refused() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        let viewer = User {
            username: "bob".into(),
            password_hash: password::hash("viewerpass12").unwrap(),
            role: Role::Viewer,
            mfa: None,
            mfa_required: false,
        };
        // An incoming set with no administrator is rejected and the current
        // admin is left untouched, so a bad merge can never lock the box out.
        assert!(matches!(
            store.replace(vec![viewer]),
            Err(AuthError::LastAdmin)
        ));
        assert_eq!(store.role_of("admin"), Some(Role::Admin));
    }

    #[test]
    fn second_admin_via_setup_is_refused() {
        let (_dir, store) = store();
        store.create_admin("admin", "hunter2hunter2").unwrap();
        assert!(matches!(
            store.create_admin("eve", "hunter2hunter2"),
            Err(AuthError::AlreadyConfigured)
        ));
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
