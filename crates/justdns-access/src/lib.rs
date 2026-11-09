// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Access requests: the "ask to unblock this page" queue.
//!
//! Each ask is an [`AccessRequest`] — domain, client, the device/group it
//! resolves to, what blocked it, and an optional reason — keyed by a deterministic
//! id from client + domain, so re-asking refreshes the one entry. [`AccessStore`]
//! persists the queue to a JSON file atomically and runs an on-change callback
//! after every change.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// A callback run after every persisted change.
type OnChange = Arc<dyn Fn() + Send + Sync>;

/// The longest reason text kept, in characters.
const MAX_REASON_CHARS: usize = 280;

/// Errors from loading, saving or editing the access-request queue.
#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("access-requests io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("access-requests parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("an access request needs a domain")]
    InvalidDomain,
    #[error("no access request with id '{0}'")]
    Unknown(String),
}

/// One visitor's ask to unblock a domain, serialised camelCase for the UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessRequest {
    /// Deterministic id from client + domain. Assigned by the store; any supplied
    /// value is replaced.
    #[serde(default)]
    pub id: String,
    /// The unblock target, lowercased with trailing dot stripped.
    pub domain: String,
    pub client_ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// What blocked it: `"ads"`, `"malware"` or `"custom"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_category: Option<String>,
    /// For a custom hit, the list or feed name that matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_list: Option<String>,
    /// The matched rule: the queried name or a parent domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_rule: Option<String>,
    /// Optional visitor reason, clamped to [`MAX_REASON_CHARS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Most recent ask, ms since the Unix epoch.
    pub created_ms: i64,
}

/// The persisted shape and in-memory state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredRequests {
    requests: Vec<AccessRequest>,
}

struct Inner {
    path: PathBuf,
    state: RwLock<StoredRequests>,
    on_change: RwLock<Option<OnChange>>,
}

/// A cheap, cloneable handle to the persistent access-request queue.
#[derive(Clone)]
pub struct AccessStore {
    inner: Arc<Inner>,
}

impl AccessStore {
    /// Load the queue from `path`. A missing file starts empty and is written.
    /// Entries are normalised on load (blank or id-less rows are repaired).
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, AccessError> {
        let path = path.into();
        let stored: StoredRequests = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| AccessError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let seed = StoredRequests::default();
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(AccessError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };
        let state = clean_state(stored);
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                state: RwLock::new(state),
                on_change: RwLock::new(None),
            }),
        })
    }

    /// Register the after-change callback, replacing any previous one.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self
            .inner
            .on_change
            .write()
            .expect("access-requests lock poisoned") = Some(Arc::new(callback));
    }

    fn state(&self) -> std::sync::RwLockReadGuard<'_, StoredRequests> {
        self.inner
            .state
            .read()
            .expect("access-requests lock poisoned")
    }

    /// Every pending request, newest first.
    pub fn view(&self) -> Vec<AccessRequest> {
        let mut requests = self.state().requests.clone();
        requests.sort_by_key(|request| std::cmp::Reverse(request.created_ms));
        requests
    }

    /// Every pending request in stored order (no re-sort).
    pub fn snapshot(&self) -> Vec<AccessRequest> {
        self.state().requests.clone()
    }

    pub fn count(&self) -> usize {
        self.state().requests.len()
    }

    pub fn get(&self, id: &str) -> Option<AccessRequest> {
        self.state().requests.iter().find(|r| r.id == id).cloned()
    }

    /// Record an ask. The id is derived from client + domain, so re-asking
    /// refreshes the one entry with the latest reason and time.
    pub fn record(&self, req: AccessRequest) -> Result<AccessRequest, AccessError> {
        let clean = clean_request(req)?;
        self.mutate(|state| {
            match state.requests.iter_mut().find(|r| r.id == clean.id) {
                Some(slot) => *slot = clean.clone(),
                None => state.requests.push(clean.clone()),
            }
            Ok(clean)
        })
    }

    /// Remove the request with `id`; an unknown id is reported as an error.
    pub fn remove(&self, id: &str) -> Result<(), AccessError> {
        self.mutate(|state| {
            let before = state.requests.len();
            state.requests.retain(|r| r.id != id);
            if state.requests.len() == before {
                return Err(AccessError::Unknown(id.to_owned()));
            }
            Ok(())
        })
    }

    /// Replace the whole queue (cluster merge or restore). Entries are normalised;
    /// an id-less row is repaired, not rejected.
    pub fn replace(&self, requests: Vec<AccessRequest>) -> Result<(), AccessError> {
        let clean = clean_state(StoredRequests { requests });
        self.mutate(|state| {
            *state = clean;
            Ok(())
        })
    }

    /// Apply `change` under the write lock, persist the result, then (after
    /// releasing the lock) run the on-change callback. A failing mutation leaves
    /// the file and the callback untouched.
    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut StoredRequests) -> Result<T, AccessError>,
    ) -> Result<T, AccessError> {
        let result = {
            let mut state = self
                .inner
                .state
                .write()
                .expect("access-requests lock poisoned");
            let value = change(&mut state)?;
            write_atomic(&self.inner.path, &state)?;
            value
        };
        self.fire_on_change();
        Ok(result)
    }

    fn fire_on_change(&self) {
        let callback = self
            .inner
            .on_change
            .read()
            .expect("access-requests lock poisoned")
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }
}

