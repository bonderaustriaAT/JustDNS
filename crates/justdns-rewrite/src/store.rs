// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persistent rewrite store: the user's local name→IP mappings, kept in a
//! JSON file on the data volume and pushed into the live [`Rewriter`].
//!
//! Every edit rewrites the file atomically and swaps a fresh table into the
//! rewriter, so a change in the dashboard answers the very next query without a
//! restart. Names are validated and normalised on the way in, and each address
//! is parsed, so only usable entries are ever stored.

use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::{Rewrite, Rewriter, normalise, valid_host};

/// Errors from loading, saving or editing the rewrites.
#[derive(Debug, thiserror::Error)]
pub enum RewriteError {
    #[error("rewrites io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("rewrites parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("'{0}' is not a valid host name")]
    InvalidName(String),
    #[error("'{0}' is not a valid IP address")]
    InvalidIp(String),
    #[error("a rewrite needs at least one IP address")]
    NoIps,
    #[error("no rewrite for '{0}'")]
    Unknown(String),
}

/// The persisted shape: just the list of rewrites. Wrapped in a struct (rather
/// than a bare array) so the file can grow more fields later without a migration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredRewrites {
    rewrites: Vec<Rewrite>,
}

/// Everything the dashboard's rewrites view needs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewritesView {
    /// Every rewrite, sorted by name.
    pub rewrites: Vec<Rewrite>,
}

struct Inner {
    path: PathBuf,
    rewrites: RwLock<Vec<Rewrite>>,
    /// The live table the DNS pipeline reads; rebuilt on every edit.
    rewriter: Arc<Rewriter>,
}

/// A cheap, cloneable handle to the persistent rewrites.
#[derive(Clone)]
pub struct RewriteStore {
    inner: Arc<Inner>,
}

impl RewriteStore {
    /// Load the rewrites from `path` and push them into `rewriter`. A missing
    /// file starts empty and is written on first run, so later edits always have
    /// a file to replace.
    pub fn load(path: impl Into<PathBuf>, rewriter: Arc<Rewriter>) -> Result<Self, RewriteError> {
        let path = path.into();
        let stored: StoredRewrites = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| RewriteError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let seed = StoredRewrites::default();
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(RewriteError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };

        rewriter.set(&stored.rewrites);
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                rewrites: RwLock::new(stored.rewrites),
                rewriter,
            }),
        })
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<Rewrite>> {
        self.inner.rewrites.read().expect("rewrites lock poisoned")
    }

    /// A snapshot of every rewrite for the dashboard.
    pub fn view(&self) -> RewritesView {
        RewritesView {
            rewrites: self.read().clone(),
        }
    }

    /// Add a rewrite, or replace the addresses of one that already exists. The
    /// name is validated and normalised, and every address is parsed; an entry
    /// with no usable address is refused. Returns the stored rewrite. A replaced
    /// entry keeps its enabled state; a new one starts enabled.
    pub fn set(&self, name: &str, ips: &[String]) -> Result<Rewrite, RewriteError> {
        let name = validate_name(name)?;
        let parsed = parse_ips(ips)?;
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let stored = upsert(&mut rewrites, name, parsed, true);
        rewrites.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites)?;
        Ok(stored)
    }

    /// Point several names at the same addresses in one edit. Every name is
    /// validated and the addresses parsed before anything is stored, so a single
    /// bad entry leaves the whole set unchanged. Existing names keep their
    /// enabled state; new ones start enabled.
    pub fn set_many(&self, names: &[String], ips: &[String]) -> Result<(), RewriteError> {
        if names.is_empty() {
            return Err(RewriteError::InvalidName(String::new()));
        }
        let mut normalised = Vec::with_capacity(names.len());
        for name in names {
            normalised.push(validate_name(name)?);
        }
        let parsed = parse_ips(ips)?;

        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        for name in normalised {
            upsert(&mut rewrites, name, parsed.clone(), true);
        }
        rewrites.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites)
    }

    /// Rename a rewrite and replace its addresses, keeping its enabled state.
    /// Used by the dashboard's edit control. The new name and addresses are
    /// validated first; an unknown `old` name is reported.
    pub fn edit(&self, old: &str, new_name: &str, ips: &[String]) -> Result<(), RewriteError> {
        let new_name = validate_name(new_name)?;
        let parsed = parse_ips(ips)?;
        let old = normalise(old);

        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let Some(position) = rewrites.iter().position(|r| r.name == old) else {
            return Err(RewriteError::Unknown(old));
        };
        // Drop the old entry, then re-add it under the new name carrying its
        // enabled state. If the new name happens to collide with another entry,
        // that entry keeps its own state (upsert only replaces the addresses).
        let enabled = rewrites.remove(position).enabled;
        upsert(&mut rewrites, new_name, parsed, enabled);
        rewrites.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites)
    }

    /// Switch a rewrite on or off by name. A disabled entry stays stored but is
    /// left out of the live table. An unknown name is reported.
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), RewriteError> {
        let target = normalise(name);
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let Some(entry) = rewrites.iter_mut().find(|r| r.name == target) else {
            return Err(RewriteError::Unknown(target));
        };
        entry.enabled = enabled;
        self.persist_and_publish(&rewrites)
    }

    /// Remove the rewrite for `name`. Unknown names are reported, so a stale
    /// delete from the UI surfaces rather than silently succeeding.
    pub fn remove(&self, name: &str) -> Result<(), RewriteError> {
        let target = normalise(name);
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let before = rewrites.len();
        rewrites.retain(|r| r.name != target);
        if rewrites.len() == before {
            return Err(RewriteError::Unknown(target));
        }
        self.persist_and_publish(&rewrites)
    }

    /// Write the rewrites to disk and swap them into the live table. The order
    /// matters only for durability: the in-memory table is published even if the
    /// write fails, so the running resolver reflects the edit and the error still
    /// tells the caller it was not saved durably.
    fn persist_and_publish(&self, rewrites: &[Rewrite]) -> Result<(), RewriteError> {
        self.inner.rewriter.set(rewrites);
        write_atomic(
            &self.inner.path,
            &StoredRewrites {
                rewrites: rewrites.to_vec(),
            },
        )
    }
}

