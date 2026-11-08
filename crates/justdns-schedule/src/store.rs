// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persistent schedule store: the user's time windows, kept in a JSON file on
//! the data volume.
//!
//! Mirrors [`ClientStore`](justdns_clients::ClientStore): every edit validates and
//! normalises the entry, rewrites the file atomically, then runs an on-change
//! callback so the binary rebuilds the live policies. The store validates a
//! schedule's shape (name, times, weekdays, target form) but not whether its
//! target group or device still exists — that is the web layer's job, where the
//! catalogue is known; the policy coordinator simply ignores a schedule whose
//! target has gone.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use justdns_clients::{parse_target, valid_device_name};
use serde::{Deserialize, Serialize};

use crate::{Day, Schedule, ScheduleEffect, ScheduleTarget, TargetScope, valid_hhmm};

/// A callback run after every change that persists, so the binary can rebuild the
/// live per-client policies when schedules change.
type OnChange = Arc<dyn Fn() + Send + Sync>;

/// Errors from loading, saving or editing schedules.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("schedules io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("schedules parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("a schedule needs a name")]
    InvalidName,
    #[error("'{0}' is not a valid HH:MM time")]
    InvalidTime(String),
    #[error("a schedule needs at least one weekday")]
    InvalidDays,
    #[error("a group or device schedule needs a target id")]
    InvalidTarget,
    #[error("no schedule with id '{0}'")]
    Unknown(String),
}

/// Everything needed to create or replace a schedule, without its id (which the
/// store assigns on creation and keeps across edits). Deserialised directly from
/// the dashboard's POST/PUT body.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleDraft {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub days: Vec<Day>,
    pub start: String,
    pub end: String,
    pub target: ScheduleTarget,
    pub effect: ScheduleEffect,
}

fn default_true() -> bool {
    true
}

impl From<Schedule> for ScheduleDraft {
    fn from(schedule: Schedule) -> Self {
        Self {
            name: schedule.name,
            enabled: schedule.enabled,
            days: schedule.days,
            start: schedule.start,
            end: schedule.end,
            target: schedule.target,
            effect: schedule.effect,
        }
    }
}

/// The persisted shape and in-memory state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredSchedules {
    schedules: Vec<Schedule>,
}

struct Inner {
    path: PathBuf,
    state: RwLock<StoredSchedules>,
    on_change: RwLock<Option<OnChange>>,
}

/// A cheap, cloneable handle to the persistent schedules.
#[derive(Clone)]
pub struct ScheduleStore {
    inner: Arc<Inner>,
}

