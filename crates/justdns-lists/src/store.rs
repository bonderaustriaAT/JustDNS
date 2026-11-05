// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The persistent list store: the user's own allow/block entries and their
//! subscriptions to external feeds, merged into the live [`Blocker`].
//!
//! The manual entries and the subscription list live in one JSON file on the
//! data volume, so they survive restarts and updates. Each subscription's
//! fetched feed is cached in its own file beside it, so a restart repopulates
//! the blocker from disk without waiting on the network; a background task
//! refreshes the feeds periodically.
//!
//! Every change rebuilds two merged [`DomainSet`]s — one allow, one block — and
//! swaps them into the shared blocker, so an edit in the dashboard takes effect
//! on the very next query without a restart. The allow set always wins over
//! every block list (see the filter crate), so a whitelisted name is never
//! refused, whatever a subscription says.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use justdns_filter::{BlockCategory, BlockSource, Blocker, DomainSet, parse_rule};
use serde::{Deserialize, Serialize};

use crate::ListKind;
use crate::presets::{Preset, preset, presets};

/// How long a single feed download may take before it is abandoned.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard cap on how much of a feed we read, so a runaway or hostile URL cannot
/// exhaust memory. The largest curated list is well under this.
const MAX_LIST_BYTES: u64 = 64 * 1024 * 1024;

/// Errors from loading, saving or editing the lists.
#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("lists io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("lists parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("'{0}' is not a valid domain")]
    InvalidDomain(String),
    #[error("'{0}' is not a valid list URL (expected http:// or https://)")]
    InvalidUrl(String),
    #[error("no subscription with id '{0}'")]
    UnknownSubscription(String),
    #[error("unknown preset '{0}'")]
    UnknownPreset(String),
}

/// A subscription to an external domain feed. Persisted, so the status fields
/// also survive a restart and the dashboard can show when a feed last updated.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// Stable id, also used as the cache file name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Where the feed is fetched from.
    pub url: String,
    /// Whether the feed allows or blocks the names on it.
    pub kind: ListKind,
    /// Whether the feed currently contributes to the blocker.
    pub enabled: bool,
    /// The catalogue key this came from, if it was switched on as a preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// When the feed was last fetched (ms since the Unix epoch), if ever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fetched_ms: Option<u64>,
    /// How many rules the last successful fetch yielded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_count: Option<usize>,
    /// The error from the last fetch, if it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// One manual list entry: a domain the user added, with an optional free-text
/// note saying why it is there. The blocker only ever reads `domain`; the note
/// is metadata the dashboard shows and never affects what is resolved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListEntry {
    pub domain: String,
    pub note: Option<String>,
}

// An entry with no note is written as a bare string, exactly as every earlier
// version wrote it — so upgrading changes nothing on disk and an older build
// still reads the file. Only an entry that carries a note becomes a
// `{ "domain": ..., "note": ... }` object. Loading accepts either form.
impl Serialize for ListEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.note {
            None => serializer.serialize_str(&self.domain),
            Some(note) => {
                use serde::ser::SerializeStruct;
                let mut entry = serializer.serialize_struct("ListEntry", 2)?;
                entry.serialize_field("domain", &self.domain)?;
                entry.serialize_field("note", note)?;
                entry.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ListEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bare(String),
            Full {
                domain: String,
                #[serde(default)]
                note: Option<String>,
            },
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Bare(domain) => ListEntry { domain, note: None },
            Raw::Full { domain, note } => ListEntry {
                domain,
                note: note.filter(|note| !note.trim().is_empty()),
            },
        })
    }
}

/// The persisted shape: the user's own entries plus their subscriptions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserLists {
    /// Domains the user always allows; a match here is never blocked.
    pub allow: Vec<ListEntry>,
    /// Domains the user always blocks (their own block list).
    pub block: Vec<ListEntry>,
    /// Subscriptions to external feeds.
    pub subscriptions: Vec<Subscription>,
}

/// A preset with the live flags the dashboard needs to render its toggle.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetView {
    #[serde(flatten)]
    pub preset: Preset,
    /// Whether this preset is currently switched on (a subscription exists).
    pub active: bool,
    /// Whether that subscription is enabled (vs. switched on but paused).
    pub enabled: bool,
}