/// Validate a host name and return its normalised form.
fn validate_name(name: &str) -> Result<String, RewriteError> {
    if !valid_host(name) {
        return Err(RewriteError::InvalidName(name.to_owned()));
    }
    Ok(normalise(name))
}

/// Parse and de-duplicate the addresses for a rewrite. At least one usable
/// address is required.
fn parse_ips(ips: &[String]) -> Result<Vec<IpAddr>, RewriteError> {
    if ips.is_empty() {
        return Err(RewriteError::NoIps);
    }
    let mut parsed = Vec::with_capacity(ips.len());
    for ip in ips {
        let ip = ip.trim();
        if ip.is_empty() {
            continue;
        }
        let address: IpAddr = ip
            .parse()
            .map_err(|_| RewriteError::InvalidIp(ip.to_owned()))?;
        if !parsed.contains(&address) {
            parsed.push(address);
        }
    }
    if parsed.is_empty() {
        return Err(RewriteError::NoIps);
    }
    Ok(parsed)
}

/// Insert a rewrite, or replace the addresses of one that already has this name.
/// An existing entry keeps its own enabled state; a new one takes
/// `enabled_for_new`. Returns the stored rewrite.
fn upsert(
    rewrites: &mut Vec<Rewrite>,
    name: String,
    ips: Vec<IpAddr>,
    enabled_for_new: bool,
) -> Rewrite {
    match rewrites.iter_mut().find(|r| r.name == name) {
        Some(existing) => {
            existing.ips = ips;
            existing.clone()
        }
        None => {
            let rewrite = Rewrite {
                name,
                ips,
                enabled: enabled_for_new,
            };
            rewrites.push(rewrite.clone());
            rewrite
        }
    }
}