impl ScheduleStore {
    /// Load the schedules from `path`. A missing file starts empty and is written
    /// on first run. Entries are re-validated on load, so a hand-edited file with
    /// a bad time or an empty weekday set is rejected.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, ScheduleError> {
        let path = path.into();
        let stored: StoredSchedules = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| ScheduleError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let seed = StoredSchedules::default();
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(ScheduleError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };
        let state = clean_state(stored)?;
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                state: RwLock::new(state),
                on_change: RwLock::new(None),
            }),
        })
    }

    /// Register the callback run after every change that persists. Replaces any
    /// previous callback.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self
            .inner
            .on_change
            .write()
            .expect("schedules lock poisoned") = Some(Arc::new(callback));
    }

    fn state(&self) -> std::sync::RwLockReadGuard<'_, StoredSchedules> {
        self.inner.state.read().expect("schedules lock poisoned")
    }

    /// Every schedule, for the dashboard and for the policy rebuild.
    pub fn view(&self) -> Vec<Schedule> {
        self.state().schedules.clone()
    }

    /// A snapshot of every schedule, for a backup export.
    pub fn snapshot(&self) -> Vec<Schedule> {
        self.state().schedules.clone()
    }

    /// Create a schedule from a draft, assigning a generated, unique id.
    pub fn add(&self, draft: ScheduleDraft) -> Result<Schedule, ScheduleError> {
        self.mutate(|state| {
            let id = make_schedule_id(&draft.name, &state.schedules);
            let schedule = clean_schedule(id, draft)?;
            state.schedules.push(schedule.clone());
            state.schedules.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(schedule)
        })
    }

    /// Replace the schedule with `id` from a draft, keeping its id. Unknown ids are
    /// reported.
    pub fn edit(&self, id: &str, draft: ScheduleDraft) -> Result<Schedule, ScheduleError> {
        let schedule = clean_schedule(id.to_owned(), draft)?;
        self.mutate(|state| {
            let Some(slot) = state.schedules.iter_mut().find(|s| s.id == id) else {
                return Err(ScheduleError::Unknown(id.to_owned()));
            };
            *slot = schedule.clone();
            state.schedules.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(schedule)
        })
    }

    /// Remove the schedule with `id`. Unknown ids are reported.
    pub fn remove(&self, id: &str) -> Result<(), ScheduleError> {
        self.mutate(|state| {
            let before = state.schedules.len();
            state.schedules.retain(|s| s.id != id);
            if state.schedules.len() == before {
                return Err(ScheduleError::Unknown(id.to_owned()));
            }
            Ok(())
        })
    }

    /// Replace every schedule at once, from a restored backup. Each entry is
    /// validated and normalised; one invalid entry rejects the whole restore.
    pub fn replace(&self, schedules: Vec<Schedule>) -> Result<(), ScheduleError> {
        let clean = clean_state(StoredSchedules { schedules })?;
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
        change: impl FnOnce(&mut StoredSchedules) -> Result<T, ScheduleError>,
    ) -> Result<T, ScheduleError> {
        let result = {
            let mut state = self.inner.state.write().expect("schedules lock poisoned");
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
            .expect("schedules lock poisoned")
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }
}

/// Build a stable, unique id for a new schedule from its name: a lowercase slug of
/// the alphanumeric runs joined by dashes, with a numeric suffix if taken. Falls
/// back to `schedule` for a name with no usable characters.
fn make_schedule_id(name: &str, existing: &[Schedule]) -> String {
    let mut slug = String::new();
    let mut last_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let base = slug.trim_matches('-');
    let base = if base.is_empty() { "schedule" } else { base };
    if !existing.iter().any(|s| s.id == base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !existing.iter().any(|s| &s.id == candidate))
        .expect("an unused suffix always exists")
}

/// Validate and normalise a draft into a stored schedule with the given id.
fn clean_schedule(id: String, draft: ScheduleDraft) -> Result<Schedule, ScheduleError> {
    if !valid_device_name(&draft.name) {
        return Err(ScheduleError::InvalidName);
    }
    if draft.days.is_empty() {
        return Err(ScheduleError::InvalidDays);
    }
    if !valid_hhmm(&draft.start) {
        return Err(ScheduleError::InvalidTime(draft.start));
    }
    if !valid_hhmm(&draft.end) {
        return Err(ScheduleError::InvalidTime(draft.end));
    }
    let target = normalise_target(draft.target)?;
    Ok(Schedule {
        id,
        name: draft.name.trim().to_owned(),
        enabled: draft.enabled,
        days: dedup_days(draft.days),
        start: draft.start.trim().to_owned(),
        end: draft.end.trim().to_owned(),
        target,
        effect: normalise_effect(draft.effect),
    })
}

/// Drop the id for the global scope; require (and trim) one for a group. A device
/// target is additionally parsed and normalised to its canonical IP/CIDR form; a
/// malformed one is rejected.
fn normalise_target(target: ScheduleTarget) -> Result<ScheduleTarget, ScheduleError> {
    match target.scope {
        TargetScope::Global => Ok(ScheduleTarget {
            scope: TargetScope::Global,
            id: None,
        }),
        TargetScope::Group => {
            let id = target
                .id
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .ok_or(ScheduleError::InvalidTarget)?;
            Ok(ScheduleTarget {
                scope: TargetScope::Group,
                id: Some(id),
            })
        }
        TargetScope::Device => {
            let raw = target
                .id
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .ok_or(ScheduleError::InvalidTarget)?;
            let normalised = parse_target(&raw).ok_or(ScheduleError::InvalidTarget)?;
            Ok(ScheduleTarget {
                scope: TargetScope::Device,
                id: Some(normalised.to_string()),
            })
        }
    }
}

