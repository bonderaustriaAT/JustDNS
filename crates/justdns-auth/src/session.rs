// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! In-memory login sessions keyed by an opaque random token.
//!
//! Tokens are high-entropy random strings handed to the client in an
//! HttpOnly cookie; the server keeps only the mapping from token to user and an
//! expiry. Nothing is persisted, so a restart clears every session — a fair
//! trade for not storing another long-lived secret on disk.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::{OsRng, RngCore};

/// How long a session stays valid without re-login: one week.
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 7);

/// Number of random bytes behind a session token (256 bits).
const TOKEN_BYTES: usize = 32;

struct Session {
    username: String,
    expires_at: Instant,
}

/// A cloneable handle to the live session table.
#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Mutex<HashMap<String, Session>>>,
    ttl: Duration,
}

impl SessionStore {
    /// A fresh, empty store whose sessions live for `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    /// Start a session for `username`, returning its token.
    pub fn create(&self, username: &str) -> String {
        let token = random_token();
        let session = Session {
            username: username.to_owned(),
            expires_at: Instant::now() + self.ttl,
        };
        self.inner
            .lock()
            .expect("session lock poisoned")
            .insert(token.clone(), session);
        token
    }

    /// The user a token belongs to, or `None` if it is unknown or expired. An
    /// expired token is dropped on the way out.
    pub fn username_for(&self, token: &str) -> Option<String> {
        let mut sessions = self.inner.lock().expect("session lock poisoned");
        let session = sessions.get(token)?;
        if session.expires_at <= Instant::now() {
            sessions.remove(token);
            return None;
        }
        Some(session.username.clone())
    }

    /// End a session, if the token names one. Logging out twice is harmless.
    pub fn remove(&self, token: &str) {
        self.inner
            .lock()
            .expect("session lock poisoned")
            .remove(token);
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
        // TTL of zero means the session is already past its expiry.
        assert_eq!(store.username_for(&token), None);
        // And it was removed, so a second look is also empty.
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
}
