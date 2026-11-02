// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persistent rewrite store: the user's local name→IP mappings, kept in a
//! JSON file on the data volume and pushed into the live [`Rewriter`].
//!
//! Every edit rewrites the file atomically and swaps a fresh table into the
//! rewriter, so a change in the dashboard answers the very next query without a
//! restart. Names are validated and normalised on the way in, and each address
//! is parsed, so only usable entries are ever stored.

use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::{Alias, Rewrite, Rewriter, normalise, valid_alias_name, valid_host};

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
    #[error("a rewrite needs at least one IP address or address object")]
    NoIps,
    #[error("no rewrite for '{0}'")]
    Unknown(String),
    #[error("'{0}' is not a valid address-object name")]
    InvalidAliasName(String),
    #[error("no address object named '{0}'")]
    UnknownAlias(String),
    #[error("address object '{name}' is still used by: {}", .rewrites.join(", "))]
    AliasInUse { name: String, rewrites: Vec<String> },
}

/// The persisted shape: the rewrites and the reusable address objects they can
/// reference, in one file. Wrapped in a struct (rather than a bare array) so the
/// file can grow more fields without a migration; `aliases` defaults to empty so
/// a file written by an older build still loads.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredRewrites {
    rewrites: Vec<Rewrite>,
    aliases: Vec<Alias>,
}

/// Everything the dashboard's rewrites view needs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewritesView {
    /// Every rewrite, sorted by name.
    pub rewrites: Vec<Rewrite>,
    /// Every address object (alias), sorted by name.
    pub aliases: Vec<Alias>,
}

struct Inner {
    path: PathBuf,
    rewrites: RwLock<Vec<Rewrite>>,
    /// Reusable address objects the rewrites can point at. Locked after
    /// `rewrites` wherever both are held, so the order never varies.
    aliases: RwLock<Vec<Alias>>,
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

