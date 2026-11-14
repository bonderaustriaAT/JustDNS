// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persistent client (device) store: the user's address→name mappings, kept
//! in a JSON file on the data volume.
//!
//! Every edit validates and normalises the entry, then rewrites the file
//! atomically, so a change in the dashboard is durable at once. Naming is a
//! read-time concern — the log and statistics resolve addresses to names through
//! a [`ClientResolver`](crate::ClientResolver) the web layer builds from a
//! snapshot — so there is no live table to publish here, unlike the rewrite
//! store.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, Group, PolicyDelta, parse_target, valid_device_name};

/// A callback run after every persisted change (the binary rebuilds the live
/// per-client policies).
type OnChange = Arc<dyn Fn() + Send + Sync>;

/// Errors from loading, saving or editing the devices and groups.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("clients io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("clients parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("'{0}' is not a valid IP address or CIDR range")]
    InvalidTarget(String),
    #[error("a device needs a name")]
    InvalidName,
    #[error("no device for '{0}'")]
    Unknown(String),
    #[error("a group needs a name")]
    InvalidGroupName,
    #[error("no group with id '{0}'")]
    UnknownGroup(String),
}

/// The persisted shape and in-memory state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredClients {
    devices: Vec<Device>,
    groups: Vec<Group>,
}

struct Inner {
    path: PathBuf,
    /// Devices and groups under one lock, so a mutation spanning both (deleting a
    /// group devices reference) stays consistent.
    state: RwLock<StoredClients>,
    on_change: RwLock<Option<OnChange>>,
}

/// A cheap, cloneable handle to the persistent devices.
#[derive(Clone)]
pub struct ClientStore {
    inner: Arc<Inner>,
}

impl ClientStore {
    /// Load the devices and groups from `path`. A missing file starts empty and is
    /// written. Entries are re-validated on load; a bad target or empty name is
    /// rejected, and policies and group memberships are preserved.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, ClientError> {
        let path = path.into();
        let stored: StoredClients = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| ClientError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let seed = StoredClients::default();
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(ClientError::Io {
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

    /// Register the after-change callback, replacing any previous one.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self.inner.on_change.write().expect("clients lock poisoned") = Some(Arc::new(callback));
    }

    fn state(&self) -> std::sync::RwLockReadGuard<'_, StoredClients> {
        self.inner.state.read().expect("clients lock poisoned")
    }

    /// Every device, sorted by target.
    pub fn view(&self) -> Vec<Device> {
        self.state().devices.clone()
    }

    /// Every group.
    pub fn groups(&self) -> Vec<Group> {
        self.state().groups.clone()
    }

    /// Every device, for a backup export.
    pub fn snapshot(&self) -> Vec<Device> {
        self.state().devices.clone()
    }

    /// Every group, for a backup export.
    pub fn snapshot_groups(&self) -> Vec<Group> {
        self.state().groups.clone()
    }

    /// Add a device, or replace the name/note/kind of one that already matches
    /// the same (normalised) target. The device's group and policy delta are
    /// preserved when it already exists; a new device starts ungrouped with no
    /// delta. The target is validated and normalised and the name required.
    pub fn set(
        &self,
        target: &str,
        name: &str,
        note: Option<&str>,
        kind: DeviceKind,
    ) -> Result<Device, ClientError> {
        let device = make_device(target, name, note, kind)?;
        self.mutate(|state| {
            let stored = upsert(&mut state.devices, device);
            state.devices.sort_by(|a, b| a.target.cmp(&b.target));
            Ok(stored)
        })
    }

    /// Change a device: re-point it at `target` and replace its name/note/kind,
    /// keyed by its old target, keeping its group and policy delta. An unknown
    /// `old_target` is reported. If the new target collides with another entry,
    /// the two merge onto the new target.
    pub fn edit(
        &self,
        old_target: &str,
        target: &str,
        name: &str,
        note: Option<&str>,
        kind: DeviceKind,
    ) -> Result<Device, ClientError> {
        let old = normalise_target(old_target)?;
        let mut device = make_device(target, name, note, kind)?;
        self.mutate(|state| {
            let Some(position) = state.devices.iter().position(|d| d.target == old) else {
                return Err(ClientError::Unknown(old.clone()));
            };
            // Carry the existing policy and group onto the re-pointed device.
            let previous = state.devices.remove(position);
            device.group = previous.group;
            device.policy = previous.policy;
            let stored = upsert(&mut state.devices, device.clone());
            state.devices.sort_by(|a, b| a.target.cmp(&b.target));
            Ok(stored)
        })
    }