/// Keep the first of each weekday, preserving order.
fn dedup_days(days: Vec<Day>) -> Vec<Day> {
    let mut seen = Vec::with_capacity(days.len());
    for day in days {
        if !seen.contains(&day) {
            seen.push(day);
        }
    }
    seen
}

/// A pause ignores its delta; keep it empty.
fn normalise_effect(effect: ScheduleEffect) -> ScheduleEffect {
    match effect.mode {
        crate::EffectMode::Pause => ScheduleEffect {
            mode: crate::EffectMode::Pause,
            delta: justdns_clients::PolicyDelta::default(),
        },
        crate::EffectMode::Block => effect,
    }
}

/// Clean the state from a load or an import: validate and normalise every entry,
/// generating an id for one left blank and dropping duplicate ids (last wins). One
/// bad entry rejects the whole state.
fn clean_state(stored: StoredSchedules) -> Result<StoredSchedules, ScheduleError> {
    let mut schedules: Vec<Schedule> = Vec::with_capacity(stored.schedules.len());
    for schedule in stored.schedules {
        let id = if schedule.id.trim().is_empty() {
            make_schedule_id(&schedule.name, &schedules)
        } else {
            schedule.id.trim().to_owned()
        };
        let clean = clean_schedule(id, ScheduleDraft::from(schedule))?;
        match schedules.iter_mut().find(|s| s.id == clean.id) {
            Some(existing) => *existing = clean,
            None => schedules.push(clean),
        }
    }
    schedules.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(StoredSchedules { schedules })
}