        publish(&rewriter, &stored.rewrites, &stored.aliases);
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                rewrites: RwLock::new(stored.rewrites),
                aliases: RwLock::new(stored.aliases),
                rewriter,
            }),
        })
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<Rewrite>> {
        self.inner.rewrites.read().expect("rewrites lock poisoned")
    }

    fn read_aliases(&self) -> std::sync::RwLockReadGuard<'_, Vec<Alias>> {
        self.inner.aliases.read().expect("aliases lock poisoned")
    }

    /// A snapshot of every rewrite and address object for the dashboard.
    pub fn view(&self) -> RewritesView {
        // Rewrites before aliases: the fixed lock order used everywhere.
        let rewrites = self.read().clone();
        let aliases = self.read_aliases().clone();
        RewritesView { rewrites, aliases }
    }

    /// A snapshot of every stored rewrite, for a backup export.
    pub fn snapshot(&self) -> Vec<Rewrite> {
        self.read().clone()
    }

    /// A snapshot of every stored address object, for a backup export.
    pub fn alias_snapshot(&self) -> Vec<Alias> {
        self.read_aliases().clone()
    }

    /// Replace every rewrite, keeping the current address objects. Each entry's
    /// name is validated and normalised, its addresses de-duplicated and its
    /// alias references checked against the existing objects, so a hand-edited
    /// import cannot store junk; one invalid entry rejects the whole set and
    /// nothing is written. Each entry keeps its `enabled` flag. Persists and
    /// swaps the live table in one step.
    pub fn replace(&self, rewrites: Vec<Rewrite>) -> Result<(), RewriteError> {
        // Rewrites before aliases: take the rewrites write lock, then read the
        // objects to validate references against.
        let mut guard = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let aliases = self.read_aliases();
        let clean = clean_rewrite_list(rewrites, &aliases)?;
        *guard = clean;
        self.persist_and_publish(&guard, &aliases)
    }

    /// Replace both the rewrites and the address objects at once, from a restored
    /// backup. The objects are cleaned first so the rewrites can be validated
    /// against them; one invalid entry on either side rejects the whole restore
    /// and nothing is written. Persists and swaps the live table in one step.
    pub fn replace_all(
        &self,
        rewrites: Vec<Rewrite>,
        aliases: Vec<Alias>,
    ) -> Result<(), RewriteError> {
        let clean_aliases = clean_alias_list(aliases)?;
        let clean_rewrites = clean_rewrite_list(rewrites, &clean_aliases)?;
        // Rewrites before aliases: the fixed lock order.
        let mut rguard = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let mut aguard = self.inner.aliases.write().expect("aliases lock poisoned");
        *rguard = clean_rewrites;
        *aguard = clean_aliases;
        self.persist_and_publish(&rguard, &aguard)
    }

    /// Add a rewrite, or replace the addresses of one that already exists. The
    /// name is validated and normalised, and every address is parsed; an entry
    /// with no usable address is refused. Returns the stored rewrite. A replaced
    /// entry keeps its enabled state; a new one starts enabled.
    pub fn set(&self, name: &str, ips: &[String]) -> Result<Rewrite, RewriteError> {
        let name = validate_name(name)?;
        let parsed = parse_ips(ips)?;
        // Rewrites before aliases: write the rewrites, then read the objects the
        // live table resolves against (this entry references none).
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let aliases = self.read_aliases();
        let stored = upsert(&mut rewrites, name, parsed, Vec::new(), true);
        rewrites.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites, &aliases)?;
        Ok(stored)
    }

    /// Point several names at the same addresses and/or address objects in one
    /// edit. Every name is validated, the addresses parsed and each alias
    /// reference checked against the existing objects before anything is stored,
    /// so a single bad entry leaves the whole set unchanged. A rewrite needs at
    /// least one target (an address or an object). Existing names keep their
    /// enabled state; new ones start enabled.
    pub fn set_many(
        &self,
        names: &[String],
        ips: &[String],
        aliases: &[String],
    ) -> Result<(), RewriteError> {
        if names.is_empty() {
            return Err(RewriteError::InvalidName(String::new()));
        }
        let mut normalised = Vec::with_capacity(names.len());
        for name in names {
            normalised.push(validate_name(name)?);
        }
        let parsed = parse_ip_list(ips)?;

        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let alias_guard = self.inner.aliases.read().expect("aliases lock poisoned");
        let alias_refs = validate_aliases(aliases, &alias_guard)?;
        if parsed.is_empty() && alias_refs.is_empty() {
            return Err(RewriteError::NoIps);
        }
        for name in normalised {
            upsert(
                &mut rewrites,
                name,
                parsed.clone(),
                alias_refs.clone(),
                true,
            );
        }
        rewrites.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites, &alias_guard)
    }

    /// Rename a rewrite and replace its addresses and object references, keeping
    /// its enabled state. Used by the dashboard's edit control. The new name,
    /// addresses and references are validated first; an unknown `old` name is
    /// reported.
    pub fn edit(
        &self,
        old: &str,
        new_name: &str,
        ips: &[String],
        aliases: &[String],
    ) -> Result<(), RewriteError> {
        let new_name = validate_name(new_name)?;
        let parsed = parse_ip_list(ips)?;
        let old = normalise(old);

        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let alias_guard = self.inner.aliases.read().expect("aliases lock poisoned");
        let alias_refs = validate_aliases(aliases, &alias_guard)?;
        if parsed.is_empty() && alias_refs.is_empty() {
            return Err(RewriteError::NoIps);
        }
        let Some(position) = rewrites.iter().position(|r| r.name == old) else {
            return Err(RewriteError::Unknown(old));
        };
        // Drop the old entry, then re-add it under the new name carrying its
        // enabled state. If the new name happens to collide with another entry,
        // that entry keeps its own state (upsert only replaces the targets).
        let enabled = rewrites.remove(position).enabled;
        upsert(&mut rewrites, new_name, parsed, alias_refs, enabled);
        rewrites.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites, &alias_guard)
    }

    /// Switch a rewrite on or off by name. A disabled entry stays stored but is
    /// left out of the live table. An unknown name is reported.
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), RewriteError> {
        let target = normalise(name);
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let aliases = self.read_aliases();
        let Some(entry) = rewrites.iter_mut().find(|r| r.name == target) else {
            return Err(RewriteError::Unknown(target));
        };
        entry.enabled = enabled;
        self.persist_and_publish(&rewrites, &aliases)
    }

    /// Remove the rewrite for `name`. Unknown names are reported, so a stale
    /// delete from the UI surfaces rather than silently succeeding.
    pub fn remove(&self, name: &str) -> Result<(), RewriteError> {
        let target = normalise(name);
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let aliases = self.read_aliases();
        let before = rewrites.len();
        rewrites.retain(|r| r.name != target);
        if rewrites.len() == before {
            return Err(RewriteError::Unknown(target));
        }
        self.persist_and_publish(&rewrites, &aliases)
    }

    /// Add an address object, or replace the addresses of one that already
    /// exists. The name is validated and normalised and every address parsed; an
    /// object needs at least one address. Returns the stored object. Changing an
    /// existing object's addresses re-resolves every rewrite that references it.
    pub fn set_alias(&self, name: &str, ips: &[String]) -> Result<Alias, RewriteError> {
        let name = validate_alias_name_input(name)?;
        let parsed = parse_ips(ips)?;
        // Rewrites before aliases: read the rewrites (the table resolves against
        // them), then write the objects.
        let rewrites = self.read();
        let mut aliases = self.inner.aliases.write().expect("aliases lock poisoned");
        let stored = upsert_alias(&mut aliases, name, parsed);
        aliases.sort_by(|a, b| a.name.cmp(&b.name));
        self.persist_and_publish(&rewrites, &aliases)?;
        Ok(stored)
    }

    /// Rename an address object and replace its addresses. The references on
    /// every rewrite that points at the old name follow to the new one, so a
    /// rename never leaves a rewrite dangling. An unknown `old` name is reported.
    pub fn edit_alias(
        &self,
        old: &str,
        new_name: &str,
        ips: &[String],
    ) -> Result<(), RewriteError> {
        let new_name = validate_alias_name_input(new_name)?;
        let parsed = parse_ips(ips)?;
        let old = normalise(old);

        // Rewrites before aliases: the rename cascades onto the rewrites, so both
        // are taken for writing, rewrites first.
        let mut rewrites = self.inner.rewrites.write().expect("rewrites lock poisoned");
        let mut aliases = self.inner.aliases.write().expect("aliases lock poisoned");
        let Some(position) = aliases.iter().position(|a| a.name == old) else {
            return Err(RewriteError::UnknownAlias(old));
        };
        aliases.remove(position);
        upsert_alias(&mut aliases, new_name.clone(), parsed);
        aliases.sort_by(|a, b| a.name.cmp(&b.name));
        if new_name != old {
            for rewrite in rewrites.iter_mut() {
                for reference in rewrite.aliases.iter_mut() {
                    if *reference == old {
                        *reference = new_name.clone();
                    }
                }
                // A rewrite that pointed at both the old and the new name now has
                // the same reference twice; keep one.
                rewrite.aliases.sort();
                rewrite.aliases.dedup();
            }
        }
        self.persist_and_publish(&rewrites, &aliases)
    }

    /// Remove the address object `name`. Refused while any rewrite still points
    /// at it, so an object in use cannot vanish out from under a rewrite; the
    /// error names every referencing rewrite. An unknown name is reported.
    pub fn remove_alias(&self, name: &str) -> Result<(), RewriteError> {
        let target = normalise(name);
        // Rewrites before aliases: read the rewrites to find references, then
        // write the objects.
        let rewrites = self.read();
        let mut aliases = self.inner.aliases.write().expect("aliases lock poisoned");
        let used_by: Vec<String> = rewrites
            .iter()
            .filter(|r| r.aliases.contains(&target))
            .map(|r| r.name.clone())
            .collect();
        if !used_by.is_empty() {
            return Err(RewriteError::AliasInUse {
                name: target,
                rewrites: used_by,
            });
        }
        let before = aliases.len();
        aliases.retain(|a| a.name != target);
        if aliases.len() == before {
            return Err(RewriteError::UnknownAlias(target));
        }
        self.persist_and_publish(&rewrites, &aliases)
    }

    /// Write the rewrites and address objects to disk and swap a freshly resolved
    /// table into the live rewriter. The order matters only for durability: the
    /// in-memory table is published even if the write fails, so the running
    /// resolver reflects the edit and the error still tells the caller it was not
    /// saved durably.
    fn persist_and_publish(
        &self,
        rewrites: &[Rewrite],
        aliases: &[Alias],
    ) -> Result<(), RewriteError> {
        publish(&self.inner.rewriter, rewrites, aliases);
        write_atomic(
            &self.inner.path,
            &StoredRewrites {
                rewrites: rewrites.to_vec(),
                aliases: aliases.to_vec(),
            },
        )
    }
}

