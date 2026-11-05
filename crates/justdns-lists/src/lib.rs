// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! User-managed domain lists: the dashboard's allow and block entries, their
//! subscriptions to external feeds, and a curated catalogue of one-click lists.
//!
//! This crate owns the *management and persistence* of lists; the matching
//! itself lives in [`justdns_filter`]. The [`ListStore`] keeps the user's own
//! entries and subscriptions in a JSON file on the data volume, caches each
//! fetched feed beside it, and merges everything into the live [`Blocker`] so a
//! change takes effect on the next query without a restart.
//!
//! [`Blocker`]: justdns_filter::Blocker

mod presets;
mod store;

pub use presets::{Preset, preset, presets};
pub use store::{
    ListError, ListObject, ListStore, ListsView, PresetView, RefreshOutcome, ResolvedLists,
    Subscription, UserLists,
};

use serde::{Deserialize, Serialize};

/// Whether a list adds names to the allow set or the block set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListKind {
    /// Names that must never be blocked (an allow list wins over every block).
    Allow,
    /// Names to block.
    Block,
}
