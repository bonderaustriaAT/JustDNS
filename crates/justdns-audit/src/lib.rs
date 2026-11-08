// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An append-only audit log of security-relevant events.
//!
//! Logins, sign-outs and administrative changes to accounts, roles, MFA and
//! settings are recorded with a UTC timestamp, the actor (`system` for an
//! unauthenticated event), a machine-readable action, an optional target and
//! detail — and **never a secret**. At most [`MAX_ENTRIES`] recent events are kept
//! in memory and on disk, as JSON Lines rewritten atomically on each change.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// How many of the most recent entries are retained, in memory and on disk.
pub const MAX_ENTRIES: usize = 2000;

/// Whether a recorded action succeeded or was refused. Serialised lowercase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
}

/// One line in the audit log. Field names are camelCase to match the dashboard's
/// JSON. `target` and `detail` are omitted from the wire when absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// When the event happened, RFC 3339 in UTC (e.g. `2026-11-06T10:15:30Z`).
    pub time: String,
    /// Who caused it: a signed-in username, or `system` for an event with no
    /// authenticated actor (such as a failed login).
    pub actor: String,
    /// What happened, as a stable machine key (e.g. `login.success`,
    /// `user.create`, `mfa.enable`). The UI maps these to translated labels.
    pub action: String,
    /// The object acted upon, often a username.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub outcome: Outcome,
    /// A short, non-secret note for context (e.g. `role=editor`), when useful.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AuditEntry {
    /// Start an entry stamped with the current UTC time.
    pub fn new(actor: impl Into<String>, action: impl Into<String>, outcome: Outcome) -> Self {
        Self {
            time: now_rfc3339(),
            actor: actor.into(),
            action: action.into(),
            target: None,
            outcome,
            detail: None,
        }
    }

    pub fn success(actor: impl Into<String>, action: impl Into<String>) -> Self {
        Self::new(actor, action, Outcome::Success)
    }

    pub fn failure(actor: impl Into<String>, action: impl Into<String>) -> Self {
        Self::new(actor, action, Outcome::Failure)
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Add a short, non-secret context note.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Errors from loading or persisting the audit log.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("audit io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("audit serialise: {0}")]
    Serialise(#[source] serde_json::Error),
}

struct Inner {
    path: PathBuf,
    entries: Mutex<VecDeque<AuditEntry>>,
}

/// A cheap, cloneable handle to the append-only audit log.
#[derive(Clone)]
pub struct AuditStore {
    inner: Arc<Inner>,
}

impl AuditStore {
    /// Load the log from `path`. A missing file starts empty; unparseable lines are
    /// skipped and only the most recent [`MAX_ENTRIES`] are kept.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, AuditError> {
        let path = path.into();
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut parsed: VecDeque<AuditEntry> = text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .filter_map(|line| serde_json::from_str::<AuditEntry>(line).ok())
                    .collect();
                while parsed.len() > MAX_ENTRIES {
                    parsed.pop_front();
                }
                parsed
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => VecDeque::new(),
            Err(source) => return Err(AuditError::Io { path, source }),
        };

        Ok(Self {
            inner: Arc::new(Inner {
                path,
                entries: Mutex::new(entries),
            }),
        })
    }

    /// Append an event and persist. The in-memory log is updated before the disk
    /// write, and entries beyond [`MAX_ENTRIES`] are dropped.
    pub fn record(&self, entry: AuditEntry) -> Result<(), AuditError> {
        let mut entries = self.inner.entries.lock().expect("audit lock poisoned");
        entries.push_back(entry);
        while entries.len() > MAX_ENTRIES {
            entries.pop_front();
        }
        persist(&self.inner.path, &entries)
    }

    /// The most recent entries, newest first, capped at `limit`.
    pub fn recent(&self, limit: usize) -> Vec<AuditEntry> {
        let entries = self.inner.entries.lock().expect("audit lock poisoned");
        entries.iter().rev().take(limit).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner
            .entries
            .lock()
            .expect("audit lock poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The current time as an RFC 3339 string in UTC, to the second.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Rewrite the whole log to `path` atomically: serialise every entry as one JSON
/// line to a sibling temp file, then rename over the target.
fn persist(path: &Path, entries: &VecDeque<AuditEntry>) -> Result<(), AuditError> {
    let io_err = |source| AuditError::Io {
        path: path.to_path_buf(),
        source,
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
    }

    let mut buf = String::with_capacity(entries.len() * 128);
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(AuditError::Serialise)?;
        buf.push_str(&line);
        buf.push('\n');
    }

    let tmp = path.with_extension("log.tmp");
    std::fs::write(&tmp, buf.as_bytes()).map_err(|source| AuditError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(io_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        (dir, path)
    }

    #[test]
    fn empty_without_a_file_and_writes_nothing() {
        let (_dir, path) = temp();
        let store = AuditStore::load(&path).unwrap();
        assert!(store.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn records_persist_and_reload_newest_first() {
        let (_dir, path) = temp();
        let store = AuditStore::load(&path).unwrap();
        store
            .record(AuditEntry::success("admin", "login.success"))
            .unwrap();
        store
            .record(
                AuditEntry::success("admin", "user.create")
                    .target("bob")
                    .detail("role=viewer"),
            )
            .unwrap();

        let recent = store.recent(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].action, "user.create", "newest first");
        assert_eq!(recent[0].target.as_deref(), Some("bob"));

        let reopened = AuditStore::load(&path).unwrap();
        assert_eq!(reopened.len(), 2);
        assert_eq!(reopened.recent(1)[0].action, "user.create");
    }

    #[test]
    fn a_secret_is_never_written_because_nothing_carries_one() {
        // The entry shape exposes only actor/action/target/detail — no field for a secret.
        let (_dir, path) = temp();
        let store = AuditStore::load(&path).unwrap();
        store
            .record(AuditEntry::failure("system", "login.failure").target("admin"))
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"action\":\"login.failure\""));
        assert!(text.contains("\"outcome\":\"failure\""));
    }

    #[test]
    fn log_is_bounded_to_max_entries() {
        let (_dir, path) = temp();
        let store = AuditStore::load(&path).unwrap();
        for i in 0..(MAX_ENTRIES + 50) {
            store
                .record(AuditEntry::success("admin", "noise").detail(i.to_string()))
                .unwrap();
        }
        assert_eq!(store.len(), MAX_ENTRIES);
        let newest = &store.recent(1)[0];
        assert_eq!(
            newest.detail.as_deref(),
            Some((MAX_ENTRIES + 49).to_string().as_str())
        );

        let reopened = AuditStore::load(&path).unwrap();
        assert_eq!(reopened.len(), MAX_ENTRIES, "bound survives reload");
    }

    #[test]
    fn unparseable_lines_are_skipped_on_load() {
        let (_dir, path) = temp();
        let good = serde_json::to_string(&AuditEntry::success("admin", "login.success")).unwrap();
        std::fs::write(&path, format!("garbage not json\n{good}\n\n")).unwrap();
        let store = AuditStore::load(&path).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.recent(1)[0].action, "login.success");
    }

    #[test]
    fn target_and_detail_are_omitted_when_absent() {
        let json = serde_json::to_string(&AuditEntry::success("admin", "logout")).unwrap();
        assert!(!json.contains("target"), "{json}");
        assert!(!json.contains("detail"), "{json}");
    }
}