/// Build the live table and swap it in: every enabled rewrite's own addresses
/// plus the current addresses of each object it references, de-duplicated. This
/// is where the "change an object once, every rewrite follows" indirection
/// happens — it runs on every edit to either rewrites or objects.
fn publish(rewriter: &Rewriter, rewrites: &[Rewrite], aliases: &[Alias]) {
    let by_alias: HashMap<&str, &[IpAddr]> = aliases
        .iter()
        .map(|alias| (alias.name.as_str(), alias.ips.as_slice()))
        .collect();
    let resolved = rewrites
        .iter()
        .filter(|rewrite| rewrite.enabled)
        .map(|rewrite| {
            let mut ips: Vec<IpAddr> = Vec::new();
            for ip in &rewrite.ips {
                push_unique(&mut ips, *ip);
            }
            for reference in &rewrite.aliases {
                if let Some(alias_ips) = by_alias.get(reference.as_str()) {
                    for ip in *alias_ips {
                        push_unique(&mut ips, *ip);
                    }
                }
            }
            (rewrite.name.clone(), ips)
        });
    rewriter.set_resolved(resolved);
}

/// Append `ip` to `ips` unless it is already there — a small de-duplicating push
/// used while resolving a rewrite's own addresses and its objects' together.
fn push_unique(ips: &mut Vec<IpAddr>, ip: IpAddr) {
    if !ips.contains(&ip) {
        ips.push(ip);
    }
}

