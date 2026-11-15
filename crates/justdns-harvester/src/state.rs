// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The harvester's own durable state — a single JSON file, kept separate from
//! JustDNS's DuckDB log (which is single-writer locked and must not be touched).
//!
//! It accumulates across runs so trends build over days: per registrable root the
//! rolling [`DomainStat`], the keyset the poller resumes each source from, and the
//! set of names seen blocked on the instance (an extra hard-exclusion the static
//! coverage set cannot know, e.g. the user's own manual block list).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::coverage::{SuffixSet, registrable_root};
use crate::model::DomainStat;

/// The persisted harvester state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Newest `at_ms` already ingested from the `upstream` feed, so the next poll
    /// only pulls rows strictly newer.
    pub last_upstream_ms: i64,
    /// Newest `at_ms` already ingested from the `blocked` feed.
    pub last_blocked_ms: i64,
    /// Rolling per-root aggregate of resolved (non-blocked) lookups.
    pub domains: HashMap<String, DomainStat>,
    /// Registrable roots observed blocked on the instance — a hard exclusion.
    pub blocked_roots: HashSet<String>,
}

impl State {
    /// Load state from `path`, or a fresh default if the file is missing. A
    /// corrupt file is an error the caller can decide to reset on.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(StateError::Io(error)),
        }
    }

    /// Persist state to `path` atomically (write a temp file, then rename).
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let json = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Fold one resolved lookup into the per-root aggregate.
    pub fn observe_resolved(&mut self, host: &str, client: &str, at_ms: i64) {
        let root = registrable_root(host);
        if root.is_empty() {
            return;
        }
        self.domains
            .entry(root)
            .or_default()
            .observe(host, client, at_ms);
        if at_ms > self.last_upstream_ms {
            self.last_upstream_ms = at_ms;
        }
    }

    /// Note one blocked lookup: record its root as an exclusion.
    pub fn observe_blocked(&mut self, host: &str, at_ms: i64) {
        let root = registrable_root(host);
        if !root.is_empty() {
            self.blocked_roots.insert(root);
        }
        if at_ms > self.last_blocked_ms {
            self.last_blocked_ms = at_ms;
        }
    }

    /// The blocked roots as a suffix set for the selection test.
    pub fn blocked_suffix_set(&self) -> SuffixSet {
        let mut set = SuffixSet::new();
        set.add_list(
            &self
                .blocked_roots
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        );
        set
    }
}

/// Errors reading or writing the state file.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("state io: {0}")]
    Io(#[from] std::io::Error),
    #[error("state json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");

        let mut state = State::default();
        state.observe_resolved("code.etracker.com", "10.0.0.2", 1000);
        state.observe_resolved("code.etracker.com", "10.0.0.3", 2000);
        state.observe_blocked("ads.doubleclick.net", 1500);
        state.save(&path).unwrap();

        let back = State::load(&path).unwrap();
        assert_eq!(back.last_upstream_ms, 2000);
        assert_eq!(back.last_blocked_ms, 1500);
        assert_eq!(back.domains.get("etracker.com").unwrap().hits, 2);
        assert!(back.blocked_suffix_set().covers("x.doubleclick.net"));
    }

    #[test]
    fn missing_file_is_a_fresh_state() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::load(&dir.path().join("none.json")).unwrap();
        assert_eq!(state.last_upstream_ms, 0);
        assert!(state.domains.is_empty());
    }
}
