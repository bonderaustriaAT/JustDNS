// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! In-memory login sessions keyed by an opaque random token.
//!
//! Tokens are high-entropy random strings handed to the client in an
//! HttpOnly cookie; the server keeps only the mapping from token to user, the
//! session's [`SessionState`] and an expiry. Nothing is persisted, so a restart
//! clears every session.
//!
//! A login with a second factor is two steps: the password check mints a
//! *pending* session (short-lived, grants no access) in state [`SessionState::PendingMfa`]
//! or [`SessionState::PendingEnroll`]; once the user proves or sets up their
//! second factor the session is [`promote`](SessionStore::promote)d to
//! [`SessionState::Authenticated`] with the full lifetime. The auth guard admits
//! only authenticated sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::{OsRng, RngCore};

/// How long a session stays valid without re-login: one week.
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 7);

/// How long a half-finished login (password accepted, second factor still
/// outstanding) stays usable before the user must start over: ten minutes.
pub const PENDING_SESSION_TTL: Duration = Duration::from_secs(60 * 10);

/// Number of random bytes behind a session token (256 bits).
const TOKEN_BYTES: usize = 32;

/// Where a session is in the login flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// Fully signed in — the only state the auth guard admits.
    Authenticated,
    /// Password accepted; the user must still present a TOTP or recovery code.
    PendingMfa,
    /// Password accepted; MFA is required for this user but not yet set up, so
    /// the user must enrol before gaining access.
    PendingEnroll,
}

struct Session {
    username: String,
    state: SessionState,
    expires_at: Instant,
}

/// A cloneable handle to the live session table.
#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Mutex<HashMap<String, Session>>>,
    ttl: Duration,
}

impl SessionStore {
    /// A fresh, empty store whose authenticated sessions live for `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    /// Start a fully authenticated session for `username`, returning its token.
    /// Used when no second factor is needed (or right after one is set up at
    /// first-run setup).
    pub fn create(&self, username: &str) -> String {
        self.insert(username, SessionState::Authenticated, self.ttl)
    }

    /// Start a half-finished session in a pending `state`, returning its token.
    /// It grants no access and expires quickly if the second step is abandoned.
    pub fn create_pending(&self, username: &str, state: SessionState) -> String {
        self.insert(username, state, PENDING_SESSION_TTL)
    }

    /// Finish a pending login: mark the session authenticated and extend it to
    /// the full lifetime. Returns the username on success, or `None` if the
    /// token is unknown or expired. A session that is already authenticated is
    /// left as it is and its username returned.
    pub fn promote(&self, token: &str) -> Option<String> {
        let mut sessions = self.inner.lock().expect("session lock poisoned");
        let session = sessions.get_mut(token)?;
        if session.expires_at <= Instant::now() {
            sessions.remove(token);
            return None;
        }
        session.state = SessionState::Authenticated;
        session.expires_at = Instant::now() + self.ttl;
        Some(session.username.clone())
    }

    /// The user behind a token, but only for a live, fully authenticated
    /// session — the check the auth guard runs. A pending or expired token
    /// yields `None` (an expired one is dropped on the way out).
    pub fn username_for(&self, token: &str) -> Option<String> {
        match self.lookup(token) {
            Some((username, SessionState::Authenticated)) => Some(username),
            _ => None,
        }
    }

    /// The user and state behind a token for any live session, pending included
    /// — used by the second-step login endpoints, which must act on a session
    /// the auth guard would reject. An expired token is dropped and yields
    /// `None`.
    pub fn lookup(&self, token: &str) -> Option<(String, SessionState)> {
        let mut sessions = self.inner.lock().expect("session lock poisoned");
        let session = sessions.get(token)?;
        if session.expires_at <= Instant::now() {
            sessions.remove(token);
            return None;
        }
        Some((session.username.clone(), session.state))
    }

    /// End a session, if the token names one. Logging out twice is harmless.
    pub fn remove(&self, token: &str) {
        self.inner
            .lock()
            .expect("session lock poisoned")
            .remove(token);
    }

    fn insert(&self, username: &str, state: SessionState, ttl: Duration) -> String {
        let token = random_token();
        let session = Session {
            username: username.to_owned(),
            state,
            expires_at: Instant::now() + ttl,
        };
        self.inner
            .lock()
            .expect("session lock poisoned")
            .insert(token.clone(), session);
        token
    }
}

/// A URL-safe random token: `TOKEN_BYTES` of OS randomness, hex-encoded.
fn random_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_resolve() {
        let store = SessionStore::new(DEFAULT_SESSION_TTL);
        let token = store.create("admin");
        assert_eq!(store.username_for(&token).as_deref(), Some("admin"));
        assert_eq!(store.username_for("nonsense"), None);
    }

    #[test]
    fn remove_ends_the_session() {
        let store = SessionStore::new(DEFAULT_SESSION_TTL);
        let token = store.create("admin");
        store.remove(&token);
        assert_eq!(store.username_for(&token), None);
    }

    #[test]
    fn expired_tokens_are_rejected_and_dropped() {
        let store = SessionStore::new(Duration::from_millis(0));
        let token = store.create("admin");
        assert_eq!(store.username_for(&token), None);
        assert_eq!(store.username_for(&token), None);
    }

    #[test]
    fn tokens_are_unique_and_long() {
        let store = SessionStore::new(DEFAULT_SESSION_TTL);
        let a = store.create("admin");
        let b = store.create("admin");
        assert_ne!(a, b);
        assert_eq!(a.len(), TOKEN_BYTES * 2);
    }

    #[test]
    fn pending_session_is_not_authenticated_until_promoted() {
        let store = SessionStore::new(DEFAULT_SESSION_TTL);
        let token = store.create_pending("admin", SessionState::PendingMfa);
        // The guard rejects a pending session.
        assert_eq!(store.username_for(&token), None);
        // But the login endpoints can see it and its state.
        assert_eq!(
            store.lookup(&token),
            Some(("admin".to_owned(), SessionState::PendingMfa))
        );
        // Promotion makes it a full session.
        assert_eq!(store.promote(&token).as_deref(), Some("admin"));
        assert_eq!(store.username_for(&token).as_deref(), Some("admin"));
    }

    #[test]
    fn promote_rejects_unknown_token() {
        let store = SessionStore::new(DEFAULT_SESSION_TTL);
        assert_eq!(store.promote("nope"), None);
    }

    #[test]
    fn enroll_pending_state_roundtrips() {
        let store = SessionStore::new(DEFAULT_SESSION_TTL);
        let token = store.create_pending("admin", SessionState::PendingEnroll);
        assert_eq!(
            store.lookup(&token).map(|(_, s)| s),
            Some(SessionState::PendingEnroll)
        );
    }
}