    /// Set a device's group and policy delta, keyed by its (normalised) target.
    /// Creates the device named after its address if it does not exist yet, so a
    /// policy can be attached to a seen-but-unnamed client in one step. A named
    /// `group` must exist. Returns the stored device.
    pub fn set_device_policy(
        &self,
        target: &str,
        group: Option<&str>,
        policy: PolicyDelta,
    ) -> Result<Device, ClientError> {
        let target = normalise_target(target)?;
        let group = group.map(str::trim).filter(|g| !g.is_empty());
        self.mutate(|state| {
            if let Some(id) = group {
                if !state.groups.iter().any(|g| g.id == id) {
                    return Err(ClientError::UnknownGroup(id.to_owned()));
                }
            }
            let device = match state.devices.iter_mut().find(|d| d.target == target) {
                Some(existing) => {
                    existing.group = group.map(str::to_owned);
                    existing.policy = policy;
                    existing.clone()
                }
                None => {
                    let device = Device {
                        target: target.clone(),
                        name: target.clone(),
                        note: None,
                        kind: DeviceKind::Other,
                        group: group.map(str::to_owned),
                        policy,
                    };
                    state.devices.push(device.clone());
                    state.devices.sort_by(|a, b| a.target.cmp(&b.target));
                    device
                }
            };
            Ok(device)
        })
    }

    /// Add several domains to one device's own block delta, creating the device
    /// (named after its address) if it does not exist yet. Returns how many were
    /// newly added; a domain already blocked for the device is left as it is, and
    /// a blank one is skipped. The device's group and allow list are preserved,
    /// and nothing is written when nothing is new. Used by the privacy
    /// dashboard's "block for this device" action, which adds a whole company's
    /// catalogue of domains at once.
    pub fn add_device_blocks(&self, target: &str, domains: &[&str]) -> Result<usize, ClientError> {
        let target = normalise_target(target)?;
        let added;
        {
            let mut state = self.inner.state.write().expect("clients lock poisoned");
            // Work out what is genuinely new before touching the device, so a
            // re-block creates no stray device and triggers no rebuild.
            let mut seen: std::collections::HashSet<String> = state
                .devices
                .iter()
                .find(|d| d.target == target)
                .map(|d| d.policy.block.iter().cloned().collect())
                .unwrap_or_default();
            let mut to_add = Vec::new();
            for domain in domains {
                let domain = domain.trim();
                if domain.is_empty() {
                    continue;
                }
                if seen.insert(domain.to_owned()) {
                    to_add.push(domain.to_owned());
                }
            }
            if to_add.is_empty() {
                return Ok(0);
            }
            added = to_add.len();

            let device = match state.devices.iter_mut().find(|d| d.target == target) {
                Some(existing) => existing,
                None => {
                    state.devices.push(Device {
                        target: target.clone(),
                        name: target.clone(),
                        note: None,
                        kind: DeviceKind::Other,
                        group: None,
                        policy: PolicyDelta::default(),
                    });
                    state
                        .devices
                        .iter_mut()
                        .find(|d| d.target == target)
                        .expect("just pushed")
                }
            };
            device.policy.block.extend(to_add);
            device.policy.block.sort();
            state.devices.sort_by(|a, b| a.target.cmp(&b.target));
            write_atomic(&self.inner.path, &state)?;
        }
        self.fire_on_change();
        Ok(added)
    }

    /// Remove the device matching `target`; an unknown target is reported.
    pub fn remove(&self, target: &str) -> Result<(), ClientError> {
        let target = normalise_target(target)?;
        self.mutate(|state| {
            let before = state.devices.len();
            state.devices.retain(|d| d.target != target);
            if state.devices.len() == before {
                return Err(ClientError::Unknown(target.clone()));
            }
            Ok(())
        })
    }