/// Validate a host name and return its normalised form.
fn validate_name(name: &str) -> Result<String, RewriteError> {
    if !valid_host(name) {
        return Err(RewriteError::InvalidName(name.to_owned()));
    }
    Ok(normalise(name))
}

/// Validate and normalise an address-object name.
fn validate_alias_name_input(name: &str) -> Result<String, RewriteError> {
    if !valid_alias_name(name) {
        return Err(RewriteError::InvalidAliasName(name.to_owned()));
    }
    Ok(normalise(name))
}

/// Parse and de-duplicate a list of addresses. Empty entries are skipped; an
/// invalid address is reported. May return an empty list (the caller decides
/// whether that is allowed).
fn parse_ip_list(ips: &[String]) -> Result<Vec<IpAddr>, RewriteError> {
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
    Ok(parsed)
}

/// Parse and de-duplicate the addresses where at least one is required (a single
/// rewrite with no object references, and every address object).
fn parse_ips(ips: &[String]) -> Result<Vec<IpAddr>, RewriteError> {
    let parsed = parse_ip_list(ips)?;
    if parsed.is_empty() {
        return Err(RewriteError::NoIps);
    }
    Ok(parsed)
}

/// De-duplicate already-parsed addresses (from a restored backup); at least one
/// is required.
fn dedupe_ips(ips: &[IpAddr]) -> Result<Vec<IpAddr>, RewriteError> {
    let mut out = Vec::with_capacity(ips.len());
    for ip in ips {
        if !out.contains(ip) {
            out.push(*ip);
        }
    }
    if out.is_empty() {
        return Err(RewriteError::NoIps);
    }
    Ok(out)
}