/// A manual entry as the dashboard sees it: always an object, so the SPA has
/// one stable shape whether or not a note is set (the on-disk form, by
/// contrast, stays a bare string when there is no note).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListEntryView {
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl From<&ListEntry> for ListEntryView {
    fn from(entry: &ListEntry) -> Self {
        ListEntryView {
            domain: entry.domain.clone(),
            note: entry.note.clone(),
        }
    }
}

/// Everything the dashboard's list view needs in one payload.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListsView {
    /// The user's own allow entries.
    pub allow: Vec<ListEntryView>,
    /// The user's own block entries.
    pub block: Vec<ListEntryView>,
    /// Subscriptions to external feeds, with their status.
    pub subscriptions: Vec<Subscription>,
    /// The curated catalogue, each marked active/enabled.
    pub presets: Vec<PresetView>,
    /// Total domains on the live allow set (manual + enabled allow feeds).
    pub allow_active: usize,
    /// Total domains on the live custom block set (manual + enabled block feeds).
    pub block_active: usize,
}

/// The outcome of refreshing one subscription.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOutcome {
    pub id: String,
    /// The rule count on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// The error message on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A callback run after every change that alters what is blocked, so the binary
/// can rebuild the live per-client policies. Registered by the binary; the store
/// itself knows nothing of the policy engine.
type OnChange = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    /// The JSON file holding [`UserLists`].
    path: PathBuf,
    /// Directory holding one cached feed file per subscription.
    cache_dir: PathBuf,
    lists: RwLock<UserLists>,
    /// The global default blocker whose allow and custom sets we own and rebuild.
    /// It backs the dashboard's active-list counts and is the base the per-client
    /// policies are layered on; the per-client engine itself is built by the
    /// binary from [`ListStore::resolved`] plus the groups and devices.
    blocker: Arc<Blocker>,
    /// Run after each change that alters what is blocked, to rebuild the policies.
    on_change: RwLock<Option<OnChange>>,
}

/// One reusable block-list object the per-client policies can switch on or off:
/// a subscribed feed, resolved to its domain set. Bundled ads/malware are added
/// by the binary from the filter crate; this covers the user's own feeds.
#[derive(Clone, Debug)]
pub struct ListObject {
    /// Stable id, referenced by a scope's list delta (the subscription id).
    pub id: String,
    /// Display name shown in the dashboard.
    pub label: String,
    /// The domains this object blocks.
    pub set: DomainSet,
    /// Whether it is on in the global default (the subscription's enabled flag).
    pub enabled: bool,
}

/// The user-managed building blocks the policy coordinator folds into the global
/// and per-scope policies: the merged allow set, the user's own manual block set,
/// and each block feed as a switchable object. Bundled ads/malware and the config
/// flags are added by the binary, which owns them.
#[derive(Clone, Debug, Default)]
pub struct ResolvedLists {
    /// The global allow set: manual allow entries plus every enabled allow feed.
    pub allow: DomainSet,
    /// The user's own manual block entries, as one set (always on globally).
    pub own_block: DomainSet,
    /// Every block feed subscription, resolved, with its global enabled flag.
    pub block_feeds: Vec<ListObject>,
}

/// A cheap, cloneable handle to the persistent lists.
#[derive(Clone)]
pub struct ListStore {
    inner: Arc<Inner>,
}

impl ListStore {
    /// Load the lists from `path`, caching feeds under `cache_dir`, and push the
    /// merged sets into `blocker`. A missing file starts empty. Cached feeds are
    /// read from disk so the blocker is populated at once, with no network.
    pub fn load(
        path: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        blocker: Arc<Blocker>,
    ) -> Result<Self, ListError> {
        let path = path.into();
        let cache_dir = cache_dir.into();
        let lists = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| ListError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let seed = UserLists::default();
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(ListError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };

        let store = Self {
            inner: Arc::new(Inner {
                path,
                cache_dir,
                lists: RwLock::new(lists),
                blocker,
                on_change: RwLock::new(None),
            }),
        };
        // Populate the blocker from the manual entries and any cached feeds.
        let lists = store.read();
        store.rebuild(&lists);
        drop(lists);
        Ok(store)
    }