    /// Create a group with a generated, unique id. The name is required.
    pub fn add_group(&self, name: &str, policy: PolicyDelta) -> Result<Group, ClientError> {
        if !valid_device_name(name) {
            return Err(ClientError::InvalidGroupName);
        }
        let name = name.trim().to_owned();
        self.mutate(|state| {
            let id = make_group_id(&name, &state.groups);
            let group = Group {
                id,
                name: name.clone(),
                policy: policy.clone(),
            };
            state.groups.push(group.clone());
            state.groups.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(group)
        })
    }

    /// Rename a group and replace its policy delta, keyed by its id. Unknown ids
    /// are reported.
    pub fn edit_group(
        &self,
        id: &str,
        name: &str,
        policy: PolicyDelta,
    ) -> Result<Group, ClientError> {
        if !valid_device_name(name) {
            return Err(ClientError::InvalidGroupName);
        }
        let name = name.trim().to_owned();
        self.mutate(|state| {
            let Some(group) = state.groups.iter_mut().find(|g| g.id == id) else {
                return Err(ClientError::UnknownGroup(id.to_owned()));
            };
            group.name = name.clone();
            group.policy = policy.clone();
            let stored = group.clone();
            state.groups.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(stored)
        })
    }

    /// Delete a group by id and detach every device that referenced it. Unknown
    /// ids are reported.
    pub fn remove_group(&self, id: &str) -> Result<(), ClientError> {
        self.mutate(|state| {
            let before = state.groups.len();
            state.groups.retain(|g| g.id != id);
            if state.groups.len() == before {
                return Err(ClientError::UnknownGroup(id.to_owned()));
            }
            for device in &mut state.devices {
                if device.group.as_deref() == Some(id) {
                    device.group = None;
                }
            }
            Ok(())
        })
    }

    /// Replace every device and group at once, from a restored backup. Each
    /// entry is validated and normalised; one invalid entry rejects the whole
    /// restore and nothing is written.
    pub fn replace(&self, devices: Vec<Device>, groups: Vec<Group>) -> Result<(), ClientError> {
        let clean = clean_state(StoredClients { devices, groups })?;
        self.mutate(|state| {
            *state = clean;
            Ok(())
        })
    }

    /// Apply `change` to the state under the write lock, persist the result to
    /// disk, then (after releasing the lock) run the on-change callback so the
    /// live policies are rebuilt. The mutation returns a value handed back to the
    /// caller; a failing mutation leaves the file and the callback untouched.
    fn mutate<T>(
        &self,
        change: impl FnOnce(&mut StoredClients) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let result = {
            let mut state = self.inner.state.write().expect("clients lock poisoned");
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
            .expect("clients lock poisoned")
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }
}

/// Build a stable, unique id for a new group from its name: a lowercase slug of
/// the alphanumeric runs, joined by dashes, with a numeric suffix if that slug
/// is already taken. Falls back to `group` for a name with no usable characters.
fn make_group_id(name: &str, existing: &[Group]) -> String {
    let mut slug = String::new();
    let mut last_dash = true; // avoid a leading dash
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
    let base = if base.is_empty() { "group" } else { base };
    if !existing.iter().any(|g| g.id == base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !existing.iter().any(|g| &g.id == candidate))
        .expect("an unused suffix always exists")
}

/// Validate and normalise a target string into its canonical stored form (a
/// plain IP, or a masked CIDR). Rejects anything that is not an address or range.
fn normalise_target(target: &str) -> Result<String, ClientError> {
    parse_target(target)
        .map(|matcher| matcher.to_string())
        .ok_or_else(|| ClientError::InvalidTarget(target.to_owned()))
}

/// Build a validated, normalised device from its parts.
fn make_device(
    target: &str,
    name: &str,
    note: Option<&str>,
    kind: DeviceKind,
) -> Result<Device, ClientError> {
    let target = normalise_target(target)?;
    if !valid_device_name(name) {
        return Err(ClientError::InvalidName);
    }
    Ok(Device {
        target,
        name: name.trim().to_owned(),
        note: clean_note(note),
        kind,
        // New devices start ungrouped with no delta.
        ..Default::default()
    })
}

/// Trim a note, dropping it when empty.
fn clean_note(note: Option<&str>) -> Option<String> {
    note.map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_owned)
}