/// Normalise and validate a request, assigning its deterministic id: domain
/// lowercased and dot-stripped, reason trimmed and clamped, blank options to `None`.
fn clean_request(mut req: AccessRequest) -> Result<AccessRequest, AccessError> {
    let domain = req.domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return Err(AccessError::InvalidDomain);
    }
    let client_ip = req.client_ip.trim().to_owned();
    req.id = make_id(&client_ip, &domain);
    req.domain = domain;
    req.client_ip = client_ip;
    req.device = none_if_blank(req.device);
    req.group = none_if_blank(req.group);
    req.block_category = none_if_blank(req.block_category);
    req.block_list = none_if_blank(req.block_list);
    req.block_rule = none_if_blank(req.block_rule);
    req.reason = none_if_blank(req.reason).map(|reason| clamp_chars(&reason, MAX_REASON_CHARS));
    Ok(req)
}

/// A deterministic, URL-safe id from client + domain; non-alphanumerics become dashes.
fn make_id(client_ip: &str, domain: &str) -> String {
    let mut id = String::with_capacity(client_ip.len() + domain.len() + 2);
    push_slug(&mut id, client_ip);
    id.push_str("--");
    push_slug(&mut id, domain);
    id
}

/// Append `text` to `id`, lowercasing alphanumerics and turning the rest into dashes.
fn push_slug(id: &mut String, text: &str) {
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch.to_ascii_lowercase());
        } else {
            id.push('-');
        }
    }
}

