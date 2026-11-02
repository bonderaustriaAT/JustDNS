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

use crate::{Device, DeviceKind, parse_target, valid_device_name};

/// Errors from loading, saving or editing the devices.
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
}

/// The persisted shape. Wrapped in a struct rather than a bare array so the file
/// can gain fields later without a migration, matching the other stores.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredClients {
    devices: Vec<Device>,
}

struct Inner {
    path: PathBuf,
    devices: RwLock<Vec<Device>>,
}

/// A cheap, cloneable handle to the persistent devices.
#[derive(Clone)]
pub struct ClientStore {
    inner: Arc<Inner>,
}

impl ClientStore {
    /// Load the devices from `path`. A missing file starts empty and is written
    /// on first run, so later edits always have a file to replace. Entries are
    /// re-validated on load, so a hand-edited file with a bad target or an empty
    /// name is rejected rather than silently kept.
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

        let devices = clean_list(stored.devices)?;
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                devices: RwLock::new(devices),
            }),
        })
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<Device>> {
        self.inner.devices.read().expect("clients lock poisoned")
    }

    /// Every device, sorted by target, for the dashboard and for resolving names.
    pub fn view(&self) -> Vec<Device> {
        self.read().clone()
    }

    /// A snapshot of every device, for a backup export.
    pub fn snapshot(&self) -> Vec<Device> {
        self.read().clone()
    }

    /// Add a device, or replace the name/note/kind of one that already matches the
    /// same (normalised) target. The target is validated and normalised and the
    /// name required; returns the stored device.
    pub fn set(
        &self,
        target: &str,
        name: &str,
        note: Option<&str>,
        kind: DeviceKind,
    ) -> Result<Device, ClientError> {
        let device = make_device(target, name, note, kind)?;
        let mut devices = self.inner.devices.write().expect("clients lock poisoned");
        let stored = upsert(&mut devices, device);
        devices.sort_by(|a, b| a.target.cmp(&b.target));
        self.persist(&devices)?;
        Ok(stored)
    }

    /// Change a device: re-point it at `target` and replace its name/note/kind,
    /// keyed by its old target. An unknown `old_target` is reported. If the new
    /// target collides with another entry, the two merge onto the new target.
    pub fn edit(
        &self,
        old_target: &str,
        target: &str,
        name: &str,
        note: Option<&str>,
        kind: DeviceKind,
    ) -> Result<Device, ClientError> {
        let old = normalise_target(old_target)?;
        let device = make_device(target, name, note, kind)?;
        let mut devices = self.inner.devices.write().expect("clients lock poisoned");
        let Some(position) = devices.iter().position(|d| d.target == old) else {
            return Err(ClientError::Unknown(old));
        };
        devices.remove(position);
        let stored = upsert(&mut devices, device);
        devices.sort_by(|a, b| a.target.cmp(&b.target));
        self.persist(&devices)?;
        Ok(stored)
    }

    /// Remove the device matching `target`. Unknown targets are reported, so a
    /// stale delete from the UI surfaces rather than silently succeeding.
    pub fn remove(&self, target: &str) -> Result<(), ClientError> {
        let target = normalise_target(target)?;
        let mut devices = self.inner.devices.write().expect("clients lock poisoned");
        let before = devices.len();
        devices.retain(|d| d.target != target);
        if devices.len() == before {
            return Err(ClientError::Unknown(target));
        }
        self.persist(&devices)
    }

    /// Replace every device at once, from a restored backup. Each entry is
    /// validated and normalised; one invalid entry rejects the whole restore and
    /// nothing is written.
    pub fn replace(&self, devices: Vec<Device>) -> Result<(), ClientError> {
        let clean = clean_list(devices)?;
        let mut guard = self.inner.devices.write().expect("clients lock poisoned");
        *guard = clean;
        self.persist(&guard)
    }

    /// Write the devices to disk atomically.
    fn persist(&self, devices: &[Device]) -> Result<(), ClientError> {
        write_atomic(
            &self.inner.path,
            &StoredClients {
                devices: devices.to_vec(),
            },
        )
    }
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
    })
}

/// Trim a note and drop it when empty, so a blank field never stores an empty
/// string that reads as "has a note" later.
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

/// Clean a list of devices from a load or an import: validate and normalise each
/// one, dropping exact-duplicate targets (last wins). One bad entry rejects the
/// whole list.
fn clean_list(devices: Vec<Device>) -> Result<Vec<Device>, ClientError> {
    let mut clean: Vec<Device> = Vec::with_capacity(devices.len());
    for device in devices {
        let made = make_device(
            &device.target,
            &device.name,
            device.note.as_deref(),
            device.kind,
        )?;
        upsert(&mut clean, made);
    }
    clean.sort_by(|a, b| a.target.cmp(&b.target));
    Ok(clean)
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

        store.replace(Vec::new()).unwrap();
        assert!(store.view().is_empty());
        store.replace(snapshot).unwrap();
        assert_eq!(store.view()[0].name, "Laptop");

        let bad = vec![Device {
            target: "not-an-ip".to_owned(),
            name: "X".to_owned(),
            note: None,
            kind: DeviceKind::Other,
        }];
        assert!(matches!(
            store.replace(bad),
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
}