/// Insert a device, or replace the fields of one that already has this target.
/// Returns the stored device.
fn upsert(devices: &mut Vec<Device>, device: Device) -> Device {
    match devices.iter_mut().find(|d| d.target == device.target) {
        Some(existing) => {
            existing.name = device.name;
            existing.note = device.note;
            existing.kind = device.kind;
            existing.clone()
        }
        None => {
            devices.push(device.clone());
            device
        }
    }
}

/// Clean the state from a load or an import: validate and normalise every device
/// and group, keeping each device's group membership and policy delta, dropping
/// exact-duplicate device targets and duplicate group ids (last wins). One bad
/// entry rejects the whole state.
fn clean_state(stored: StoredClients) -> Result<StoredClients, ClientError> {
    let mut devices: Vec<Device> = Vec::with_capacity(stored.devices.len());
    for device in stored.devices {
        let target = normalise_target(&device.target)?;
        if !valid_device_name(&device.name) {
            return Err(ClientError::InvalidName);
        }
        let clean = Device {
            target,
            name: device.name.trim().to_owned(),
            note: clean_note(device.note.as_deref()),
            kind: device.kind,
            group: device.group,
            policy: device.policy,
        };
        // Replace an earlier entry with the same target, keeping this one's policy.
        match devices.iter_mut().find(|d| d.target == clean.target) {
            Some(existing) => *existing = clean,
            None => devices.push(clean),
        }
    }
    devices.sort_by(|a, b| a.target.cmp(&b.target));

    let mut groups: Vec<Group> = Vec::with_capacity(stored.groups.len());
    for group in stored.groups {
        if !valid_device_name(&group.name) {
            return Err(ClientError::InvalidGroupName);
        }
        // A group needs a stable id; generate one if an import left it blank.
        let id = if group.id.trim().is_empty() {
            make_group_id(&group.name, &groups)
        } else {
            group.id.trim().to_owned()
        };
        let clean = Group {
            id,
            name: group.name.trim().to_owned(),
            policy: group.policy,
        };
        match groups.iter_mut().find(|g| g.id == clean.id) {
            Some(existing) => *existing = clean,
            None => groups.push(clean),
        }
    }
    groups.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(StoredClients { devices, groups })
}