    /// Register the callback run after every change that alters what is blocked,
    /// so the binary can rebuild the live per-client policies. Replaces any
    /// previously registered callback.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) {
        *self.inner.on_change.write().expect("lists lock poisoned") = Some(Arc::new(callback));
    }

    fn fire_on_change(&self) {
        let callback = self
            .inner
            .on_change
            .read()
            .expect("lists lock poisoned")
            .clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    /// The user-managed building blocks for the policy coordinator: the merged
    /// allow set, the user's own manual block set, and each block feed as a
    /// switchable object. Mirrors how [`rebuild`](Self::rebuild) assembles the
    /// global blocker, but returns the pieces as data so the binary can layer the
    /// groups' and devices' deltas on top.
    pub fn resolved(&self) -> ResolvedLists {
        let lists = self.read();

        let mut allow = DomainSet::new();
        for entry in &lists.allow {
            allow.add_rules(&entry.domain);
        }

        let mut own_block = DomainSet::new();
        for entry in &lists.block {
            own_block.add_rules(&entry.domain);
        }

        let mut block_feeds = Vec::new();
        for sub in &lists.subscriptions {
            match sub.kind {
                // Allow feeds merge into the one allow set; they are global-only.
                ListKind::Allow => {
                    if sub.enabled {
                        if let Some(text) = self.read_cache(&sub.id) {
                            allow.add_rules(&text);
                        }
                    }
                }
                // Each block feed is a switchable object a scope can toggle.
                ListKind::Block => {
                    let mut set = DomainSet::new();
                    if let Some(text) = self.read_cache(&sub.id) {
                        set.add_rules(&text);
                    }
                    block_feeds.push(ListObject {
                        id: sub.id.clone(),
                        label: sub.name.clone(),
                        set,
                        enabled: sub.enabled,
                    });
                }
            }
        }

        ResolvedLists {
            allow,
            own_block,
            block_feeds,
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, UserLists> {
        self.inner.lists.read().expect("lists lock poisoned")
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, UserLists> {
        self.inner.lists.write().expect("lists lock poisoned")
    }

    /// A view of everything the dashboard needs: manual entries, subscriptions
    /// and the catalogue with each preset's live state.
    pub fn view(&self) -> ListsView {
        let lists = self.read();
        let presets = presets()
            .iter()
            .map(|preset| {
                let sub = lists
                    .subscriptions
                    .iter()
                    .find(|sub| sub.preset.as_deref() == Some(preset.key));
                PresetView {
                    preset: *preset,
                    active: sub.is_some(),
                    enabled: sub.is_some_and(|sub| sub.enabled),
                }
            })
            .collect();
        ListsView {
            allow: lists.allow.iter().map(ListEntryView::from).collect(),
            block: lists.block.iter().map(ListEntryView::from).collect(),
            subscriptions: lists.subscriptions.clone(),
            presets,
            allow_active: self.inner.blocker.allow_len(),
            block_active: self.inner.blocker.custom_len(),
        }
    }

    /// A snapshot of the user's stored lists — their allow/block entries and
    /// their subscriptions — for a backup export.
    pub fn snapshot(&self) -> UserLists {
        self.read().clone()
    }

    /// Replace the whole stored list set from a restored backup, then persist
    /// and rebuild the live blocker. The feed caches on disk are left as they
    /// are: a restored subscription contributes to the blocker again only once
    /// its feed is next fetched (the periodic refresh, or a manual one).
    pub fn replace(&self, lists: UserLists) -> Result<(), ListError> {
        let mut guard = self.write();
        *guard = lists;
        self.persist(&guard)?;
        self.rebuild(&guard);
        drop(guard);
        self.fire_on_change();
        Ok(())
    }

    /// Add a domain to the user's allow list, with an optional note. Validated
    /// and normalised; adding a name already present keeps it, and only sets the
    /// note when one is given (so a plain re-add never wipes an existing note).
    /// Returns the normalised domain.
    pub fn add_allow(&self, domain: &str, note: Option<&str>) -> Result<String, ListError> {
        self.add_entry(domain, note, Which::Allow)
    }

    /// Remove a domain from the user's allow list.
    pub fn remove_allow(&self, domain: &str) -> Result<(), ListError> {
        self.remove_entry(domain, Which::Allow)
    }

    /// Set or clear the note on an existing allow entry. An empty note clears it.
    pub fn set_allow_note(&self, domain: &str, note: Option<&str>) -> Result<(), ListError> {
        self.set_note(domain, note, Which::Allow)
    }

    /// Add a domain to the user's own block list, with an optional note.
    pub fn add_block(&self, domain: &str, note: Option<&str>) -> Result<String, ListError> {
        self.add_entry(domain, note, Which::Block)
    }

    /// Remove a domain from the user's own block list.
    pub fn remove_block(&self, domain: &str) -> Result<(), ListError> {
        self.remove_entry(domain, Which::Block)
    }

    /// Set or clear the note on an existing block entry. An empty note clears it.
    pub fn set_block_note(&self, domain: &str, note: Option<&str>) -> Result<(), ListError> {
        self.set_note(domain, note, Which::Block)
    }

    fn add_entry(
        &self,
        domain: &str,
        note: Option<&str>,
        which: Which,
    ) -> Result<String, ListError> {
        let normalised =
            parse_rule(domain).ok_or_else(|| ListError::InvalidDomain(domain.to_owned()))?;
        let note = clean_note(note);
        let mut lists = self.write();
        let bucket = which.bucket_mut(&mut lists);
        match bucket.iter_mut().find(|entry| entry.domain == normalised) {
            // A re-add only updates the note when one was supplied, so adding an
            // existing name again from the plain add field never clears its note.
            Some(existing) => {
                if note.is_some() {
                    existing.note = note;
                }
            }
            None => {
                bucket.push(ListEntry {
                    domain: normalised.clone(),
                    note,
                });
                bucket.sort_by(|a, b| a.domain.cmp(&b.domain));
            }
        }
        self.persist(&lists)?;
        self.rebuild(&lists);
        drop(lists);
        self.fire_on_change();
        Ok(normalised)
    }

    fn set_note(&self, domain: &str, note: Option<&str>, which: Which) -> Result<(), ListError> {
        let target = parse_rule(domain).unwrap_or_else(|| domain.trim().to_ascii_lowercase());
        let note = clean_note(note);
        let mut lists = self.write();
        let bucket = which.bucket_mut(&mut lists);
        let entry = bucket
            .iter_mut()
            .find(|entry| entry.domain == target || entry.domain == domain)
            .ok_or_else(|| ListError::InvalidDomain(domain.to_owned()))?;
        entry.note = note;
        // No rebuild: a note never changes what is allowed or blocked.
        self.persist(&lists)?;
        Ok(())
    }

    fn remove_entry(&self, domain: &str, which: Which) -> Result<(), ListError> {
        // Normalise so "Doubleclick.net" removes "doubleclick.net"; fall back to
        // the raw string so an entry that predates stricter parsing can still go.
        let target = parse_rule(domain).unwrap_or_else(|| domain.trim().to_ascii_lowercase());
        let mut lists = self.write();
        let bucket = which.bucket_mut(&mut lists);
        bucket.retain(|entry| entry.domain != target && entry.domain != domain);
        self.persist(&lists)?;
        self.rebuild(&lists);
        drop(lists);
        self.fire_on_change();
        Ok(())
    }

    /// Subscribe to an external feed by URL. Returns the new subscription's id.
    pub fn add_subscription(
        &self,
        name: &str,
        url: &str,
        kind: ListKind,
    ) -> Result<String, ListError> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(ListError::InvalidUrl(url.to_owned()));
        }
        let name = name.trim();
        let name = if name.is_empty() { url } else { name };
        let id = gen_id(url);
        let sub = Subscription {
            id: id.clone(),
            name: name.to_owned(),
            url: url.to_owned(),
            kind,
            enabled: true,
            preset: None,
            last_fetched_ms: None,
            last_count: None,
            last_error: None,
        };
        let mut lists = self.write();
        lists.subscriptions.push(sub);
        self.persist(&lists)?;
        // No rebuild yet: the feed is empty until the first refresh.
        Ok(id)
    }

    /// Remove a subscription and delete its cached feed.
    pub fn remove_subscription(&self, id: &str) -> Result<(), ListError> {
        let mut lists = self.write();
        let before = lists.subscriptions.len();
        lists.subscriptions.retain(|sub| sub.id != id);
        if lists.subscriptions.len() == before {
            return Err(ListError::UnknownSubscription(id.to_owned()));
        }
        self.persist(&lists)?;
        self.rebuild(&lists);
        drop(lists);
        self.remove_cache(id);
        self.fire_on_change();
        Ok(())
    }

    /// Enable or disable a subscription without removing it.
    pub fn set_subscription_enabled(&self, id: &str, enabled: bool) -> Result<(), ListError> {
        let mut lists = self.write();
        let sub = lists
            .subscriptions
            .iter_mut()
            .find(|sub| sub.id == id)
            .ok_or_else(|| ListError::UnknownSubscription(id.to_owned()))?;
        sub.enabled = enabled;
        self.persist(&lists)?;
        self.rebuild(&lists);
        drop(lists);
        self.fire_on_change();
        Ok(())
    }

    /// Switch a curated preset on (creating its subscription) or off (removing
    /// it). Toggling it on when already present just re-enables it.
    pub fn set_preset(&self, key: &str, on: bool) -> Result<(), ListError> {
        let preset = preset(key).ok_or_else(|| ListError::UnknownPreset(key.to_owned()))?;
        if on {
            let id = format!("preset-{key}");
            let mut lists = self.write();
            match lists
                .subscriptions
                .iter_mut()
                .find(|sub| sub.preset.as_deref() == Some(key))
            {
                Some(sub) => sub.enabled = true,
                None => lists.subscriptions.push(Subscription {
                    id,
                    name: preset.name.to_owned(),
                    url: preset.url.to_owned(),
                    kind: preset.kind,
                    enabled: true,
                    preset: Some(key.to_owned()),
                    last_fetched_ms: None,
                    last_count: None,
                    last_error: None,
                }),
            }
            self.persist(&lists)?;
            self.rebuild(&lists);
            drop(lists);
            self.fire_on_change();
            Ok(())
        } else {
            let mut lists = self.write();
            let removed: Vec<String> = lists
                .subscriptions
                .iter()
                .filter(|sub| sub.preset.as_deref() == Some(key))
                .map(|sub| sub.id.clone())
                .collect();
            lists
                .subscriptions
                .retain(|sub| sub.preset.as_deref() != Some(key));
            self.persist(&lists)?;
            self.rebuild(&lists);
            drop(lists);
            for id in removed {
                self.remove_cache(&id);
            }
            self.fire_on_change();
            Ok(())
        }
    }

    /// Fetch every enabled subscription, cache the results, record the status
    /// and rebuild the blocker. Blocking (it does network I/O), so callers on an
    /// async runtime should wrap it in `spawn_blocking`.
    pub fn refresh_all(&self) -> Vec<RefreshOutcome> {
        let targets: Vec<(String, String)> = {
            let lists = self.read();
            lists
                .subscriptions
                .iter()
                .filter(|sub| sub.enabled)
                .map(|sub| (sub.id.clone(), sub.url.clone()))
                .collect()
        };

        // Fetch outside the lock so slow downloads never block an edit.
        let mut results: Vec<(String, Result<usize, String>)> = Vec::with_capacity(targets.len());
        for (id, url) in &targets {
            let result = fetch(url).and_then(|body| {
                let count = count_rules(&body);
                self.write_cache(id, &body).map(|()| count)
            });
            results.push((id.clone(), result));
        }

        let now = now_ms();
        let mut lists = self.write();
        for (id, result) in &results {
            if let Some(sub) = lists.subscriptions.iter_mut().find(|sub| &sub.id == id) {
                sub.last_fetched_ms = Some(now);
                match result {
                    Ok(count) => {
                        sub.last_count = Some(*count);
                        sub.last_error = None;
                    }
                    Err(error) => sub.last_error = Some(error.clone()),
                }
            }
        }
        let _ = self.persist(&lists);
        self.rebuild(&lists);
        drop(lists);
        self.fire_on_change();

        results
            .into_iter()
            .map(|(id, result)| match result {
                Ok(count) => RefreshOutcome {
                    id,
                    count: Some(count),
                    error: None,
                },
                Err(error) => RefreshOutcome {
                    id,
                    count: None,
                    error: Some(error),
                },
            })
            .collect()
    }

    /// Rebuild the allow set and the block sources and swap them into the
    /// blocker. The allow side stays merged, but the block side is kept as one
    /// named source per list — the user's manual list first, then each enabled
    /// block subscription under its own name — so a block can report the exact
    /// list that caught the query. Reads each enabled feed's cache from disk.
    fn rebuild(&self, lists: &UserLists) {
        let mut allow = DomainSet::new();
        for entry in &lists.allow {
            allow.add_rules(&entry.domain);
        }

        let mut sources: Vec<BlockSource> = Vec::new();
        // The user's own manual block list comes first, so an entry they added
        // on purpose is attributed to them rather than to a feed that overlaps.
        let mut own = DomainSet::new();
        for entry in &lists.block {
            own.add_rules(&entry.domain);
        }
        if !own.is_empty() {
            sources.push(BlockSource {
                category: BlockCategory::Custom,
                label: "Eigene Blockliste".to_owned(),
                set: own,
            });
        }

        for sub in &lists.subscriptions {
            if !sub.enabled {
                continue;
            }
            let Some(text) = self.read_cache(&sub.id) else {
                continue;
            };
            match sub.kind {
                // Allow feeds still merge into the one allow set.
                ListKind::Allow => allow.add_rules(&text),
                // Each block feed is its own named source, so the dashboard can
                // name it as the reason for a block.
                ListKind::Block => {
                    let mut set = DomainSet::new();
                    set.add_rules(&text);
                    sources.push(BlockSource {
                        category: BlockCategory::Custom,
                        label: sub.name.clone(),
                        set,
                    });
                }
            }
        }

        self.inner.blocker.set_allow(allow);
        self.inner.blocker.set_block_sources(sources);
    }

    fn persist(&self, lists: &UserLists) -> Result<(), ListError> {
        write_atomic(&self.inner.path, lists)
    }

    fn cache_file(&self, id: &str) -> PathBuf {
        self.inner.cache_dir.join(format!("{id}.txt"))
    }

    fn read_cache(&self, id: &str) -> Option<String> {
        std::fs::read_to_string(self.cache_file(id)).ok()
    }

    fn write_cache(&self, id: &str, body: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.inner.cache_dir).map_err(|e| e.to_string())?;
        let path = self.cache_file(id);
        let tmp = path.with_extension("txt.tmp");
        std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn remove_cache(&self, id: &str) {
        let _ = std::fs::remove_file(self.cache_file(id));
    }
}

/// Which manual bucket an edit touches.
#[derive(Clone, Copy)]
enum Which {
    Allow,
    Block,
}

impl Which {
    fn bucket_mut(self, lists: &mut UserLists) -> &mut Vec<ListEntry> {
        match self {
            Which::Allow => &mut lists.allow,
            Which::Block => &mut lists.block,
        }
    }
}

/// Longest note we keep, in characters, so a stored note cannot grow without
/// bound. The dashboard caps its input to match.
const MAX_NOTE_LEN: usize = 200;

/// Normalise a note: trim it, drop an empty one, and cap its length.
fn clean_note(note: Option<&str>) -> Option<String> {
    note.map(str::trim)
        .filter(|note| !note.is_empty())
        .map(|note| note.chars().take(MAX_NOTE_LEN).collect())
}

/// Count how many lines of `text` parse into a usable rule, without building a
/// set — cheap enough to run on the largest feeds.
fn count_rules(text: &str) -> usize {
    text.lines()
        .filter(|line| parse_rule(line).is_some())
        .count()
}

/// Download `url` as text, with a timeout and a size cap.
fn fetch(url: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .timeout(FETCH_TIMEOUT)
        .call()
        .map_err(|error| error.to_string())?;
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_LIST_BYTES)
        .read_to_string(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(body)
}

/// A short, collision-resistant id for a hand-added subscription.
fn gen_id(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    now_nanos().hash(&mut hasher);
    format!("url-{:016x}", hasher.finish())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Write `lists` to `path` as pretty JSON, atomically (temp file then rename),
/// creating the parent directory if needed.
fn write_atomic(path: &Path, lists: &UserLists) -> Result<(), ListError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| ListError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let json = serde_json::to_vec_pretty(lists).map_err(|source| ListError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| ListError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| ListError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(true))
    }

    /// A store backed by a fresh temp dir, with its own empty blocker.
    fn store() -> (tempfile::TempDir, ListStore, Arc<Blocker>) {
        let dir = tempfile::tempdir().unwrap();
        let blocker = Arc::new(Blocker::new(
            DomainSet::from_rules("doubleclick.net\n"),
            flag(),
            DomainSet::new(),
            flag(),
        ));
        let store = ListStore::load(
            dir.path().join("lists.json"),
            dir.path().join("cache"),
            blocker.clone(),
        )
        .unwrap();
        (dir, store, blocker)
    }

    #[test]
    fn allow_entry_overrides_a_bundled_block_live() {
        let (_dir, store, blocker) = store();
        assert!(blocker.blocks("doubleclick.net"), "blocked to start with");

        store.add_allow("doubleclick.net", None).unwrap();
        assert!(!blocker.blocks("doubleclick.net"), "allow entry wins, live");

        store.remove_allow("doubleclick.net").unwrap();
        assert!(
            blocker.blocks("doubleclick.net"),
            "removing it restores the block"
        );
    }

    #[test]
    fn custom_block_entry_is_live_and_persists() {
        let (dir, store, blocker) = store();
        store.add_block("tracker.test", None).unwrap();
        assert!(blocker.blocks("tracker.test"));
        assert!(blocker.blocks("sub.tracker.test"), "covers sub-domains");

        // Reload from the same files into a fresh blocker: the entry comes back.
        let fresh = Arc::new(Blocker::new(
            DomainSet::new(),
            flag(),
            DomainSet::new(),
            flag(),
        ));
        let reopened = ListStore::load(
            dir.path().join("lists.json"),
            dir.path().join("cache"),
            fresh.clone(),
        )
        .unwrap();
        assert!(fresh.blocks("tracker.test"), "persisted across reload");
        let block = reopened.view().block;
        assert_eq!(block.len(), 1);
        assert_eq!(block[0].domain, "tracker.test");
        assert_eq!(block[0].note, None);
    }

    #[test]
    fn invalid_domain_is_rejected() {
        let (_dir, store, _blocker) = store();
        assert!(matches!(
            store.add_allow("not a domain", None),
            Err(ListError::InvalidDomain(_))
        ));
        assert!(matches!(
            store.add_block("com", None), // a bare TLD is rejected by the parser
            Err(ListError::InvalidDomain(_))
        ));
    }

    #[test]
    fn entries_are_normalised_and_deduplicated() {
        let (_dir, store, _blocker) = store();
        store.add_allow("Example.COM.", None).unwrap();
        store.add_allow("example.com", None).unwrap();
        let allow = store.view().allow;
        assert_eq!(allow.len(), 1);
        assert_eq!(allow[0].domain, "example.com");
    }

    #[test]
    fn a_note_is_stored_and_survives_a_reload() {
        let (dir, store, _blocker) = store();
        store
            .add_block("tracker.test", Some("  blocks the smart-TV phone-home  "))
            .unwrap();
        let block = store.view().block;
        assert_eq!(block[0].domain, "tracker.test");
        assert_eq!(
            block[0].note.as_deref(),
            Some("blocks the smart-TV phone-home")
        );

        // Reopen the same file: the note comes back.
        let fresh = Arc::new(Blocker::new(
            DomainSet::new(),
            flag(),
            DomainSet::new(),
            flag(),
        ));
        let reopened = ListStore::load(
            dir.path().join("lists.json"),
            dir.path().join("cache"),
            fresh,
        )
        .unwrap();
        assert_eq!(
            reopened.view().block[0].note.as_deref(),
            Some("blocks the smart-TV phone-home")
        );
    }

    #[test]
    fn re_adding_keeps_the_note_but_set_note_can_change_or_clear_it() {
        let (_dir, store, _blocker) = store();
        store.add_allow("nas.home", Some("my own box")).unwrap();

        // Re-adding from the plain add field (no note) must not wipe the note.
        store.add_allow("nas.home", None).unwrap();
        assert_eq!(store.view().allow[0].note.as_deref(), Some("my own box"));

        // The edit action replaces the note...
        store.set_allow_note("nas.home", Some("home NAS")).unwrap();
        assert_eq!(store.view().allow[0].note.as_deref(), Some("home NAS"));

        // ...and an empty note clears it.
        store.set_allow_note("nas.home", Some("   ")).unwrap();
        assert_eq!(store.view().allow[0].note, None);
    }

    #[test]
    fn set_note_on_a_missing_entry_is_rejected() {
        let (_dir, store, _blocker) = store();
        assert!(matches!(
            store.set_block_note("nope.test", Some("x")),
            Err(ListError::InvalidDomain(_))
        ));
    }

    #[test]
    fn bare_string_entries_from_an_older_file_still_load() {
        let dir = tempfile::tempdir().unwrap();
        // A lists.json exactly as an older version wrote it: allow/block are
        // plain string arrays, with no note field anywhere.
        std::fs::write(
            dir.path().join("lists.json"),
            r#"{"allow":["nas.home"],"block":["ads.test"],"subscriptions":[]}"#,
        )
        .unwrap();
        let blocker = Arc::new(Blocker::new(
            DomainSet::new(),
            flag(),
            DomainSet::new(),
            flag(),
        ));
        let store = ListStore::load(
            dir.path().join("lists.json"),
            dir.path().join("cache"),
            blocker,
        )
        .unwrap();
        let view = store.view();
        assert_eq!(view.allow[0].domain, "nas.home");
        assert_eq!(view.allow[0].note, None);
        assert_eq!(view.block[0].domain, "ads.test");
    }

    #[test]
    fn snapshot_and_replace_round_trip_and_rebuild_live() {
        let (_dir, store, blocker) = store();
        store.add_block("tracker.test", Some("telemetry")).unwrap();
        store.add_allow("nas.home", None).unwrap();
        let snapshot = store.snapshot();
        assert_eq!(snapshot.block.len(), 1);
        assert_eq!(snapshot.allow.len(), 1);

        // Wipe the store, then restore the snapshot: the entries and their note
        // come back, and the block takes effect on the live blocker again.
        store.replace(UserLists::default()).unwrap();
        assert!(store.view().block.is_empty());
        assert!(!blocker.blocks("tracker.test"), "cleared live");

        store.replace(snapshot).unwrap();
        let view = store.view();
        assert_eq!(view.block[0].domain, "tracker.test");
        assert_eq!(view.block[0].note.as_deref(), Some("telemetry"));
        assert_eq!(view.allow[0].domain, "nas.home");
        assert!(blocker.blocks("tracker.test"), "restored live");
    }

    #[test]
    fn subscriptions_rebuild_from_cache_and_toggle() {
        let (_dir, store, blocker) = store();
        let id = store
            .add_subscription(
                "Test-Feed",
                "https://example.invalid/list.txt",
                ListKind::Block,
            )
            .unwrap();
        // Simulate a successful fetch by writing the cache file directly.
        store.write_cache(&id, "ads.feedtest\n").unwrap();
        // Re-enable to force a rebuild that reads the cache.
        store.set_subscription_enabled(&id, true).unwrap();
        assert!(
            blocker.blocks("ads.feedtest"),
            "feed contributes once cached"
        );

        store.set_subscription_enabled(&id, false).unwrap();
        assert!(!blocker.blocks("ads.feedtest"), "disabling drops it live");

        store.remove_subscription(&id).unwrap();
        assert!(store.view().subscriptions.is_empty());
        assert!(
            store.read_cache(&id).is_none(),
            "cache file is deleted with the subscription"
        );
    }

    #[test]
    fn presets_switch_on_and_off() {
        let (_dir, store, _blocker) = store();
        let view = store.view();
        let light = view
            .presets
            .iter()
            .find(|p| p.preset.key == "hagezi-light")
            .unwrap();
        assert!(!light.active);

        store.set_preset("hagezi-light", true).unwrap();
        let view = store.view();
        let light = view
            .presets
            .iter()
            .find(|p| p.preset.key == "hagezi-light")
            .unwrap();
        assert!(light.active && light.enabled);
        assert_eq!(view.subscriptions.len(), 1);
        assert_eq!(
            view.subscriptions[0].preset.as_deref(),
            Some("hagezi-light")
        );

        store.set_preset("hagezi-light", false).unwrap();
        assert!(store.view().subscriptions.is_empty());

        assert!(matches!(
            store.set_preset("nope", true),
            Err(ListError::UnknownPreset(_))
        ));
    }

    #[test]
    fn add_subscription_rejects_a_non_url() {
        let (_dir, store, _blocker) = store();
        assert!(matches!(
            store.add_subscription("x", "not-a-url", ListKind::Block),
            Err(ListError::InvalidUrl(_))
        ));
    }
}