/// Validate and normalise a rewrite's object references: each must name an
/// existing object. Returns the de-duplicated normalised names, order preserved.
fn validate_aliases(references: &[String], aliases: &[Alias]) -> Result<Vec<String>, RewriteError> {
    let mut out: Vec<String> = Vec::with_capacity(references.len());
    for reference in references {
        let name = normalise(reference);
        if name.is_empty() {
            continue;
        }
        if !aliases.iter().any(|alias| alias.name == name) {
            return Err(RewriteError::UnknownAlias(name));
        }
        if !out.contains(&name) {
            out.push(name);
        }
    }
    Ok(out)
}

/// Clean a list of rewrites from an import: validate each name, de-duplicate its
/// addresses, check its object references against `aliases`, and require at least
/// one target. One bad entry rejects the whole list.
fn clean_rewrite_list(
    rewrites: Vec<Rewrite>,
    aliases: &[Alias],
) -> Result<Vec<Rewrite>, RewriteError> {
    let mut clean: Vec<Rewrite> = Vec::with_capacity(rewrites.len());
    for rewrite in rewrites {
        let name = validate_name(&rewrite.name)?;
        let ips = {
            let mut out = Vec::with_capacity(rewrite.ips.len());
            for ip in &rewrite.ips {
                if !out.contains(ip) {
                    out.push(*ip);
                }
            }
            out
        };
        let alias_refs = validate_aliases(&rewrite.aliases, aliases)?;
        if ips.is_empty() && alias_refs.is_empty() {
            return Err(RewriteError::NoIps);
        }
        upsert(&mut clean, name, ips, alias_refs, rewrite.enabled);
    }
    clean.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(clean)
}

/// Clean a list of address objects from an import: validate each name and
/// require at least one address. One bad entry rejects the whole list.
fn clean_alias_list(aliases: Vec<Alias>) -> Result<Vec<Alias>, RewriteError> {
    let mut clean: Vec<Alias> = Vec::with_capacity(aliases.len());
    for alias in aliases {
        let name = validate_alias_name_input(&alias.name)?;
        let ips = dedupe_ips(&alias.ips)?;
        upsert_alias(&mut clean, name, ips);
    }
    clean.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(clean)
}

/// Insert a rewrite, or replace the targets of one that already has this name.
/// An existing entry keeps its own enabled state; a new one takes
/// `enabled_for_new`. Returns the stored rewrite.
fn upsert(
    rewrites: &mut Vec<Rewrite>,
    name: String,
    ips: Vec<IpAddr>,
    aliases: Vec<String>,
    enabled_for_new: bool,
) -> Rewrite {
    match rewrites.iter_mut().find(|r| r.name == name) {
        Some(existing) => {
            existing.ips = ips;
            existing.aliases = aliases;
            existing.clone()
        }
        None => {
            let rewrite = Rewrite {
                name,
                ips,
                aliases,
                enabled: enabled_for_new,
            };
            rewrites.push(rewrite.clone());
            rewrite
        }
    }
}