/// Write `stored` to `path` as pretty JSON, atomically (temp file then rename),
/// creating the parent directory if needed.
fn write_atomic(path: &Path, stored: &StoredClients) -> Result<(), ClientError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| ClientError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let json = serde_json::to_vec_pretty(stored).map_err(|source| ClientError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| ClientError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| ClientError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ClientStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ClientStore::load(dir.path().join("clients.json")).unwrap();
        (dir, store)
    }

    #[test]
    fn set_normalises_and_stores() {
        let (_dir, store) = store();
        store
            .set(
                "192.168.20.10",
                "  Laptop  ",
                Some("  "),
                DeviceKind::Laptop,
            )
            .unwrap();
        let view = store.view();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].target, "192.168.20.10");
        assert_eq!(view[0].name, "Laptop", "name trimmed");
        assert_eq!(view[0].note, None, "blank note dropped");
        assert_eq!(view[0].kind, DeviceKind::Laptop);
    }

    #[test]
    fn set_masks_a_cidr_target() {
        let (_dir, store) = store();
        store
            .set("192.168.30.42/24", "Guest Wi-Fi", None, DeviceKind::Other)
            .unwrap();
        assert_eq!(store.view()[0].target, "192.168.30.0/24", "base masked");
    }

    #[test]
    fn set_replaces_fields_for_the_same_target() {
        let (_dir, store) = store();
        store
            .set("192.168.20.10", "Old", None, DeviceKind::Other)
            .unwrap();
        store
            .set("192.168.20.10", "New", Some("desk"), DeviceKind::Desktop)
            .unwrap();
        let view = store.view();
        assert_eq!(view.len(), 1, "one entry per target");
        assert_eq!(view[0].name, "New");
        assert_eq!(view[0].note.as_deref(), Some("desk"));
        assert_eq!(view[0].kind, DeviceKind::Desktop);
    }

    #[test]
    fn invalid_target_and_name_are_rejected() {
        let (_dir, store) = store();
        assert!(matches!(
            store.set("nope", "X", None, DeviceKind::Other),
            Err(ClientError::InvalidTarget(_))
        ));
        assert!(matches!(
            store.set("192.168.20.10", "  ", None, DeviceKind::Other),
            Err(ClientError::InvalidName)
        ));
    }

    #[test]
    fn edit_repoints_and_reports_unknown() {
        let (_dir, store) = store();
        store
            .set("192.168.20.10", "Laptop", None, DeviceKind::Laptop)
            .unwrap();
        store
            .edit(
                "192.168.20.10",
                "192.168.20.11",
                "Laptop",
                None,
                DeviceKind::Laptop,
            )
            .unwrap();
        let view = store.view();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].target, "192.168.20.11", "re-pointed");

        assert!(matches!(
            store.edit("10.0.0.9", "10.0.0.8", "X", None, DeviceKind::Other),
            Err(ClientError::Unknown(_))
        ));
    }

    #[test]
    fn remove_drops_it_and_reports_unknown() {
        let (_dir, store) = store();
        store
            .set("192.168.20.10", "Laptop", None, DeviceKind::Laptop)
            .unwrap();
        store.remove("192.168.20.10").unwrap();
        assert!(store.view().is_empty());
        assert!(matches!(
            store.remove("192.168.20.10"),
            Err(ClientError::Unknown(_))
        ));
    }

    #[test]
    fn add_device_blocks_creates_merges_and_preserves() {
        let (_dir, store) = store();

        // An unknown client is created, named after its address.
        assert_eq!(
            store
                .add_device_blocks("192.168.20.10", &["a.test", "b.test"])
                .unwrap(),
            2
        );
        let view = store.view();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].target, "192.168.20.10");
        assert_eq!(view[0].name, "192.168.20.10", "named after its address");
        assert_eq!(
            view[0].policy.block,
            vec!["a.test".to_owned(), "b.test".to_owned()]
        );

        // A re-block (including blanks and repeats) adds nothing.
        assert_eq!(
            store
                .add_device_blocks("192.168.20.10", &["a.test", "", "a.test"])
                .unwrap(),
            0
        );
        assert_eq!(store.view().len(), 1);

        // The device's group and allow list survive a later block.
        let group = store.add_group("Kids", PolicyDelta::default()).unwrap();
        store
            .set_device_policy(
                "192.168.20.20",
                Some(&group.id),
                delta(&["ok.test"], &["old.test"]),
            )
            .unwrap();
        assert_eq!(
            store
                .add_device_blocks("192.168.20.20", &["new.test", "old.test"])
                .unwrap(),
            1,
            "only the genuinely new domain counts"
        );
        let dev = store
            .view()
            .into_iter()
            .find(|d| d.target == "192.168.20.20")
            .unwrap();
        assert_eq!(dev.group.as_deref(), Some(group.id.as_str()), "group kept");
        assert_eq!(dev.policy.allow, vec!["ok.test".to_owned()], "allow kept");
        assert_eq!(
            dev.policy.block,
            vec!["new.test".to_owned(), "old.test".to_owned()]
        );
    }

    #[test]
    fn devices_persist_across_reload() {
        let (dir, store) = store();
        store
            .set("192.168.30.0/24", "Guest", None, DeviceKind::Other)
            .unwrap();
        let reopened = ClientStore::load(dir.path().join("clients.json")).unwrap();
        assert_eq!(reopened.view().len(), 1);
        assert_eq!(reopened.view()[0].name, "Guest");
    }

    #[test]
    fn replace_round_trips_and_rejects_junk() {
        let (_dir, store) = store();
        store
            .set("192.168.20.10", "Laptop", None, DeviceKind::Laptop)
            .unwrap();
        let snapshot = store.snapshot();

        store.replace(Vec::new(), Vec::new()).unwrap();
        assert!(store.view().is_empty());
        store.replace(snapshot, Vec::new()).unwrap();
        assert_eq!(store.view()[0].name, "Laptop");

        let bad = vec![Device {
            target: "not-an-ip".to_owned(),
            name: "X".to_owned(),
            ..Default::default()
        }];
        assert!(matches!(
            store.replace(bad, Vec::new()),
            Err(ClientError::InvalidTarget(_))
        ));
        assert_eq!(store.view().len(), 1, "unchanged after a reject");
    }

    #[test]
    fn a_hand_edited_file_with_a_bad_entry_is_rejected_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clients.json");
        std::fs::write(
            &path,
            r#"{"devices":[{"target":"bad","name":"X","kind":"other"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            ClientStore::load(&path),
            Err(ClientError::InvalidTarget(_))
        ));
    }

    fn delta(allow: &[&str], block: &[&str]) -> PolicyDelta {
        PolicyDelta {
            allow: allow.iter().map(|s| (*s).to_owned()).collect(),
            block: block.iter().map(|s| (*s).to_owned()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn groups_crud_generates_unique_ids_and_persists() {
        let (dir, store) = store();
        let a = store
            .add_group("Kids", delta(&[], &["games.test"]))
            .unwrap();
        let b = store.add_group("Kids", PolicyDelta::default()).unwrap();
        assert_eq!(a.id, "kids");
        assert_eq!(b.id, "kids-2", "a clashing name gets a numeric suffix");

        store
            .edit_group(&a.id, "Children", PolicyDelta::default())
            .unwrap();
        assert_eq!(
            store.groups().iter().find(|g| g.id == "kids").unwrap().name,
            "Children",
            "rename keeps the id"
        );

        // Groups survive a reload.
        let reopened = ClientStore::load(dir.path().join("clients.json")).unwrap();
        assert_eq!(reopened.groups().len(), 2);

        assert!(matches!(
            store.edit_group("nope", "X", PolicyDelta::default()),
            Err(ClientError::UnknownGroup(_))
        ));
    }

    #[test]
    fn removing_a_group_detaches_its_devices() {
        let (_dir, store) = store();
        let group = store.add_group("Kids", PolicyDelta::default()).unwrap();
        store
            .set_device_policy("192.168.20.10", Some(&group.id), delta(&[], &["ads.test"]))
            .unwrap();
        assert_eq!(store.view()[0].group.as_deref(), Some("kids"));

        store.remove_group(&group.id).unwrap();
        assert!(store.groups().is_empty());
        assert_eq!(
            store.view()[0].group,
            None,
            "the device is detached, not deleted"
        );
        assert_eq!(
            store.view()[0].policy.block,
            vec!["ads.test".to_owned()],
            "the device keeps its own delta"
        );
    }

    #[test]
    fn set_device_policy_requires_a_known_group_and_preserves_across_rename() {
        let (_dir, store) = store();
        assert!(matches!(
            store.set_device_policy("192.168.20.10", Some("ghost"), PolicyDelta::default()),
            Err(ClientError::UnknownGroup(_))
        ));

        store
            .set("192.168.20.10", "Laptop", Some("desk"), DeviceKind::Laptop)
            .unwrap();
        store
            .set_device_policy("192.168.20.10", None, delta(&["allow.test"], &[]))
            .unwrap();
        // A later naming edit keeps the policy that was set.
        store
            .edit(
                "192.168.20.10",
                "192.168.20.11",
                "Laptop",
                None,
                DeviceKind::Laptop,
            )
            .unwrap();
        let device = &store.view()[0];
        assert_eq!(device.target, "192.168.20.11");
        assert_eq!(device.policy.allow, vec!["allow.test".to_owned()]);
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
        store
            .set("192.168.20.10", "Laptop", None, DeviceKind::Laptop)
            .unwrap();
        store.add_group("Kids", PolicyDelta::default()).unwrap();
        assert_eq!(hits.load(Ordering::Relaxed), 2, "once per change");
    }

    #[test]
    fn an_old_file_without_groups_or_policy_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clients.json");
        // Exactly the shape older builds wrote: devices only, no policy/group.
        std::fs::write(
            &path,
            r#"{"devices":[{"target":"192.168.20.10","name":"Laptop","kind":"laptop"}]}"#,
        )
        .unwrap();
        let store = ClientStore::load(&path).unwrap();
        assert_eq!(store.view().len(), 1);
        assert!(store.view()[0].policy.is_empty());
        assert_eq!(store.view()[0].group, None);
        assert!(store.groups().is_empty());
    }
}