/// Write `stored` to `path` as pretty JSON, atomically (temp file then rename),
/// creating the parent directory if needed.
fn write_atomic(path: &Path, stored: &StoredRewrites) -> Result<(), RewriteError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| RewriteError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let json = serde_json::to_vec_pretty(stored).map_err(|source| RewriteError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| RewriteError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| RewriteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, RewriteStore, Arc<Rewriter>) {
        let dir = tempfile::tempdir().unwrap();
        let rewriter = Arc::new(Rewriter::new());
        let store = RewriteStore::load(dir.path().join("rewrites.json"), rewriter.clone()).unwrap();
        (dir, store, rewriter)
    }

    #[test]
    fn set_is_live_and_normalised() {
        let (_dir, store, rewriter) = store();
        store
            .set("NAS.Home.", &["192.168.20.5".to_owned()])
            .unwrap();

        let ips = rewriter.lookup("nas.home").expect("published live");
        assert_eq!(ips, vec!["192.168.20.5".parse::<IpAddr>().unwrap()]);
        assert_eq!(store.view().rewrites[0].name, "nas.home");
    }

    #[test]
    fn set_replaces_addresses_for_an_existing_name() {
        let (_dir, store, rewriter) = store();
        store.set("nas.home", &["192.168.20.5".to_owned()]).unwrap();
        store
            .set(
                "nas.home",
                &["192.168.20.9".to_owned(), "192.168.20.9".to_owned()],
            )
            .unwrap();

        let ips = rewriter.lookup("nas.home").unwrap();
        assert_eq!(
            ips,
            vec!["192.168.20.9".parse::<IpAddr>().unwrap()],
            "deduped + replaced"
        );
        assert_eq!(store.view().rewrites.len(), 1, "one entry per name");
    }

    #[test]
    fn invalid_name_and_ip_are_rejected() {
        let (_dir, store, _rewriter) = store();
        assert!(matches!(
            store.set("bad name", &["1.2.3.4".to_owned()]),
            Err(RewriteError::InvalidName(_))
        ));
        assert!(matches!(
            store.set("nas.home", &["not-an-ip".to_owned()]),
            Err(RewriteError::InvalidIp(_))
        ));
        assert!(matches!(
            store.set("nas.home", &[]),
            Err(RewriteError::NoIps)
        ));
    }

    #[test]
    fn remove_drops_it_live_and_reports_unknown() {
        let (_dir, store, rewriter) = store();
        store.set("nas.home", &["192.168.20.5".to_owned()]).unwrap();
        assert!(rewriter.lookup("nas.home").is_some());

        store.remove("NAS.home.").unwrap();
        assert!(rewriter.lookup("nas.home").is_none(), "dropped live");
        assert!(matches!(
            store.remove("nas.home"),
            Err(RewriteError::Unknown(_))
        ));
    }

    #[test]
    fn set_many_points_several_names_at_one_ip() {
        let (_dir, store, rewriter) = store();
        store
            .set_many(
                &[
                    "a.home".to_owned(),
                    "b.home".to_owned(),
                    "c.home".to_owned(),
                ],
                &["192.168.20.11".to_owned()],
            )
            .unwrap();

        for name in ["a.home", "b.home", "c.home"] {
            assert_eq!(
                rewriter.lookup(name).unwrap(),
                vec!["192.168.20.11".parse::<IpAddr>().unwrap()],
                "{name} is live"
            );
        }
        assert_eq!(store.view().rewrites.len(), 3);
    }

    #[test]
    fn set_many_rejects_the_whole_batch_on_a_bad_name() {
        let (_dir, store, rewriter) = store();
        assert!(matches!(
            store.set_many(
                &["good.home".to_owned(), "bad name".to_owned()],
                &["1.2.3.4".to_owned()],
            ),
            Err(RewriteError::InvalidName(_))
        ));
        // Nothing was stored — validation happens before any write.
        assert!(rewriter.lookup("good.home").is_none());
        assert_eq!(store.view().rewrites.len(), 0);
    }

    #[test]
    fn edit_renames_and_keeps_enabled_state() {
        let (_dir, store, rewriter) = store();
        store.set("old.home", &["10.0.0.1".to_owned()]).unwrap();
        store.set_enabled("old.home", false).unwrap();

        store
            .edit("old.home", "new.home", &["10.0.0.2".to_owned()])
            .unwrap();

        assert!(rewriter.lookup("old.home").is_none(), "old name is gone");
        // Still disabled after the rename, so not live.
        assert!(rewriter.lookup("new.home").is_none(), "stays disabled");
        let view = store.view();
        assert_eq!(view.rewrites.len(), 1);
        assert_eq!(view.rewrites[0].name, "new.home");
        assert!(!view.rewrites[0].enabled);
        assert_eq!(
            view.rewrites[0].ips,
            vec!["10.0.0.2".parse::<IpAddr>().unwrap()]
        );

        assert!(matches!(
            store.edit("missing.home", "x.home", &["10.0.0.3".to_owned()]),
            Err(RewriteError::Unknown(_))
        ));
    }

    #[test]
    fn set_enabled_toggles_the_live_table() {
        let (_dir, store, rewriter) = store();
        store.set("nas.home", &["192.168.20.5".to_owned()]).unwrap();
        assert!(rewriter.lookup("nas.home").is_some());

        store.set_enabled("NAS.home.", false).unwrap();
        assert!(
            rewriter.lookup("nas.home").is_none(),
            "disabled, falls through"
        );
        assert!(!store.view().rewrites[0].enabled, "still listed, just off");

        store.set_enabled("nas.home", true).unwrap();
        assert!(rewriter.lookup("nas.home").is_some(), "back on");

        assert!(matches!(
            store.set_enabled("missing.home", false),
            Err(RewriteError::Unknown(_))
        ));
    }

    #[test]
    fn rewrites_persist_across_reload() {
        let (dir, store, _rewriter) = store();
        store.set("nas.home", &["192.168.20.5".to_owned()]).unwrap();

        let fresh = Arc::new(Rewriter::new());
        let reopened = RewriteStore::load(dir.path().join("rewrites.json"), fresh.clone()).unwrap();
        assert!(
            fresh.lookup("nas.home").is_some(),
            "persisted across reload"
        );
        assert_eq!(reopened.view().rewrites.len(), 1);
    }
}