/// Insert an address object, or replace the addresses of one that already has
/// this name. Returns the stored object.
fn upsert_alias(aliases: &mut Vec<Alias>, name: String, ips: Vec<IpAddr>) -> Alias {
    match aliases.iter_mut().find(|a| a.name == name) {
        Some(existing) => {
            existing.ips = ips;
            existing.clone()
        }
        None => {
            let alias = Alias { name, ips };
            aliases.push(alias.clone());
            alias
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
                &[],
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
                &[],
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
            .edit("old.home", "new.home", &["10.0.0.2".to_owned()], &[])
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
            store.edit("missing.home", "x.home", &["10.0.0.3".to_owned()], &[]),
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
    fn snapshot_and_replace_round_trip_and_reject_junk() {
        let (_dir, store, rewriter) = store();
        store.set("nas.home", &["192.168.20.5".to_owned()]).unwrap();
        store.set_enabled("nas.home", false).unwrap();
        let snapshot = store.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(!snapshot[0].enabled, "snapshot keeps the disabled state");

        // Wipe, then restore: the entry (and its disabled state) comes back, and
        // because it is disabled it is not answered live.
        store.replace(Vec::new()).unwrap();
        assert!(store.view().rewrites.is_empty());
        store.replace(snapshot).unwrap();
        let view = store.view();
        assert_eq!(view.rewrites[0].name, "nas.home");
        assert!(!view.rewrites[0].enabled);
        assert!(rewriter.lookup("nas.home").is_none(), "stays disabled");

        // A restored entry with no addresses is refused, leaving the store as it
        // was.
        let bad = vec![Rewrite {
            name: "x.home".to_owned(),
            ips: Vec::new(),
            aliases: Vec::new(),
            enabled: true,
        }];
        assert!(matches!(store.replace(bad), Err(RewriteError::NoIps)));
        assert_eq!(store.view().rewrites.len(), 1, "unchanged after a reject");
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

    #[test]
    fn rewrite_resolves_through_an_address_object() {
        let (_dir, store, rewriter) = store();
        store
            .set_alias("reverse-proxy", &["192.168.20.9".to_owned()])
            .unwrap();
        // A rewrite pointing only at the object (no own IP) still resolves.
        store
            .set_many(
                &["blog.home".to_owned()],
                &[],
                &["reverse-proxy".to_owned()],
            )
            .unwrap();

        assert_eq!(
            rewriter.lookup("blog.home").unwrap(),
            vec!["192.168.20.9".parse::<IpAddr>().unwrap()],
            "resolves to the object's address"
        );
        let view = store.view();
        assert_eq!(view.aliases.len(), 1);
        assert_eq!(view.rewrites[0].aliases, vec!["reverse-proxy".to_owned()]);
    }

    #[test]
    fn changing_an_object_updates_every_referencing_rewrite() {
        let (_dir, store, rewriter) = store();
        store
            .set_alias("reverse-proxy", &["192.168.20.9".to_owned()])
            .unwrap();
        store
            .set_many(
                &["a.home".to_owned(), "b.home".to_owned()],
                &[],
                &["reverse-proxy".to_owned()],
            )
            .unwrap();

        // Change the object once; both rewrites follow at once.
        store
            .edit_alias(
                "reverse-proxy",
                "reverse-proxy",
                &["192.168.20.50".to_owned()],
            )
            .unwrap();

        let want = vec!["192.168.20.50".parse::<IpAddr>().unwrap()];
        assert_eq!(rewriter.lookup("a.home").unwrap(), want);
        assert_eq!(rewriter.lookup("b.home").unwrap(), want);
    }

    #[test]
    fn rewrite_merges_own_ip_with_object_ip() {
        let (_dir, store, rewriter) = store();
        store.set_alias("v6", &["2001:db8::1".to_owned()]).unwrap();
        store
            .set_many(
                &["dual.home".to_owned()],
                &["192.168.20.5".to_owned()],
                &["v6".to_owned()],
            )
            .unwrap();

        let ips = rewriter.lookup("dual.home").unwrap();
        assert!(ips.contains(&"192.168.20.5".parse::<IpAddr>().unwrap()));
        assert!(ips.contains(&"2001:db8::1".parse::<IpAddr>().unwrap()));
        assert_eq!(ips.len(), 2, "own address and the object's, de-duplicated");
    }

    #[test]
    fn unknown_object_reference_is_rejected() {
        let (_dir, store, rewriter) = store();
        assert!(matches!(
            store.set_many(&["x.home".to_owned()], &[], &["ghost".to_owned()]),
            Err(RewriteError::UnknownAlias(_))
        ));
        assert!(rewriter.lookup("x.home").is_none(), "nothing stored");
    }

    #[test]
    fn a_rewrite_needs_at_least_one_target() {
        let (_dir, store, _rewriter) = store();
        assert!(matches!(
            store.set_many(&["x.home".to_owned()], &[], &[]),
            Err(RewriteError::NoIps)
        ));
    }

    #[test]
    fn removing_an_object_in_use_is_blocked_and_names_the_rewrites() {
        let (_dir, store, _rewriter) = store();
        store.set_alias("proxy", &["10.0.0.9".to_owned()]).unwrap();
        store
            .set_many(
                &["one.home".to_owned(), "two.home".to_owned()],
                &[],
                &["proxy".to_owned()],
            )
            .unwrap();

        match store.remove_alias("proxy") {
            Err(RewriteError::AliasInUse { name, rewrites }) => {
                assert_eq!(name, "proxy");
                assert_eq!(rewrites, vec!["one.home".to_owned(), "two.home".to_owned()]);
            }
            other => panic!("expected AliasInUse, got {other:?}"),
        }
        // The object is still there after the refused delete.
        assert_eq!(store.view().aliases.len(), 1);

        // Once nothing references it, the delete goes through.
        store.remove("one.home").unwrap();
        store.remove("two.home").unwrap();
        store.remove_alias("proxy").unwrap();
        assert!(store.view().aliases.is_empty());
    }

    #[test]
    fn renaming_an_object_follows_on_its_rewrites() {
        let (_dir, store, rewriter) = store();
        store.set_alias("old", &["10.0.0.9".to_owned()]).unwrap();
        store
            .set_many(&["site.home".to_owned()], &[], &["old".to_owned()])
            .unwrap();

        store
            .edit_alias("old", "new", &["10.0.0.10".to_owned()])
            .unwrap();

        let view = store.view();
        assert_eq!(view.aliases[0].name, "new");
        assert_eq!(
            view.rewrites[0].aliases,
            vec!["new".to_owned()],
            "reference followed"
        );
        assert_eq!(
            rewriter.lookup("site.home").unwrap(),
            vec!["10.0.0.10".parse::<IpAddr>().unwrap()]
        );
        // The old object can now be deleted (nothing references it) — it is gone.
        assert!(matches!(
            store.remove_alias("old"),
            Err(RewriteError::UnknownAlias(_))
        ));
    }

    #[test]
    fn objects_and_references_persist_across_reload() {
        let (dir, store, _rewriter) = store();
        store.set_alias("proxy", &["10.0.0.9".to_owned()]).unwrap();
        store
            .set_many(&["site.home".to_owned()], &[], &["proxy".to_owned()])
            .unwrap();

        let fresh = Arc::new(Rewriter::new());
        let reopened = RewriteStore::load(dir.path().join("rewrites.json"), fresh.clone()).unwrap();
        assert_eq!(
            fresh.lookup("site.home").unwrap(),
            vec!["10.0.0.9".parse::<IpAddr>().unwrap()],
            "object reference resolved after reload"
        );
        assert_eq!(reopened.view().aliases.len(), 1);
    }

    #[test]
    fn replace_all_round_trips_rewrites_and_objects() {
        let (_dir, store, rewriter) = store();
        store.set_alias("proxy", &["10.0.0.9".to_owned()]).unwrap();
        store
            .set_many(&["site.home".to_owned()], &[], &["proxy".to_owned()])
            .unwrap();
        let rewrites = store.snapshot();
        let aliases = store.alias_snapshot();

        store.replace_all(Vec::new(), Vec::new()).unwrap();
        assert!(store.view().rewrites.is_empty());
        assert!(store.view().aliases.is_empty());

        store.replace_all(rewrites, aliases).unwrap();
        assert_eq!(
            rewriter.lookup("site.home").unwrap(),
            vec!["10.0.0.9".parse::<IpAddr>().unwrap()],
            "restored and resolved"
        );
    }

    #[test]
    fn invalid_object_name_is_rejected() {
        let (_dir, store, _rewriter) = store();
        assert!(matches!(
            store.set_alias("bad name", &["10.0.0.9".to_owned()]),
            Err(RewriteError::InvalidAliasName(_))
        ));
        // A dotted name reads like a host, not an object, so it is refused too.
        assert!(matches!(
            store.set_alias("proxy.home", &["10.0.0.9".to_owned()]),
            Err(RewriteError::InvalidAliasName(_))
        ));
    }
}
