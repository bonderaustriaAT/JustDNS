// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Account roles: what a signed-in user is allowed to do.
//!
//! Three tiers, from most to least powerful. The ordering is meaningful — a
//! higher role can do everything a lower one can — so [`Role`] implements `Ord`
//! with `Admin > Editor > Viewer`, and the web layer gates each endpoint on a
//! *minimum* role.

use serde::{Deserialize, Serialize};

/// What a signed-in user may do. Serialised lowercase to match the dashboard's
/// JSON (`"admin"`, `"editor"`, `"viewer"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Read-only: may view everything but change nothing.
    Viewer,
    /// May change filtering content (lists, rewrites, devices, groups,
    /// schedules, cache) but not settings, users, TLS or backups.
    Editor,
    /// Full control, including user management and server settings.
    Admin,
}

impl Role {
    /// Whether this role is at least as powerful as `needed` — the check the web
    /// layer runs for every guarded endpoint.
    pub fn at_least(self, needed: Role) -> bool {
        self >= needed
    }

    /// The wire string for this role, for logs and JSON built by hand.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Editor => "editor",
            Role::Admin => "admin",
        }
    }
}

/// The default role for an account whose stored record predates roles: a lone
/// administrator from before this feature keeps full control (`Admin`), so an
/// existing `auth.json` loads with no change in access.
pub(crate) fn default_admin() -> Role {
    Role::Admin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_admin_highest() {
        assert!(Role::Admin > Role::Editor);
        assert!(Role::Editor > Role::Viewer);
        assert!(Role::Admin.at_least(Role::Viewer));
        assert!(Role::Editor.at_least(Role::Editor));
        assert!(!Role::Viewer.at_least(Role::Editor));
        assert!(!Role::Editor.at_least(Role::Admin));
    }

    #[test]
    fn serialises_lowercase() {
        assert_eq!(serde_json::to_string(&Role::Admin).unwrap(), "\"admin\"");
        assert_eq!(
            serde_json::from_str::<Role>("\"viewer\"").unwrap(),
            Role::Viewer
        );
    }
}