fn none_if_blank(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// Clamp `text` to at most `max` characters (not bytes), trimming trailing space.
fn clamp_chars(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_owned();
    }
    trimmed
        .chars()
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Clean the state from a load or a merge: normalise every entry (repairing a
/// blank id) and drop duplicate ids, newest kept.
fn clean_state(stored: StoredRequests) -> StoredRequests {
    let mut requests: Vec<AccessRequest> = Vec::with_capacity(stored.requests.len());
    for req in stored.requests {
        let Ok(clean) = clean_request(req) else {
            continue;
        };
        match requests.iter_mut().find(|r| r.id == clean.id) {
            Some(existing) => {
                if clean.created_ms >= existing.created_ms {
                    *existing = clean;
                }
            }
            None => requests.push(clean),
        }
    }
    StoredRequests { requests }
}

/// Write `stored` to `path` as pretty JSON, atomically (temp file then rename),
/// creating the parent directory if needed.
fn write_atomic(path: &Path, stored: &StoredRequests) -> Result<(), AccessError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| AccessError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let json = serde_json::to_vec_pretty(stored).map_err(|source| AccessError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| AccessError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| AccessError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, AccessStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = AccessStore::load(dir.path().join("access.json")).unwrap();
        (dir, store)
    }

    fn req(ip: &str, domain: &str, reason: Option<&str>, created_ms: i64) -> AccessRequest {
        AccessRequest {
            id: String::new(),
            domain: domain.to_owned(),
            client_ip: ip.to_owned(),
            device: Some("Laptop".to_owned()),
            group: Some("Kinder".to_owned()),
            block_category: Some("custom".to_owned()),
            block_list: Some("Hagezi Pro".to_owned()),
            block_rule: Some(domain.to_owned()),
            reason: reason.map(str::to_owned),
            created_ms,
        }
    }

    #[test]
    fn record_assigns_deterministic_id_and_persists() {
        let (dir, store) = store();
        let a = store
            .record(req("192.168.20.30", "ads.example.com", Some("need it"), 10))
            .unwrap();
        assert_eq!(a.id, "192-168-20-30--ads-example-com");
        assert_eq!(a.domain, "ads.example.com");

        let reopened = AccessStore::load(dir.path().join("access.json")).unwrap();
        assert_eq!(reopened.view().len(), 1);
        assert_eq!(
            reopened.get(&a.id).unwrap().reason.as_deref(),
            Some("need it")
        );
    }

    #[test]
    fn re_asking_refreshes_one_entry() {
        let (_dir, store) = store();
        store
            .record(req("10.0.0.5", "Example.com.", None, 10))
            .unwrap();
        let second = store
            .record(req("10.0.0.5", "example.com", Some("please"), 20))
            .unwrap();
        assert_eq!(store.view().len(), 1, "same client+domain dedupes");
        assert_eq!(second.reason.as_deref(), Some("please"));
        assert_eq!(second.created_ms, 20);
    }

    #[test]
    fn different_clients_make_separate_entries() {
        let (_dir, store) = store();
        store.record(req("10.0.0.5", "a.test", None, 1)).unwrap();
        store.record(req("10.0.0.6", "a.test", None, 2)).unwrap();
        assert_eq!(store.view().len(), 2);
        assert_eq!(store.count(), 2);
    }

    #[test]
    fn view_is_newest_first() {
        let (_dir, store) = store();
        store.record(req("10.0.0.5", "old.test", None, 1)).unwrap();
        store.record(req("10.0.0.6", "new.test", None, 9)).unwrap();
        let view = store.view();
        assert_eq!(view[0].domain, "new.test");
        assert_eq!(view[1].domain, "old.test");
    }

    #[test]
    fn remove_drops_it_and_reports_unknown() {
        let (_dir, store) = store();
        let a = store.record(req("10.0.0.5", "a.test", None, 1)).unwrap();
        store.remove(&a.id).unwrap();
        assert!(store.view().is_empty());
        assert!(matches!(store.remove(&a.id), Err(AccessError::Unknown(_))));
    }

    #[test]
    fn blank_domain_is_rejected() {
        let (_dir, store) = store();
        assert!(matches!(
            store.record(req("10.0.0.5", "   ", None, 1)),
            Err(AccessError::InvalidDomain)
        ));
    }

    #[test]
    fn reason_is_clamped() {
        let (_dir, store) = store();
        let long = "x".repeat(500);
        let stored = store
            .record(req("10.0.0.5", "a.test", Some(&long), 1))
            .unwrap();
        assert_eq!(
            stored.reason.as_deref().unwrap().chars().count(),
            MAX_REASON_CHARS
        );
    }

    #[test]
    fn replace_round_trips_and_repairs_ids() {
        let (_dir, store) = store();
        store.record(req("10.0.0.5", "a.test", None, 1)).unwrap();
        let snap = store.snapshot();
        store.replace(Vec::new()).unwrap();
        assert!(store.view().is_empty());
        store.replace(snap).unwrap();
        assert_eq!(store.view().len(), 1);
    }

    #[test]
    fn on_change_fires_after_a_mutation() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (_dir, store) = store();
        let hits = Arc::new(AtomicUsize::new(0));
        let sink = hits.clone();
        store.on_change(move || {
            sink.fetch_add(1, Ordering::Relaxed);
        });
        let a = store.record(req("10.0.0.5", "a.test", None, 1)).unwrap();
        store.remove(&a.id).unwrap();
        assert_eq!(hits.load(Ordering::Relaxed), 2, "once per change");
    }
}