/// Write `stored` to `path` as pretty JSON, atomically (temp file then rename),
/// creating the parent directory if needed.
fn write_atomic(path: &Path, stored: &StoredSchedules) -> Result<(), ScheduleError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| ScheduleError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let json = serde_json::to_vec_pretty(stored).map_err(|source| ScheduleError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| ScheduleError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| ScheduleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EffectMode;

    fn store() -> (tempfile::TempDir, ScheduleStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ScheduleStore::load(dir.path().join("schedules.json")).unwrap();
        (dir, store)
    }

    fn draft(name: &str, scope: TargetScope, id: Option<&str>) -> ScheduleDraft {
        ScheduleDraft {
            name: name.to_owned(),
            enabled: true,
            days: vec![Day::Mon, Day::Tue],
            start: "22:00".to_owned(),
            end: "07:00".to_owned(),
            target: ScheduleTarget {
                scope,
                id: id.map(str::to_owned),
            },
            effect: ScheduleEffect {
                mode: EffectMode::Pause,
                delta: Default::default(),
            },
        }
    }

    #[test]
    fn add_generates_unique_ids_and_persists() {
        let (dir, store) = store();
        let a = store
            .add(draft("Bedtime", TargetScope::Global, None))
            .unwrap();
        let b = store
            .add(draft("Bedtime", TargetScope::Global, None))
            .unwrap();
        assert_eq!(a.id, "bedtime");
        assert_eq!(b.id, "bedtime-2", "a clashing name gets a suffix");

        let reopened = ScheduleStore::load(dir.path().join("schedules.json")).unwrap();
        assert_eq!(reopened.view().len(), 2);
    }

    #[test]
    fn edit_keeps_id_and_reports_unknown() {
        let (_dir, store) = store();
        let a = store
            .add(draft("Focus", TargetScope::Global, None))
            .unwrap();
        let mut d = draft("Focus time", TargetScope::Global, None);
        d.start = "09:00".to_owned();
        d.end = "12:00".to_owned();
        let edited = store.edit(&a.id, d).unwrap();
        assert_eq!(edited.id, a.id, "id kept across rename");
        assert_eq!(edited.name, "Focus time");
        assert_eq!(edited.start, "09:00");

        assert!(matches!(
            store.edit("ghost", draft("X", TargetScope::Global, None)),
            Err(ScheduleError::Unknown(_))
        ));
    }

    #[test]
    fn remove_drops_it_and_reports_unknown() {
        let (_dir, store) = store();
        let a = store
            .add(draft("Bedtime", TargetScope::Global, None))
            .unwrap();
        store.remove(&a.id).unwrap();
        assert!(store.view().is_empty());
        assert!(matches!(
            store.remove(&a.id),
            Err(ScheduleError::Unknown(_))
        ));
    }

    #[test]
    fn validation_rejects_bad_drafts() {
        let (_dir, store) = store();
        // Empty name.
        assert!(matches!(
            store.add(draft("  ", TargetScope::Global, None)),
            Err(ScheduleError::InvalidName)
        ));
        // No weekdays.
        let mut d = draft("X", TargetScope::Global, None);
        d.days.clear();
        assert!(matches!(store.add(d), Err(ScheduleError::InvalidDays)));
        // Bad time.
        let mut d = draft("X", TargetScope::Global, None);
        d.end = "25:00".to_owned();
        assert!(matches!(store.add(d), Err(ScheduleError::InvalidTime(_))));
        // Group scope without an id.
        assert!(matches!(
            store.add(draft("X", TargetScope::Group, None)),
            Err(ScheduleError::InvalidTarget)
        ));
    }

    #[test]
    fn global_target_drops_id_and_pause_drops_delta() {
        let (_dir, store) = store();
        let mut d = draft("X", TargetScope::Global, Some("stray"));
        d.effect = ScheduleEffect {
            mode: EffectMode::Pause,
            delta: justdns_clients::PolicyDelta {
                block: vec!["ignored.test".to_owned()],
                ..Default::default()
            },
        };
        let s = store.add(d).unwrap();
        assert_eq!(s.target.id, None, "global carries no id");
        assert!(s.effect.delta.is_empty(), "pause drops its delta");
    }

    #[test]
    fn group_target_keeps_trimmed_id() {
        let (_dir, store) = store();
        let s = store
            .add(draft("Kids bedtime", TargetScope::Group, Some("  kids  ")))
            .unwrap();
        assert_eq!(s.target.scope, TargetScope::Group);
        assert_eq!(s.target.id.as_deref(), Some("kids"));
    }

    #[test]
    fn replace_round_trips_and_rejects_junk() {
        let (_dir, store) = store();
        store
            .add(draft("Bedtime", TargetScope::Global, None))
            .unwrap();
        let snapshot = store.snapshot();
        store.replace(Vec::new()).unwrap();
        assert!(store.view().is_empty());
        store.replace(snapshot).unwrap();
        assert_eq!(store.view().len(), 1);

        let bad = vec![Schedule {
            id: "x".to_owned(),
            name: "X".to_owned(),
            enabled: true,
            days: vec![Day::Mon],
            start: "99:99".to_owned(),
            end: "07:00".to_owned(),
            target: ScheduleTarget {
                scope: TargetScope::Global,
                id: None,
            },
            effect: ScheduleEffect {
                mode: EffectMode::Pause,
                delta: Default::default(),
            },
        }];
        assert!(matches!(
            store.replace(bad),
            Err(ScheduleError::InvalidTime(_))
        ));
        assert_eq!(store.view().len(), 1, "unchanged after a reject");
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
        let a = store.add(draft("A", TargetScope::Global, None)).unwrap();
        store.remove(&a.id).unwrap();
        assert_eq!(hits.load(Ordering::Relaxed), 2, "once per change");
    }
}
