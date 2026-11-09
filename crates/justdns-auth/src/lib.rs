// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Authentication: the admin account and login sessions.
//!
//! Slice 3 is a single administrator. The account is stored as Argon2 password
//! hashes in a JSON file next to the configuration, created on first run when
//! nobody has set one up yet. Logins are tracked in memory as opaque session
//! tokens, so a restart simply asks everyone to sign in again — there is no
//! long-lived secret on disk beyond the password hash itself.

pub mod mfa;
mod password;
mod role;
mod session;
mod store;

pub use role::Role;
pub use session::{DEFAULT_SESSION_TTL, SessionState, SessionStore};
pub use store::{AuthError, Mfa, User, UserInfo, UserStore};
