// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The shapes read back from the JustDNS query-log API, and the in-memory
//! aggregation they feed.
//!
//! The harvester only ever *reads* the log over the HTTP API, so these mirror
//! exactly what `GET /api/queries/history` serialises: a flattened
//! [`justdns_store::QueryEvent`] per row (snake_case fields) plus a camelCase
//! `nextBefore` keyset cursor. We deserialise leniently — unknown fields are
//! ignored — so a newer server that adds columns never breaks the poller.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// One query-log row as the API serialises it. Only the fields the selection
/// test needs are named; everything else in the row is ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct LogRow {
    /// Wall-clock finish time, milliseconds since the Unix epoch.
    pub at_ms: i64,
    /// The client that asked, as text (an IP address).
    #[serde(default)]
    pub client: String,
    /// The queried name, in its textual form (may carry a trailing dot).
    pub name: String,
    /// Where the answer came from: `upstream`, `cache`, `blocked`, …
    #[serde(default)]
    pub source: String,
}

/// The keyset cursor the explorer returns, mirroring the store's `(at_ms, row)`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub at_ms: i64,
    pub row: i64,
}

/// One page of history: rows newest-first, and a cursor when more may follow.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    #[serde(default)]
    pub rows: Vec<LogRow>,
    #[serde(default)]
    pub next_before: Option<Cursor>,
}

/// How many distinct client addresses we keep per domain. Enough to judge
/// fan-out (a third-party beacon is reached by many devices) without letting the
/// state file grow without bound on a busy network.
pub const MAX_TRACKED_CLIENTS: usize = 256;

/// How many distinct full hostnames we keep under one registrable root, busiest
/// first when trimmed — enough to suggest the real beacon host instead of the
/// apex, without unbounded growth.
pub const MAX_TRACKED_HOSTS: usize = 32;

/// The rolling aggregate the harvester keeps for one registrable domain across
/// runs. Sets are capped (see the constants above) so the persisted state stays
/// bounded on a busy network.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DomainStat {
    /// Total resolved (non-blocked) lookups seen for this root.
    pub hits: u64,
    /// Distinct client addresses that looked it up (capped).
    pub clients: HashSet<String>,
    /// Distinct UTC day buckets it was seen on (`at_ms / MS_PER_DAY`, capped by
    /// nature — one entry per day).
    pub days: HashSet<i64>,
    /// Full hostnames observed under this root and their hit counts (capped).
    pub hosts: HashMap<String, u64>,
    /// Earliest and latest `at_ms` we have attributed to this root.
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
}

/// Milliseconds in a day, for the day-bucket signal.
pub const MS_PER_DAY: i64 = 86_400_000;

impl DomainStat {
    /// Fold one observed lookup of `host` by `client` at `at_ms` into the aggregate.
    pub fn observe(&mut self, host: &str, client: &str, at_ms: i64) {
        self.hits += 1;
        if self.first_seen_ms == 0 || at_ms < self.first_seen_ms {
            self.first_seen_ms = at_ms;
        }
        if at_ms > self.last_seen_ms {
            self.last_seen_ms = at_ms;
        }
        self.days.insert(at_ms / MS_PER_DAY);
        if !client.is_empty() && self.clients.len() < MAX_TRACKED_CLIENTS {
            self.clients.insert(client.to_owned());
        }
        let counter = self.hosts.entry(host.to_owned()).or_insert(0);
        *counter += 1;
        // Trim only when we exceed the cap, dropping the least-seen host.
        if self.hosts.len() > MAX_TRACKED_HOSTS {
            if let Some(least) = self
                .hosts
                .iter()
                .min_by_key(|(_, count)| **count)
                .map(|(host, _)| host.clone())
            {
                self.hosts.remove(&least);
            }
        }
    }

    /// Distinct devices that reached this root (capped at [`MAX_TRACKED_CLIENTS`]).
    pub fn distinct_clients(&self) -> usize {
        self.clients.len()
    }

    /// Distinct days this root was seen on.
    pub fn days_seen(&self) -> usize {
        self.days.len()
    }

    /// The observed hostnames under this root, busiest first.
    pub fn hosts_by_hits(&self) -> Vec<(String, u64)> {
        let mut hosts: Vec<(String, u64)> =
            self.hosts.iter().map(|(h, c)| (h.clone(), *c)).collect();
        hosts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        hosts
    }
}
