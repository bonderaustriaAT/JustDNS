// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! High-availability sync: keep two (or more) JustDNS nodes converged on the
//! same user-set configuration — active/active, with no fixed primary.
//!
//! The feature is **off** until the user enables it and pairs the nodes. Once
//! paired, each node periodically pulls its peer's [`SyncSnapshot`], merges it,
//! and applies the result through the ordinary store `replace`/`set` methods.
//! Because every node does this and the merge is a commutative last-writer-wins
//! over a total `(timestamp, node)` order, the nodes converge.
//!
//! All sync metadata lives in one ledger (persisted as `/data/ha.json`), not in
//! the individual stores: each config entity is tracked by a stable key
//! `"<collection>/<id>"` with a content hash, the time its current state was
//! established, and the node that established it. Deletes leave a tombstone so a
//! removal on one node is not mistaken for an addition on the other.
//!
//! This module holds the pure, data-only core — the ledger, the snapshot shape
//! and the merge — so it is exhaustively unit-testable without any HTTP or store
//! wiring. The transport, pairing and background loop live alongside it (added in
//! later steps) and the web layer translates between the real store types and the
//! opaque-JSON collections the merge works on.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::ha_crypto;

/// How far a peer's auth timestamp may be from ours before the request is rejected
/// as stale (replay guard). Nodes are expected to keep NTP-synced clocks.
const PEER_AUTH_MAX_SKEW_MS: u64 = 60_000;

/// One collection's entities, keyed by their stable id, as opaque JSON. The web
/// layer serialises each real store type (a `Rewrite`, a `Device`, …) into this
/// form and back, so the merge never needs to know the concrete types. A
/// single-valued collection (the settings) is modelled as one entry under a
/// fixed key.
pub type Collection = BTreeMap<String, Value>;

/// The sync metadata for one entity: enough to order versions across nodes and
/// to tell a live entity from a deleted one, without carrying the content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    /// Stable content hash of the entity's canonical JSON. Meaningless when
    /// `deleted`; kept so a re-add of identical content is not seen as a change.
    pub hash: u64,
    /// When this state (present with `hash`, or deleted) was established, in ms
    /// since the Unix epoch. The primary ordering key for last-writer-wins.
    pub at_ms: u64,
    /// The node that established this state. Breaks ties when two nodes stamp the
    /// exact same `at_ms`, so the order is total and the merge is deterministic.
    pub node: String,
    /// Whether the entity is a tombstone (deleted). A tombstone still travels and
    /// wins over an older live version, so a delete propagates instead of being
    /// resurrected by the other node's stale copy.
    #[serde(default)]
    pub deleted: bool,
}

impl LedgerEntry {
    /// The total order key for last-writer-wins: later time wins; same time is
    /// broken by the higher node id. Deletion is **not** part of the order — a
    /// delete only wins by being stamped later than the edit it supersedes.
    fn order_key(&self) -> (u64, &str) {
        (self.at_ms, self.node.as_str())
    }

    /// Whether `self` is the winner against `other` under the total order. Ties
    /// (identical time and node) keep `self`, which is harmless since equal
    /// `(at_ms, node)` means the same origin version.
    fn wins_over(&self, other: &LedgerEntry) -> bool {
        self.order_key() >= other.order_key()
    }
}

/// The ledger: entity key `"<collection>/<id>"` → its sync metadata.
pub type Ledger = BTreeMap<String, LedgerEntry>;

/// The mergeable view a node exposes to its peer: its id, the live config as
/// keyed collections, and the ledger that orders every entity (including
/// tombstones, whose keys are absent from `collections`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSnapshot {
    /// The exporting node's stable id.
    pub node_id: String,
    /// Collection name → (entity id → entity JSON). Tombstoned keys are absent.
    pub collections: BTreeMap<String, Collection>,
    /// Sync metadata for every tracked key, tombstones included.
    pub ledger: Ledger,
    /// The exporting node's running version, so a peer can display it live and it
    /// refreshes every sync cycle. Empty from an older peer that predates this
    /// field — the puller then keeps the version it already knows.
    #[serde(default)]
    pub version: String,
    /// This node's known cluster roster: node id → how to reach it. Travels with
    /// the snapshot so a node that pulls learns every other member and meshes with
    /// them automatically. Absent from an older peer that predates the mesh.
    #[serde(default)]
    pub roster: BTreeMap<String, RosterMember>,
}

/// One cluster member as the roster records it: the base URL to reach it, when
/// that address was last advertised, and whether the node has been removed. The
/// later `at_ms` wins on merge, so a node's own freshest address prevails over a
/// stale copy another node still holds; a removal is a tombstone (`deleted`) that
/// travels the same way, so forgetting a node is not undone by a peer that still
/// holds its live entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterMember {
    pub url: String,
    pub at_ms: u64,
    /// A removed node: kept as a tombstone so the removal propagates and the mesh
    /// stops reconnecting to it, instead of a live copy on another node resurrecting
    /// it. The `url` is retained only for display; `deleted`/`at_ms` drive the merge.
    #[serde(default)]
    pub deleted: bool,
}

/// The total order that decides a roster collision: the later advertisement wins;
/// an equal `at_ms` prefers a tombstone (a removal is sticky), then the greater
/// url. A total order makes the merge commutative and idempotent, so every node
/// converges on the same roster regardless of the order it folds peers in.
fn roster_order(member: &RosterMember) -> (u64, bool, &str) {
    (member.at_ms, member.deleted, member.url.as_str())
}

/// Union two rosters under [`roster_order`], keeping the winning entry (live
/// address or tombstone) for each node id. A node is authoritative for its own
/// entry (it re-stamps its own address with the current time whenever it is set),
/// so this converges on everyone's latest url — and on a removal once one node
/// tombstones it.
pub fn merge_roster(
    local: &BTreeMap<String, RosterMember>,
    remote: &BTreeMap<String, RosterMember>,
) -> BTreeMap<String, RosterMember> {
    let mut merged = local.clone();
    for (node, member) in remote {
        let take = merged
            .get(node)
            .is_none_or(|existing| roster_order(member) >= roster_order(existing));
        if take {
            merged.insert(node.clone(), member.clone());
        }
    }
    merged
}

/// Extract a human-friendly host label from a node's advertised base URL — the
/// part a person recognises a server by. Strips the scheme, any userinfo, path
/// and the port, so `http://dns-02:8080` reads as `dns-02` and
/// `http://[::1]:8080` as `[::1]`. Returns the trimmed input unchanged when it
/// carries no recognisable host, and an empty string stays empty.
pub fn host_label(url: &str) -> String {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    // Keep only the authority, dropping any path, query or fragment.
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme);
    // Drop userinfo (`user@host`) if present.
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 literal: keep the bracketed address, drop any `:port` after it.
        if let Some((inner, _)) = rest.split_once(']') {
            return format!("[{inner}]");
        }
    }
    // host[:port] — keep the host.
    let host = authority.split(':').next().unwrap_or(authority);
    if host.is_empty() {
        trimmed.to_owned()
    } else {
        host.to_owned()
    }
}

/// A 64-bit FNV-1a hash — small, dependency-free and **stable across builds**
/// (unlike `DefaultHasher`, whose output may change between std versions and so
/// must never be persisted). Used to detect that an entity's content changed.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// The content hash of one entity. `serde_json` serialises a `Value`'s object
/// keys in sorted order (its default `Map` is a `BTreeMap`), so the byte form —
/// and thus the hash — is canonical and order-independent for object fields.
fn hash_entity(entity: &Value) -> u64 {
    match serde_json::to_vec(entity) {
        Ok(bytes) => fnv1a(&bytes),
        // A `Value` always serialises; the arm is only for total safety.
        Err(_) => 0,
    }
}

/// Split an entity key `"<collection>/<id>"` into its parts. Splits on the FIRST
/// `/` only, so an id that itself contains a slash — a device `target` CIDR such
/// as `192.168.30.0/24` — is preserved intact.
fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once('/')
}

/// Join a collection name and entity id into a ledger key.
fn entity_key(collection: &str, id: &str) -> String {
    format!("{collection}/{id}")
}

/// Re-derive the ledger from the current live `collections` and the previous
/// ledger, stamping every change with `now`/`node`:
/// - a key whose content hash differs from the ledger (or is new, or was a
///   tombstone that has reappeared) → a fresh live entry at `now`;
/// - a key the ledger tracked as live but that is now absent → a tombstone at
///   `now`;
/// - an unchanged key keeps its existing metadata (so its timestamp does not
///   creep forward every cycle and lose to a genuinely older peer edit).
///
/// Existing tombstones are carried forward untouched (pruning is separate).
pub fn restamp(
    collections: &BTreeMap<String, Collection>,
    prev: &Ledger,
    node: &str,
    now: u64,
) -> Ledger {
    let mut next: Ledger = Ledger::new();

    for (collection, entities) in collections {
        for (id, entity) in entities {
            let key = entity_key(collection, id);
            let hash = hash_entity(entity);
            match prev.get(&key) {
                Some(old) if !old.deleted && old.hash == hash => {
                    next.insert(key, old.clone());
                }
                _ => {
                    next.insert(
                        key,
                        LedgerEntry {
                            hash,
                            at_ms: now,
                            node: node.to_owned(),
                            deleted: false,
                        },
                    );
                }
            }
        }
    }

    // Keys the ledger knew but that are no longer present: carry existing
    // tombstones, and turn a vanished live entry into a fresh tombstone.
    for (key, old) in prev {
        if next.contains_key(key) {
            continue;
        }
        if old.deleted {
            next.insert(key.clone(), old.clone());
        } else {
            next.insert(
                key.clone(),
                LedgerEntry {
                    hash: old.hash,
                    at_ms: now,
                    node: node.to_owned(),
                    deleted: true,
                },
            );
        }
    }

    next
}

/// Bump **every** live entry in `ledger` to `(at_ms, node)` — used once, on the
/// chosen winner node at pairing, so ordinary last-writer-wins then makes the
/// winner take every genuine collision while entries unique to either node still
/// survive (a lone key has no competitor to lose to). Tombstones are left as they
/// are. This is why the first join needs no special merge mode.
pub fn restamp_all(ledger: &Ledger, at_ms: u64, node: &str) -> Ledger {
    ledger
        .iter()
        .map(|(key, entry)| {
            let bumped = if entry.deleted {
                entry.clone()
            } else {
                LedgerEntry {
                    hash: entry.hash,
                    at_ms,
                    node: node.to_owned(),
                    deleted: false,
                }
            };
            (key.clone(), bumped)
        })
        .collect()
}

/// The result of merging a local snapshot with a peer's: the converged
/// collections to apply through the stores, and the converged ledger to persist.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    /// Collection name → (id → entity JSON) after the merge. Tombstoned keys are
    /// absent (the store `replace` simply won't contain them → they're removed).
    pub collections: BTreeMap<String, Collection>,
    /// The converged ledger (tombstones included), to write back.
    pub ledger: Ledger,
}

/// Merge `local` with `remote` by last-writer-wins per entity over the total
/// `(at_ms, node)` order. For each key either side knows: the later version wins;
/// a winning tombstone drops the entity; a winning live version takes its content
/// from the side that owns that version. The merge is commutative and idempotent,
/// so running it on both nodes converges them.
pub fn merge(local: &SyncSnapshot, remote: &SyncSnapshot) -> Merged {
    let mut ledger: Ledger = Ledger::new();
    let mut collections: BTreeMap<String, Collection> = BTreeMap::new();

    // Every key either ledger mentions.
    let keys: std::collections::BTreeSet<&String> =
        local.ledger.keys().chain(remote.ledger.keys()).collect();

    for key in keys {
        let winner_is_remote = match (local.ledger.get(key), remote.ledger.get(key)) {
            (Some(lm), Some(rm)) => {
                rm.wins_over(lm) && !(lm.wins_over(rm) && lm.order_key() == rm.order_key())
            }
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => continue,
        };

        let (meta, source) = if winner_is_remote {
            (remote.ledger.get(key).unwrap(), remote)
        } else {
            (local.ledger.get(key).unwrap(), local)
        };
        ledger.insert(key.clone(), meta.clone());

        if meta.deleted {
            continue;
        }
        if let Some((collection, id)) = split_key(key) {
            if let Some(entity) = source
                .collections
                .get(collection)
                .and_then(|entities| entities.get(id))
            {
                collections
                    .entry(collection.to_owned())
                    .or_default()
                    .insert(id.to_owned(), entity.clone());
            }
        }
    }

    Merged {
        collections,
        ledger,
    }
}

/// Drop tombstones older than `retain_ms` (measured from `now`), so the ledger
/// does not grow without bound. A tombstone only needs to outlive the window in
/// which a peer might still hold the stale live version; after that it is safe to
/// forget. Live entries are always kept.
pub fn prune_tombstones(ledger: &mut Ledger, now: u64, retain_ms: u64) {
    ledger.retain(|_, entry| !entry.deleted || now.saturating_sub(entry.at_ms) < retain_ms);
}

// ----------------------------------------------------------------------------
// Runtime: identity, pairing state, persistence and the peer HTTP client.
// ----------------------------------------------------------------------------

/// Where the HA state file (`<data>/ha.json`) lives, unless `JUSTDNS_DATA_DIR`
/// overrides it. Defaults to the working directory — the same place the other
/// stores keep their bare-named files — so a local run persists alongside them.
/// Production sets `JUSTDNS_DATA_DIR=/data` explicitly (see docker-compose), so
/// this default only applies to a dev run; hardcoding `/data` here instead made
/// HA the one store that tried to write outside the working directory and failed
/// with a "No such file or directory" warning every save.
const DEFAULT_DATA_DIR: &str = ".";
/// Default seconds between pull-sync cycles, unless `JUSTDNS_SYNC_INTERVAL`
/// overrides it. Kept short so a change on one node shows on the other quickly.
const DEFAULT_SYNC_SECS: u64 = 30;
/// Never sync more often than this, however the interval is set — a floor that
/// keeps a mistyped env var from hammering the peer.
const MIN_SYNC_SECS: u64 = 5;
/// How long to wait on a peer request before giving up.
const PEER_TIMEOUT: Duration = Duration::from_secs(15);
/// Hard cap on a peer response body, so a hostile or broken peer cannot exhaust
/// memory. A full config snapshot is far smaller than this.
const MAX_PEER_BYTES: u64 = 32 * 1024 * 1024;
/// How long a tombstone is kept before pruning (30 days) — long enough that a
/// peer offline for weeks still learns of a deletion instead of resurrecting it.
pub const TOMBSTONE_RETAIN_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// The most peers a node may pair with: a JustDNS HA set is up to four nodes, so
/// each node holds up to three others. A full mesh (every node paired with every
/// other) means one node going down never breaks the sync among the rest.
pub const MAX_PEERS: usize = 3;

/// A node we are meshed with, plus the last we know of it. A JustDNS HA set is up
/// to four nodes, so a node holds up to [`MAX_PEERS`] of these. Most peers are
/// discovered automatically: only the first one is entered by hand (the seed), and
/// the rest are learned from the roster and connected to without user action.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    /// Base URL of the peer's web API, e.g. `http://192.168.20.6:8080`.
    pub url: String,
    /// When this peer was first connected, ms since the epoch.
    pub added_at_ms: u64,
    /// The peer's node id, learned from the connect handshake. The stable identity
    /// a peer is matched on — the url can change, the id does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_node_id: Option<String>,
    /// The peer's version string, refreshed each successful sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_version: Option<String>,
    /// When the last successful sync with this peer completed, ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_ms: Option<u64>,
    /// The error from the most recent failed sync, cleared on the next success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// How a node joins when the user enters a seed address. The choice is applied
/// once, as a single ledger restamp, so ordinary last-writer-wins takes over from
/// the next cycle. Auto-discovered peers never carry a mode — the winner was
/// already settled when the cluster was seeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JoinMode {
    /// Adopt the cluster's configuration: bump our own ledger as far back as it
    /// goes, so every cluster entry wins a collision while our unique entries (with
    /// no rival) still survive. The default for a node joining an established set.
    Adopt,
    /// Make our configuration win: bump our ledger to now, so we take every
    /// collision. Our unique entries survive either way.
    Win,
    /// Join without restamping — used only by the auto-mesh for peers learned from
    /// the roster, where the seed already decided who wins.
    Join,
}

/// The node-local HA state, persisted as `<data>/ha.json`. None of this is ever
/// synced — enablement, the shared key, the peer and the ledger are per-node, so
/// disabling HA on one node can never propagate and switch off the other.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    /// This node's stable id. Breaks LWW ties, so it must never change once set.
    node_id: String,
    /// Whether HA is switched on. Off by default: no sync runs and the peer
    /// endpoints are disabled until the user turns it on.
    #[serde(default)]
    enabled: bool,
    /// The pre-shared sync key set through the wizard. An env key overrides it.
    /// Never returned to any client; only its presence is ever exposed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync_key: Option<String>,
    /// This node's own reachable base URL, as entered by the user (prefilled from
    /// the browser). Advertised in the roster so other nodes can mesh back to us;
    /// without it, peers can pull from us but we are invisible to new joiners.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    self_url: Option<String>,
    /// The meshed peers, once connected (up to [`MAX_PEERS`]).
    #[serde(default)]
    peers: Vec<Peer>,
    /// The known cluster membership: node id → address, merged from every peer's
    /// snapshot by last-writer-wins. Drives the auto-mesh — any member here that we
    /// are not yet peered with, we connect to on the next cycle.
    #[serde(default)]
    roster: BTreeMap<String, RosterMember>,
    /// Per-peer query-log sync watermark: peer node id → the highest `at_ms` of
    /// that peer's events already ingested. The next pull asks for events at or
    /// after it (re-pulling the boundary millisecond, which the store dedups).
    #[serde(default)]
    query_watermarks: BTreeMap<String, i64>,
    /// Legacy single-peer field from before multi-node support. Read from an older
    /// `ha.json` and folded into `peers` on load; never written back.
    #[serde(default, rename = "peer", skip_serializing)]
    legacy_peer: Option<Peer>,
    /// The sync ledger: entity key → metadata, tombstones included.
    #[serde(default)]
    ledger: Ledger,
}

/// A read-only view of HA state for the status API — never carries the key.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaStatus {
    pub node_id: String,
    pub enabled: bool,
    /// Whether a sync key is set (by env or wizard). The key itself is not shown.
    pub key_set: bool,
    /// Whether the key is pinned by an environment variable (so the UI shows it as
    /// managed and does not offer to change it).
    pub key_from_env: bool,
    pub version: String,
    pub interval_secs: u64,
    /// This node's own advertised address, if set (prefilled by the UI otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_url: Option<String>,
    /// `true` once enabled with at least one peer, so the status view can switch
    /// from the wizard to the node list.
    pub paired: bool,
    /// The meshed peers (empty before the first seed). The UI lists them alongside
    /// this node as the cluster roster.
    pub peers: Vec<Peer>,
    /// Count of live (non-tombstone) tracked entities — a cheap health signal.
    pub tracked: usize,
}

/// The connect handshake payload exchanged over `/api/ha/hello`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub node_id: String,
    pub version: String,
}

/// The decrypted contents of an `X-Ha-Auth` token.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthClaim {
    ts_ms: u64,
    node_id: String,
    #[serde(default)]
    self_url: Option<String>,
}

/// A peer's authenticated identity, from a verified `X-Ha-Auth` token. Holding one
/// means the caller proved possession of the shared key.
pub struct PeerAuth {
    pub node_id: String,
    pub self_url: Option<String>,
}

/// Owns this node's HA identity, pairing state and ledger, and speaks to the peer.
/// Cheap to share behind an `Arc`; mutable state sits behind an `RwLock`. The
/// background sync loop and the store wiring live in the web layer, which calls
/// these methods — this type stays free of any store dependency.
pub struct Ha {
    data_dir: PathBuf,
    /// A sync key pinned by `JUSTDNS_SYNC_KEY`, which overrides the wizard key.
    env_key: Option<String>,
    interval: Duration,
    version: String,
    state: RwLock<Persisted>,
}

impl Ha {
    /// Build the HA manager for the running `version`, loading `<data>/ha.json`
    /// and taking the data dir, sync key and interval from the environment. A
    /// missing node id is minted and persisted so it is stable across restarts.
    pub fn from_env(version: &str) -> Self {
        let data_dir = env_nonempty("JUSTDNS_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
        let env_key = env_nonempty("JUSTDNS_SYNC_KEY");
        let interval = env_nonempty("JUSTDNS_SYNC_INTERVAL")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|secs| Duration::from_secs(secs.max(MIN_SYNC_SECS)))
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_SYNC_SECS));

        let path = Self::state_path(&data_dir);
        let mut state = load_state(&path).unwrap_or_default();
        fold_legacy_peer(&mut state);
        if let Some(env_id) = env_nonempty("JUSTDNS_NODE_ID") {
            state.node_id = env_id;
        }
        if state.node_id.trim().is_empty() {
            state.node_id = random_node_id();
        }

        let ha = Self {
            data_dir,
            env_key,
            interval,
            version: version.to_owned(),
            state: RwLock::new(state),
        };
        // Persist so a freshly minted node id (or env override) survives a restart.
        ha.save();
        ha
    }

    fn state_path(data_dir: &Path) -> PathBuf {
        data_dir.join("ha.json")
    }

    /// Persist the current state atomically, so a crash never leaves a half file.
    fn save(&self) {
        let state = self.state.read().expect("ha state not poisoned").clone();
        if let Err(error) = write_state(&Self::state_path(&self.data_dir), &state) {
            warn!(%error, "could not persist HA state");
        }
    }

    /// This node's stable id.
    pub fn node_id(&self) -> String {
        self.state
            .read()
            .expect("ha state not poisoned")
            .node_id
            .clone()
    }

    /// This node's own advertised cluster address (e.g. `http://192.168.20.5:8080`),
    /// if one has been set. The host part is this node's real LAN address, which the
    /// block page uses to point blocked names at the right node without the admin
    /// having to type the IP by hand.
    pub fn self_url(&self) -> Option<String> {
        self.state
            .read()
            .expect("ha state not poisoned")
            .self_url
            .clone()
    }

    /// How often the sync loop runs.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// The effective sync key: the env key if pinned, else the wizard key.
    pub fn effective_key(&self) -> Option<String> {
        if self.env_key.is_some() {
            return self.env_key.clone();
        }
        self.state
            .read()
            .expect("ha state not poisoned")
            .sync_key
            .clone()
    }

    /// The AEAD key derived from the effective sync key (`None` if no key is set).
    fn aead_key(&self) -> Option<ring::aead::LessSafeKey> {
        self.effective_key().map(|key| ha_crypto::derive_key(&key))
    }

    /// Seal a payload under the cluster key so it can be sent to a peer in the clear-
    /// transport without exposing its contents. `None` when no key is set.
    pub fn seal_for_peer(&self, plaintext: &[u8]) -> Option<Vec<u8>> {
        self.aead_key().map(|key| ha_crypto::seal(&key, plaintext))
    }

    /// Open a payload sealed by a peer under the shared key. `None` when no key is
    /// set or the payload does not authenticate (wrong key or tampered).
    pub fn open_from_peer(&self, data: &[u8]) -> Option<Vec<u8>> {
        let key = self.aead_key()?;
        ha_crypto::open(&key, data)
    }

    /// Build the `X-Ha-Auth` header for an outbound peer call: a sealed, time-stamped
    /// token that proves this node holds the shared key and carries its id and address
    /// (so a node that only pulls still becomes known). The key itself never travels.
    fn peer_auth_header(&self) -> Option<String> {
        let (node_id, self_url) = {
            let state = self.state.read().expect("ha state not poisoned");
            (state.node_id.clone(), state.self_url.clone())
        };
        let blob = serde_json::json!({
            "tsMs": now_ms(),
            "nodeId": node_id,
            "selfUrl": self_url,
        })
        .to_string();
        let sealed = self.seal_for_peer(blob.as_bytes())?;
        Some(base64::engine::general_purpose::STANDARD.encode(sealed))
    }

    /// Verify an incoming `X-Ha-Auth` header: base64-decode, open under the shared key,
    /// and check the timestamp is fresh. Returns the caller's authenticated identity,
    /// or `None` when the header is missing, stale, or not sealed under the key.
    pub fn verify_peer_auth(&self, header: &str) -> Option<PeerAuth> {
        let sealed = base64::engine::general_purpose::STANDARD
            .decode(header.trim())
            .ok()?;
        let plaintext = self.open_from_peer(&sealed)?;
        let claim: AuthClaim = serde_json::from_slice(&plaintext).ok()?;
        if now_ms().abs_diff(claim.ts_ms) > PEER_AUTH_MAX_SKEW_MS {
            return None;
        }
        Some(PeerAuth {
            node_id: claim.node_id,
            self_url: claim.self_url,
        })
    }

    /// Whether HA is switched on.
    pub fn enabled(&self) -> bool {
        self.state.read().expect("ha state not poisoned").enabled
    }

    /// Whether the peer endpoints should answer: when HA is enabled and a shared
    /// key is set. Holding the key (proven by a payload that opens under it) is the
    /// actual gate, so this deliberately does **not** require an existing pairing —
    /// the very first
    /// `connect` handshake calls the partner's `/api/ha/hello` before either side
    /// has a peer, so requiring one here would make pairing impossible.
    pub fn peer_api_open(&self) -> bool {
        let state = self.state.read().expect("ha state not poisoned");
        state.enabled && self.effective_key().is_some()
    }

    /// Switch HA on or off. Turning it off leaves the pairing and ledger intact, so
    /// turning it back on resumes without re-pairing.
    pub fn set_enabled(&self, enabled: bool) {
        self.state.write().expect("ha state not poisoned").enabled = enabled;
        self.save();
    }

    /// Set (or clear) the wizard sync key. Ignored in effect while an env key is
    /// pinned, but still stored so removing the env var falls back to it.
    pub fn set_key(&self, key: Option<String>) {
        let key = key.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        });
        self.state.write().expect("ha state not poisoned").sync_key = key;
        self.save();
    }

    /// Whether the key is pinned by the environment (the wizard then shows it as
    /// managed outside the app).
    pub fn key_from_env(&self) -> bool {
        self.env_key.is_some()
    }

    /// The current ledger.
    pub fn ledger(&self) -> Ledger {
        self.state
            .read()
            .expect("ha state not poisoned")
            .ledger
            .clone()
    }

    /// Replace the ledger (after a merge) and persist.
    pub fn set_ledger(&self, ledger: Ledger) {
        self.state.write().expect("ha state not poisoned").ledger = ledger;
        self.save();
    }

    /// The meshed peers.
    pub fn peers(&self) -> Vec<Peer> {
        self.state
            .read()
            .expect("ha state not poisoned")
            .peers
            .clone()
    }

    /// Set (or clear) this node's own advertised address and re-stamp its roster
    /// entry to now, so the fresh address wins over any stale copy a peer holds.
    pub fn set_self_url(&self, url: Option<String>) {
        let url = url.and_then(|value| {
            let trimmed = value.trim().trim_end_matches('/');
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        });
        let mut state = self.state.write().expect("ha state not poisoned");
        state.self_url = url.clone();
        if let Some(url) = url {
            let node = state.node_id.clone();
            state.roster.insert(
                node,
                RosterMember {
                    url,
                    at_ms: now_ms(),
                    deleted: false,
                },
            );
        }
        drop(state);
        self.save();
    }

    /// The known cluster roster: node id → address.
    pub fn roster(&self) -> BTreeMap<String, RosterMember> {
        self.state
            .read()
            .expect("ha state not poisoned")
            .roster
            .clone()
    }

    /// Fold a peer's roster into ours (last-writer-wins per node), persist, and
    /// return the merged roster so the caller can drive the auto-mesh from it.
    pub fn merge_roster_in(
        &self,
        remote: &BTreeMap<String, RosterMember>,
    ) -> BTreeMap<String, RosterMember> {
        let mut state = self.state.write().expect("ha state not poisoned");
        let self_id = state.node_id.clone();
        let mut merged = merge_roster(&state.roster, remote);
        // We are authoritative for our own liveness: never let a peer's stale
        // tombstone of us stand. Restore our own live entry (or drop a bogus one), so
        // a node that was removed and genuinely rejoins re-advertises itself alive.
        if merged.get(&self_id).is_some_and(|member| member.deleted) {
            match state.self_url.clone() {
                Some(url) => {
                    merged.insert(
                        self_id.clone(),
                        RosterMember {
                            url,
                            at_ms: now_ms(),
                            deleted: false,
                        },
                    );
                }
                None => {
                    merged.remove(&self_id);
                }
            }
        }
        // A tombstone learned from the gossip also drops the live peer, so the mesh
        // stops trying to sync with a node another member has removed.
        state.peers.retain(|peer| {
            peer.peer_node_id
                .as_deref()
                .is_none_or(|id| !merged.get(id).is_some_and(|member| member.deleted))
        });
        state.roster = merged.clone();
        drop(state);
        self.save();
        merged
    }

    /// Record a node that announced itself to us (via a pull's headers), so a new
    /// joiner we have never pulled becomes visible and we mesh back to it. Ignores
    /// our own id and an announcement no newer than what we already hold.
    pub fn note_announced(&self, node_id: &str, url: &str) {
        let url = url.trim().trim_end_matches('/');
        if url.is_empty() || node_id.trim().is_empty() {
            return;
        }
        let mut state = self.state.write().expect("ha state not poisoned");
        if node_id == state.node_id {
            return;
        }
        // A passive announce never un-removes a node: one that was deliberately
        // removed rejoins only through an explicit `connect`, not because a stray
        // pull arrived from it. This stops a removal from flapping back as a corpse.
        if state
            .roster
            .get(node_id)
            .is_some_and(|existing| existing.deleted)
        {
            return;
        }
        let now = now_ms();
        let fresher = state
            .roster
            .get(node_id)
            .is_none_or(|existing| now > existing.at_ms);
        if fresher {
            state.roster.insert(
                node_id.to_owned(),
                RosterMember {
                    url: url.to_owned(),
                    at_ms: now,
                    deleted: false,
                },
            );
        }
        drop(state);
        self.save();
    }

    /// This node's own running version.
    pub fn version(&self) -> String {
        self.version.clone()
    }

    /// The wizard-set sync key, for an admin to view and copy when joining another
    /// node. `None` when no key is set, or when the key is pinned by the
    /// environment — the app never echoes an environment secret, and the UI already
    /// shows such a key as managed outside the app.
    pub fn revealable_key(&self) -> Option<String> {
        if self.env_key.is_some() {
            return None;
        }
        self.state
            .read()
            .expect("ha state not poisoned")
            .sync_key
            .clone()
    }

    /// The query-log sync watermark held for a peer, if any — the highest `at_ms`
    /// of that peer's events already ingested.
    pub fn query_watermark(&self, node_id: &str) -> Option<i64> {
        self.state
            .read()
            .expect("ha state not poisoned")
            .query_watermarks
            .get(node_id)
            .copied()
    }

    /// Advance a peer's query-log watermark. Monotonic: a lower value is ignored,
    /// so a late or reordered pull never rewinds it. Persists only on a change.
    pub fn set_query_watermark(&self, node_id: &str, at_ms: i64) {
        let mut state = self.state.write().expect("ha state not poisoned");
        if state
            .query_watermarks
            .get(node_id)
            .is_some_and(|&cur| cur >= at_ms)
        {
            return;
        }
        state.query_watermarks.insert(node_id.to_owned(), at_ms);
        drop(state);
        self.save();
    }

    /// Pull a page of a peer's own query-log events over `/api/ha/queries`,
    /// announcing ourselves as for a snapshot. Generic in the response type so this
    /// module stays free of any query-store dependency — the web layer names the
    /// payload. `since_ms` is inclusive; `limit` caps the page.
    pub fn fetch_queries<T: serde::de::DeserializeOwned>(
        &self,
        peer_url: &str,
        since_ms: i64,
        limit: usize,
    ) -> Result<T, String> {
        let base = endpoint(peer_url, "api/ha/queries");
        let url = format!("{base}?sinceMs={since_ms}&limit={limit}");
        self.get_announced(&url)
    }

    /// Refresh a peer's known version from a snapshot it served (matched by node
    /// id). An older peer that predates the snapshot `version` field sends an empty
    /// string; that is ignored so a known version is never clobbered with nothing.
    /// Persists only when the value actually changes, to keep a steady state quiet.
    pub fn note_peer_version(&self, node_id: &str, version: &str) {
        if version.is_empty() {
            return;
        }
        let mut state = self.state.write().expect("ha state not poisoned");
        let Some(peer) = state
            .peers
            .iter_mut()
            .find(|peer| peer.peer_node_id.as_deref() == Some(node_id))
        else {
            return;
        };
        if peer.peer_version.as_deref() == Some(version) {
            return;
        }
        peer.peer_version = Some(version.to_owned());
        drop(state);
        self.save();
    }

    /// Record the outcome of a sync attempt against one peer (matched by node id):
    /// stamp the success time and clear any error, or store the failure.
    pub fn record_sync(&self, peer_node_id: &str, result: Result<(), String>) {
        let mut state = self.state.write().expect("ha state not poisoned");
        let Some(peer) = state
            .peers
            .iter_mut()
            .find(|peer| peer.peer_node_id.as_deref() == Some(peer_node_id))
        else {
            return;
        };
        match result {
            Ok(()) => {
                peer.last_sync_ms = Some(now_ms());
                peer.last_error = None;
            }
            Err(error) => peer.last_error = Some(error),
        }
        drop(state);
        self.save();
    }

    /// A status view for the UI. Never includes the key.
    pub fn status(&self) -> HaStatus {
        let state = self.state.read().expect("ha state not poisoned");
        let tracked = state.ledger.values().filter(|entry| !entry.deleted).count();
        HaStatus {
            node_id: state.node_id.clone(),
            enabled: state.enabled,
            key_set: self.effective_key().is_some(),
            key_from_env: self.env_key.is_some(),
            version: self.version.clone(),
            interval_secs: self.interval.as_secs(),
            self_url: state.self_url.clone(),
            paired: state.enabled && !state.peers.is_empty(),
            peers: state.peers.clone(),
            tracked,
        }
    }

    /// Whether we already hold a peer with this node id.
    fn has_peer(&self, node_id: &str) -> bool {
        self.state
            .read()
            .expect("ha state not poisoned")
            .peers
            .iter()
            .any(|peer| peer.peer_node_id.as_deref() == Some(node_id))
    }

    /// Connect to a node by base URL and add it to the mesh. Handshakes over
    /// `/api/ha/hello` (announcing ourselves so the partner learns us too) and
    /// stores the peer only on success — requires a key, a free peer slot, a
    /// *different* node id, and that we are not already meshed with it.
    ///
    /// `mode` is applied once to our ledger before the first sync: [`JoinMode::Adopt`]
    /// hands all collisions to the cluster, [`JoinMode::Win`] hands them to us, and
    /// [`JoinMode::Join`] (auto-mesh) leaves the ledger alone. Callers that seed by
    /// hand pass Adopt or Win; the auto-mesh passes Join.
    pub fn connect(&self, url: &str, mode: JoinMode) -> Result<Hello, String> {
        if self.effective_key().is_none() {
            return Err("set a shared key before connecting".to_owned());
        }
        let url = url.trim().trim_end_matches('/').to_owned();
        if url.is_empty() {
            return Err("peer URL is empty".to_owned());
        }
        let hello: Hello = self.get_announced(&endpoint(&url, "api/ha/hello"))?;
        if hello.node_id == self.node_id() {
            return Err("that address is this node itself".to_owned());
        }
        let now = now_ms();
        let mut state = self.state.write().expect("ha state not poisoned");
        if state.peers.len() >= MAX_PEERS
            && !state
                .peers
                .iter()
                .any(|peer| peer.peer_node_id.as_deref() == Some(&hello.node_id))
        {
            return Err(format!(
                "already meshed with the maximum of {MAX_PEERS} peers"
            ));
        }
        // Apply the one-time join restamp before the first merge.
        match mode {
            JoinMode::Adopt => {
                let node = state.node_id.clone();
                state.ledger = restamp_all(&state.ledger, 1, &node);
            }
            JoinMode::Win => {
                let node = state.node_id.clone();
                state.ledger = restamp_all(&state.ledger, now, &node);
            }
            JoinMode::Join => {}
        }
        // Record (or refresh) the peer, matched on its stable node id.
        if let Some(peer) = state
            .peers
            .iter_mut()
            .find(|peer| peer.peer_node_id.as_deref() == Some(&hello.node_id))
        {
            peer.url = url.clone();
            peer.peer_version = Some(hello.version.clone());
        } else {
            state.peers.push(Peer {
                url: url.clone(),
                added_at_ms: now,
                peer_node_id: Some(hello.node_id.clone()),
                peer_version: Some(hello.version.clone()),
                last_sync_ms: None,
                last_error: None,
            });
        }
        // Learn the peer's address into the roster too, so it propagates onward.
        state.roster.insert(
            hello.node_id.clone(),
            RosterMember {
                url,
                at_ms: now,
                deleted: false,
            },
        );
        drop(state);
        self.save();
        Ok(hello)
    }

    /// Automatically mesh with every roster member we can reach but are not yet
    /// peered with, up to the peer cap. Each attempt is isolated: one unreachable
    /// node never stops us connecting to the others. Returns how many new peers
    /// were added. Called by the sync loop after merging a peer's roster.
    pub fn auto_mesh(&self) -> usize {
        let self_id = self.node_id();
        let roster = self.roster();
        let mut added = 0;
        for (node_id, member) in roster {
            if node_id == self_id || member.deleted || self.has_peer(&node_id) {
                continue;
            }
            if self.peers().len() >= MAX_PEERS {
                break;
            }
            if member.url.trim().is_empty() {
                continue;
            }
            match self.connect(&member.url, JoinMode::Join) {
                Ok(_) => added += 1,
                Err(error) => {
                    warn!(%node_id, %error, "auto-mesh: could not reach roster member");
                }
            }
        }
        added
    }

    /// Forget a peer by node id (manual removal): drop it as a peer **and** leave a
    /// roster tombstone, so the auto-mesh stops reconnecting to it and the removal
    /// gossips to the rest of the cluster instead of a peer's live copy resurrecting
    /// it. The ledger is kept, so a genuine re-join (an explicit `connect`) converges
    /// cheaply. Stamped at `now`, so it wins over every existing live entry.
    pub fn disconnect(&self, node_id: &str) {
        self.tombstone_node(node_id, now_ms());
    }

    /// Record a node as removed: write (or advance) its roster tombstone and drop it
    /// from the peers. Never tombstones our own node (we know we are alive), and only
    /// moves the stamp forward, so a replayed or older removal never un-removes a node
    /// that has since been re-added. Used both by a local removal and by a `depart`
    /// announcement learned from another node.
    pub fn tombstone_node(&self, node_id: &str, at_ms: u64) {
        let node_id = node_id.trim();
        if node_id.is_empty() {
            return;
        }
        let mut state = self.state.write().expect("ha state not poisoned");
        if node_id == state.node_id {
            return;
        }
        let supersedes = state
            .roster
            .get(node_id)
            .is_none_or(|existing| at_ms >= existing.at_ms);
        if supersedes {
            let url = state
                .roster
                .get(node_id)
                .map(|existing| existing.url.clone())
                .unwrap_or_default();
            state.roster.insert(
                node_id.to_owned(),
                RosterMember {
                    url,
                    at_ms,
                    deleted: true,
                },
            );
        }
        state
            .peers
            .retain(|peer| peer.peer_node_id.as_deref() != Some(node_id));
        drop(state);
        self.save();
    }

    /// Leave the cluster entirely: forget every peer and the whole roster except
    /// our own entry. The ledger is kept (it converges again if we later rejoin).
    pub fn leave(&self) {
        let mut state = self.state.write().expect("ha state not poisoned");
        state.peers.clear();
        let self_id = state.node_id.clone();
        state.roster.retain(|node_id, _| node_id == &self_id);
        drop(state);
        self.save();
    }

    /// Fetch one peer's current [`SyncSnapshot`] over `/api/ha/snapshot`,
    /// announcing ourselves so the peer meshes back to us.
    pub fn fetch_snapshot(&self, peer_url: &str) -> Result<SyncSnapshot, String> {
        self.get_announced(&endpoint(peer_url, "api/ha/snapshot"))
    }

    /// GET `url` with a sealed `X-Ha-Auth` token (the shared key proves identity
    /// without travelling), then open and parse the **encrypted** response body, with
    /// a timeout and a size cap. Every sensitive peer payload is sealed under the
    /// cluster key, so the sync's secrets never cross the wire in clear.
    fn get_announced<T: DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let auth = self
            .peer_auth_header()
            .ok_or_else(|| "no sync key set".to_owned())?;
        let response = ureq::get(url)
            .timeout(PEER_TIMEOUT)
            .set("X-Ha-Auth", &auth)
            .set("User-Agent", &format!("justdns/{}", self.version))
            .call()
            .map_err(|error| error.to_string())?;
        let mut sealed = Vec::new();
        response
            .into_reader()
            .take(MAX_PEER_BYTES)
            .read_to_end(&mut sealed)
            .map_err(|error| error.to_string())?;
        let body = self
            .open_from_peer(&sealed)
            .ok_or_else(|| "peer response did not authenticate".to_owned())?;
        serde_json::from_slice(&body).map_err(|error| format!("bad peer response: {error}"))
    }

    /// Tell a peer that a node has left (or been removed): POST its id and the
    /// removal time to the peer's `/api/ha/depart`, authenticated with a sealed
    /// `X-Ha-Auth` token. Best-effort — the roster tombstone still reaches any
    /// unreachable peer through the ordinary gossip on a later cycle.
    pub fn notify_depart(&self, peer_url: &str, node_id: &str, at_ms: u64) -> Result<(), String> {
        let auth = self
            .peer_auth_header()
            .ok_or_else(|| "no sync key set".to_owned())?;
        let url = endpoint(peer_url, "api/ha/depart");
        let body = serde_json::json!({ "nodeId": node_id, "atMs": at_ms }).to_string();
        ureq::post(&url)
            .timeout(PEER_TIMEOUT)
            .set("X-Ha-Auth", &auth)
            .set("User-Agent", &format!("justdns/{}", self.version))
            .set("Content-Type", "application/json")
            .send_string(&body)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// Read an environment variable, treating empty/whitespace as unset.
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Join a base URL and a path into one URL with exactly one separating slash.
fn endpoint(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Mint a random, stable node id as 24 hex characters. No external RNG crate: it
/// mixes the wall clock, the process id and a stack address through FNV, which is
/// ample uniqueness for a two-node tiebreak identity.
fn random_node_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let pid = u128::from(std::process::id());
    let stack = &nanos as *const _ as usize as u128;
    let seed = nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (pid << 64)
        ^ stack.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let high = fnv1a(&seed.to_le_bytes());
    let low = fnv1a(&high.to_le_bytes());
    format!("{high:016x}{:08x}", (low & 0xFFFF_FFFF) as u32)
}

/// Migrate a pre-multi-node `ha.json`: fold the single `peer` field into `peers`
/// so an existing pairing survives the upgrade untouched. Idempotent — once the
/// legacy field is gone (it is never written back) this does nothing.
fn fold_legacy_peer(state: &mut Persisted) {
    if let Some(old) = state.legacy_peer.take() {
        if state.peers.is_empty() {
            state.peers.push(old);
        }
    }
}

/// Load the HA state file, or `None` if it is absent or unreadable.
fn load_state(path: &Path) -> Option<Persisted> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(state) => Some(state),
        Err(error) => {
            warn!(%error, "ignoring unreadable ha.json");
            None
        }
    }
}

/// Write the HA state atomically (temp file then rename).
fn write_state(path: &Path, state: &Persisted) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(state)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(tmp, path)
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(node: &str, entries: &[(&str, Value, &LedgerEntry)]) -> SyncSnapshot {
        let mut collections: BTreeMap<String, Collection> = BTreeMap::new();
        let mut ledger = Ledger::new();
        for (key, content, meta) in entries {
            ledger.insert((*key).to_owned(), (*meta).clone());
            if !meta.deleted {
                if let Some((collection, id)) = split_key(key) {
                    collections
                        .entry(collection.to_owned())
                        .or_default()
                        .insert(id.to_owned(), content.clone());
                }
            }
        }
        SyncSnapshot {
            node_id: node.to_owned(),
            collections,
            ledger,
            version: String::new(),
            roster: BTreeMap::new(),
        }
    }

    fn live(at_ms: u64, node: &str, content: &Value) -> LedgerEntry {
        LedgerEntry {
            hash: hash_entity(content),
            at_ms,
            node: node.to_owned(),
            deleted: false,
        }
    }

    fn dead(at_ms: u64, node: &str) -> LedgerEntry {
        LedgerEntry {
            hash: 0,
            at_ms,
            node: node.to_owned(),
            deleted: true,
        }
    }

    #[test]
    fn restamp_detects_add_edit_and_delete() {
        let node = "a";
        let mut cols: BTreeMap<String, Collection> = BTreeMap::new();
        cols.entry("rewrite".into())
            .or_default()
            .insert("nas.home".into(), json!({"ips": ["10.0.0.1"]}));

        // First pass: a brand-new entry is stamped.
        let l1 = restamp(&cols, &Ledger::new(), node, 100);
        assert_eq!(l1["rewrite/nas.home"].at_ms, 100);
        assert!(!l1["rewrite/nas.home"].deleted);

        // Unchanged content keeps its timestamp (no creep).
        let l2 = restamp(&cols, &l1, node, 200);
        assert_eq!(
            l2["rewrite/nas.home"].at_ms, 100,
            "unchanged keeps its time"
        );

        // Edited content restamps to now.
        cols.get_mut("rewrite")
            .unwrap()
            .insert("nas.home".into(), json!({"ips": ["10.0.0.2"]}));
        let l3 = restamp(&cols, &l2, node, 300);
        assert_eq!(l3["rewrite/nas.home"].at_ms, 300, "edit restamps");

        // Removing it leaves a tombstone.
        cols.get_mut("rewrite").unwrap().clear();
        let l4 = restamp(&cols, &l3, node, 400);
        assert!(l4["rewrite/nas.home"].deleted);
        assert_eq!(l4["rewrite/nas.home"].at_ms, 400);
    }

    #[test]
    fn merge_is_commutative_and_idempotent() {
        let v_a = json!({"name": "from-a"});
        let v_b = json!({"name": "from-b"});
        let a = snap(
            "a",
            &[
                ("device/192.168.20.10", v_a.clone(), &live(300, "a", &v_a)),
                (
                    "device/192.168.20.20",
                    json!({"name": "only-a"}),
                    &live(100, "a", &json!({"name": "only-a"})),
                ),
            ],
        );
        let b = snap(
            "b",
            &[
                ("device/192.168.20.10", v_b.clone(), &live(200, "b", &v_b)),
                (
                    "device/192.168.20.30",
                    json!({"name": "only-b"}),
                    &live(100, "b", &json!({"name": "only-b"})),
                ),
            ],
        );

        let ab = merge(&a, &b);
        let ba = merge(&b, &a);
        assert_eq!(ab, ba, "merge is commutative");

        // Collision resolves to the newer (a @300 beats b @200); unique entries
        // from both sides survive.
        let devices = &ab.collections["device"];
        assert_eq!(devices["192.168.20.10"], v_a, "newer edit wins");
        assert!(devices.contains_key("192.168.20.20"), "unique-to-a kept");
        assert!(devices.contains_key("192.168.20.30"), "unique-to-b kept");

        // Idempotent: merging the result with either input changes nothing.
        let merged_snap = SyncSnapshot {
            node_id: "a".into(),
            collections: ab.collections.clone(),
            ledger: ab.ledger.clone(),
            version: String::new(),
            roster: BTreeMap::new(),
        };
        let again = merge(&merged_snap, &b);
        assert_eq!(again.collections, ab.collections, "idempotent collections");
        assert_eq!(again.ledger, ab.ledger, "idempotent ledger");
    }

    #[test]
    fn tombstone_beats_older_live_but_loses_to_newer_edit() {
        let content = json!({"name": "tv"});
        // a deleted the key at 500; b still has an older live copy at 200.
        let a = snap("a", &[("device/d", json!(null), &dead(500, "a"))]);
        let b = snap(
            "b",
            &[("device/d", content.clone(), &live(200, "b", &content))],
        );
        let merged = merge(&a, &b);
        assert!(
            merged
                .collections
                .get("device")
                .is_none_or(|c| !c.contains_key("d")),
            "delete wins over the older live copy"
        );
        assert!(merged.ledger["device/d"].deleted);

        // But a newer re-creation (b @600) beats the tombstone (a @500).
        let b2 = snap(
            "b",
            &[("device/d", content.clone(), &live(600, "b", &content))],
        );
        let merged2 = merge(&a, &b2);
        assert_eq!(
            merged2.collections["device"]["d"], content,
            "newer re-add wins"
        );
        assert!(!merged2.ledger["device/d"].deleted);
    }

    #[test]
    fn equal_timestamps_break_by_node_id() {
        let v_a = json!({"v": "a"});
        let v_b = json!({"v": "b"});
        // Same at_ms; node "b" > node "a", so b wins deterministically.
        let a = snap("a", &[("k/1", v_a.clone(), &live(100, "a", &v_a))]);
        let b = snap("b", &[("k/1", v_b.clone(), &live(100, "b", &v_b))]);
        assert_eq!(merge(&a, &b).collections["k"]["1"], v_b);
        assert_eq!(
            merge(&b, &a).collections["k"]["1"],
            v_b,
            "same winner either way"
        );
    }

    #[test]
    fn winner_bump_makes_the_chosen_node_win_collisions_at_first_join() {
        // Two independently-set-up nodes. Their timestamps are arbitrary; a's
        // "admin" and b's "admin" differ. At pairing, a is chosen winner, so a's
        // ledger is bumped to the pairing epoch T0 > any of b's times.
        let admin_a = json!({"pw": "a-secret"});
        let admin_b = json!({"pw": "b-secret"});
        let a0 = snap(
            "a",
            &[
                ("user/admin", admin_a.clone(), &live(10, "a", &admin_a)),
                (
                    "device/x",
                    json!({"n": "ax"}),
                    &live(10, "a", &json!({"n": "ax"})),
                ),
            ],
        );
        let b = snap(
            "b",
            &[
                ("user/admin", admin_b.clone(), &live(50, "b", &admin_b)),
                (
                    "device/y",
                    json!({"n": "by"}),
                    &live(50, "b", &json!({"n": "by"})),
                ),
            ],
        );

        let t0 = 1_000;
        let a = SyncSnapshot {
            node_id: "a".into(),
            collections: a0.collections.clone(),
            ledger: restamp_all(&a0.ledger, t0, "a"),
            version: String::new(),
            roster: BTreeMap::new(),
        };

        let merged = merge(&a, &b);
        assert_eq!(
            merged.collections["user"]["admin"], admin_a,
            "winner a keeps its admin"
        );
        assert!(
            merged.collections["device"].contains_key("x"),
            "a's unique device kept"
        );
        assert!(
            merged.collections["device"].contains_key("y"),
            "b's unique device kept"
        );
    }

    #[test]
    fn prune_drops_only_old_tombstones() {
        let mut ledger = Ledger::new();
        ledger.insert("k/old".into(), dead(100, "a"));
        ledger.insert("k/new".into(), dead(900, "a"));
        ledger.insert("k/live".into(), live(100, "a", &json!({})));
        prune_tombstones(&mut ledger, 1_000, 500);
        assert!(!ledger.contains_key("k/old"), "old tombstone pruned");
        assert!(ledger.contains_key("k/new"), "recent tombstone kept");
        assert!(ledger.contains_key("k/live"), "live entry always kept");
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("justdns-ha-{}", random_node_id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn ha_with(data_dir: PathBuf, env_key: Option<String>, state: Persisted) -> Ha {
        Ha {
            data_dir,
            env_key,
            interval: Duration::from_secs(30),
            version: "v0.0.0".to_owned(),
            state: RwLock::new(state),
        }
    }

    #[test]
    fn note_peer_version_refreshes_and_ignores_empty() {
        let peer = Peer {
            url: "http://host:8080".to_owned(),
            added_at_ms: 1,
            peer_node_id: Some("p1".to_owned()),
            peer_version: Some("v0.13.1".to_owned()),
            last_sync_ms: None,
            last_error: None,
        };
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "n1".to_owned(),
                peers: vec![peer],
                ..Default::default()
            },
        );

        // A newer version from the peer's snapshot updates the stored value.
        ha.note_peer_version("p1", "v0.14.0");
        assert_eq!(
            ha.peers()[0].peer_version.as_deref(),
            Some("v0.14.0"),
            "an upgraded peer's version is relearned"
        );

        // An empty version (an older peer that predates the field) is ignored
        // rather than clobbering the version we already know.
        ha.note_peer_version("p1", "");
        assert_eq!(
            ha.peers()[0].peer_version.as_deref(),
            Some("v0.14.0"),
            "an empty version never overwrites a known one"
        );

        // An unknown node id is a no-op.
        ha.note_peer_version("nope", "v9.9.9");
        assert_eq!(ha.peers()[0].peer_version.as_deref(), Some("v0.14.0"));
    }

    #[test]
    fn revealable_key_returns_the_wizard_key_but_never_an_env_key() {
        // A wizard-set key is revealable so an admin can copy it to a new node.
        let wizard = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "n1".to_owned(),
                sync_key: Some("wizard-shared-key".to_owned()),
                ..Default::default()
            },
        );
        assert_eq!(
            wizard.revealable_key().as_deref(),
            Some("wizard-shared-key")
        );

        // An env-pinned key is never echoed, even if a stale wizard key lingers.
        let env = ha_with(
            temp_dir(),
            Some("env-secret-key".to_owned()),
            Persisted {
                node_id: "n1".to_owned(),
                sync_key: Some("wizard-shared-key".to_owned()),
                ..Default::default()
            },
        );
        assert_eq!(env.revealable_key(), None, "an env key is not revealed");

        // No key set at all: nothing to reveal.
        let none = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "n1".to_owned(),
                ..Default::default()
            },
        );
        assert_eq!(none.revealable_key(), None);
    }

    #[test]
    fn query_watermark_is_monotonic_per_peer() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "n1".to_owned(),
                ..Default::default()
            },
        );
        assert_eq!(ha.query_watermark("p1"), None, "unset at first");

        ha.set_query_watermark("p1", 100);
        assert_eq!(ha.query_watermark("p1"), Some(100));

        // A later watermark advances; an earlier one is ignored (never rewinds).
        ha.set_query_watermark("p1", 250);
        assert_eq!(ha.query_watermark("p1"), Some(250));
        ha.set_query_watermark("p1", 90);
        assert_eq!(ha.query_watermark("p1"), Some(250), "does not go backwards");

        // Watermarks are kept per peer.
        ha.set_query_watermark("p2", 10);
        assert_eq!(ha.query_watermark("p2"), Some(10));
        assert_eq!(ha.query_watermark("p1"), Some(250));
    }

    #[test]
    fn endpoint_joins_with_one_slash() {
        assert_eq!(
            endpoint("http://host:8080/", "/api/ha/hello"),
            "http://host:8080/api/ha/hello"
        );
        assert_eq!(
            endpoint("http://host:8080", "api/ha/hello"),
            "http://host:8080/api/ha/hello"
        );
    }

    #[test]
    fn node_id_is_24_hex() {
        let id = random_node_id();
        assert_eq!(id.len(), 24);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(random_node_id(), random_node_id(), "ids differ");
    }

    #[test]
    fn env_key_overrides_wizard_key() {
        let ha = ha_with(
            temp_dir(),
            Some("env-key".to_owned()),
            Persisted {
                node_id: "n1".to_owned(),
                sync_key: Some("wizard-key".to_owned()),
                ..Default::default()
            },
        );
        assert_eq!(ha.effective_key().as_deref(), Some("env-key"));
        assert!(ha.key_from_env());
    }

    #[test]
    fn wizard_key_used_when_no_env() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "n1".to_owned(),
                sync_key: Some("wizard-key".to_owned()),
                ..Default::default()
            },
        );
        assert_eq!(ha.effective_key().as_deref(), Some("wizard-key"));
        assert!(!ha.key_from_env());
    }

    fn keyed_node(node_id: &str, self_url: &str, key: &str) -> Ha {
        ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: node_id.to_owned(),
                self_url: Some(self_url.to_owned()),
                sync_key: Some(key.to_owned()),
                ..Default::default()
            },
        )
    }

    #[test]
    fn peer_auth_header_round_trips_under_the_shared_key() {
        let caller = keyed_node("caller", "http://caller:8080", "cluster-key");
        let verifier = keyed_node("verifier", "http://verifier:8080", "cluster-key");
        let header = caller.peer_auth_header().expect("header built");
        let auth = verifier.verify_peer_auth(&header).expect("verifies");
        assert_eq!(auth.node_id, "caller");
        assert_eq!(auth.self_url.as_deref(), Some("http://caller:8080"));
    }

    #[test]
    fn peer_auth_header_from_a_foreign_key_is_rejected() {
        let caller = keyed_node("caller", "http://caller:8080", "cluster-key");
        let stranger = keyed_node("verifier", "http://verifier:8080", "other-key");
        let header = caller.peer_auth_header().expect("header built");
        assert!(stranger.verify_peer_auth(&header).is_none());
        assert!(stranger.verify_peer_auth("not-base64!!").is_none());
    }

    #[test]
    fn a_stale_peer_auth_token_is_rejected() {
        let node = keyed_node("n", "http://n:8080", "cluster-key");
        let stale = serde_json::json!({
            "tsMs": now_ms() - PEER_AUTH_MAX_SKEW_MS - 1_000,
            "nodeId": "n",
            "selfUrl": "http://n:8080",
        })
        .to_string();
        let sealed = node.seal_for_peer(stale.as_bytes()).expect("sealed");
        let header = base64::engine::general_purpose::STANDARD.encode(sealed);
        assert!(node.verify_peer_auth(&header).is_none(), "stale rejected");
    }

    #[test]
    fn peer_api_opens_at_enabled_plus_key_so_pairing_can_start() {
        let dir = temp_dir();
        let ha = ha_with(
            dir,
            None,
            Persisted {
                node_id: "n1".to_owned(),
                ..Default::default()
            },
        );
        assert!(!ha.peer_api_open(), "off and no key ⇒ closed");
        ha.set_enabled(true);
        assert!(!ha.peer_api_open(), "enabled but no key ⇒ closed");
        ha.set_key(Some("k".to_owned()));
        // Open with enabled + key and NO peer: the first connect handshake reaches
        // the partner's /api/ha/hello before either side is paired, so requiring a
        // peer here would deadlock pairing.
        assert!(ha.peer_api_open(), "enabled + key ⇒ open, no peer needed");
        // Pairing then makes the status report as paired.
        ha.state.write().unwrap().peers.push(Peer {
            url: "http://peer".to_owned(),
            added_at_ms: 1,
            peer_node_id: Some("n2".to_owned()),
            ..Default::default()
        });
        assert!(ha.peer_api_open());
        assert!(ha.status().paired);
        // Turning HA off closes the surface again regardless of the stored pairing.
        ha.set_enabled(false);
        assert!(!ha.peer_api_open(), "disabled ⇒ closed");
    }

    #[test]
    fn state_persists_and_reloads() {
        let dir = temp_dir();
        {
            let ha = ha_with(
                dir.clone(),
                None,
                Persisted {
                    node_id: "node-xyz".to_owned(),
                    ..Default::default()
                },
            );
            ha.set_enabled(true);
            ha.set_key(Some("shared".to_owned()));
            let mut ledger = Ledger::new();
            ledger.insert(
                "settings/settings".to_owned(),
                LedgerEntry {
                    hash: 42,
                    at_ms: 7,
                    node: "node-xyz".to_owned(),
                    deleted: false,
                },
            );
            ha.set_ledger(ledger);
        }
        let reloaded = load_state(&Ha::state_path(&dir)).expect("state file written");
        assert_eq!(reloaded.node_id, "node-xyz");
        assert!(reloaded.enabled);
        assert_eq!(reloaded.sync_key.as_deref(), Some("shared"));
        assert_eq!(reloaded.ledger["settings/settings"].hash, 42);
        assert!(!reloaded.sync_key.as_deref().unwrap().is_empty());
    }

    #[test]
    fn key_is_never_in_the_status_view() {
        let ha = ha_with(
            temp_dir(),
            Some("top-secret".to_owned()),
            Persisted {
                node_id: "n1".to_owned(),
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&ha.status()).unwrap();
        assert!(!json.contains("top-secret"), "status must not leak the key");
        assert!(json.contains("\"keySet\":true"));
        assert!(json.contains("\"keyFromEnv\":true"));
    }

    fn member(url: &str, at_ms: u64) -> RosterMember {
        RosterMember {
            url: url.to_owned(),
            at_ms,
            deleted: false,
        }
    }

    fn tombstone(url: &str, at_ms: u64) -> RosterMember {
        RosterMember {
            url: url.to_owned(),
            at_ms,
            deleted: true,
        }
    }

    #[test]
    fn host_label_strips_scheme_port_and_path() {
        assert_eq!(host_label("http://dns-02:8080"), "dns-02");
        assert_eq!(
            host_label("https://dns-01.fritz.box:8443/"),
            "dns-01.fritz.box"
        );
        assert_eq!(host_label("http://192.168.20.6:8080"), "192.168.20.6");
        assert_eq!(host_label("http://[::1]:8080"), "[::1]");
        assert_eq!(host_label("http://[2001:db8::5]:8080/x"), "[2001:db8::5]");
        assert_eq!(host_label("  http://host  "), "host");
        // No scheme, no port: taken as the host as-is.
        assert_eq!(host_label("dns-04"), "dns-04");
        assert_eq!(host_label(""), "");
    }

    #[test]
    fn roster_merge_keeps_the_later_address_and_is_order_independent() {
        let mut a = BTreeMap::new();
        a.insert("n1".to_owned(), member("http://old", 100));
        a.insert("n2".to_owned(), member("http://n2", 100));
        let mut b = BTreeMap::new();
        b.insert("n1".to_owned(), member("http://new", 200)); // newer address for n1
        b.insert("n3".to_owned(), member("http://n3", 100));

        let ab = merge_roster(&a, &b);
        let ba = merge_roster(&b, &a);
        assert_eq!(ab, ba, "roster merge is commutative");
        assert_eq!(ab["n1"].url, "http://new", "later address wins");
        assert_eq!(ab["n2"].url, "http://n2", "unique-to-a kept");
        assert_eq!(ab["n3"].url, "http://n3", "unique-to-b kept");
    }

    #[test]
    fn roster_gossip_converges_across_three_nodes() {
        // Each node initially knows only itself; folding pairwise rosters in any
        // order ends with all three knowing the full membership.
        let mut n1 = BTreeMap::new();
        n1.insert("n1".to_owned(), member("http://n1", 10));
        let mut n2 = BTreeMap::new();
        n2.insert("n2".to_owned(), member("http://n2", 10));
        let mut n3 = BTreeMap::new();
        n3.insert("n3".to_owned(), member("http://n3", 10));

        // n2 pulls n1, n3 pulls n2, then n1 pulls n3 — a ring, not a full mesh.
        let n2 = merge_roster(&n2, &n1);
        let n3 = merge_roster(&n3, &n2);
        let n1 = merge_roster(&n1, &n3);
        assert_eq!(n1.len(), 3, "the full membership reached n1 transitively");
        assert!(["n1", "n2", "n3"].iter().all(|id| n1.contains_key(*id)));
    }

    #[test]
    fn self_url_restamps_our_roster_entry() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.set_self_url(Some("http://me:8080/".to_owned()));
        let roster = ha.roster();
        assert_eq!(roster["me"].url, "http://me:8080", "trailing slash trimmed");
        assert!(roster["me"].at_ms > 0, "stamped with a real time");
        assert_eq!(ha.status().self_url.as_deref(), Some("http://me:8080"));
    }

    #[test]
    fn note_announced_learns_new_nodes_but_not_self() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.note_announced("me", "http://me"); // ignored: that is us
        ha.note_announced("other", "http://other:8080/");
        let roster = ha.roster();
        assert!(!roster.contains_key("me"), "never learns itself this way");
        assert_eq!(roster["other"].url, "http://other:8080");
    }

    #[test]
    fn leave_forgets_peers_and_foreign_roster_but_keeps_self() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.set_self_url(Some("http://me".to_owned()));
        ha.note_announced("other", "http://other");
        ha.state.write().unwrap().peers.push(Peer {
            url: "http://other".to_owned(),
            peer_node_id: Some("other".to_owned()),
            ..Default::default()
        });
        ha.leave();
        assert!(ha.peers().is_empty(), "peers forgotten");
        let roster = ha.roster();
        assert_eq!(roster.len(), 1, "only our own entry remains");
        assert!(roster.contains_key("me"));
    }

    #[test]
    fn legacy_single_peer_migrates_into_peers_vec() {
        // An older ha.json carried a single `peer`; load it and fold it in.
        let dir = temp_dir();
        let path = Ha::state_path(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            r#"{"nodeId":"old","enabled":true,"peer":{"url":"http://p","addedAtMs":5,"peerNodeId":"p"}}"#,
        )
        .unwrap();
        let mut state = load_state(&path).expect("legacy state parses");
        assert!(state.peers.is_empty(), "no peers vec in the old file");
        assert!(state.legacy_peer.is_some(), "the old `peer` field is read");

        fold_legacy_peer(&mut state);
        assert_eq!(state.peers.len(), 1, "legacy peer folded in");
        assert_eq!(state.peers[0].url, "http://p");
        assert_eq!(state.peers[0].peer_node_id.as_deref(), Some("p"));
        assert!(state.legacy_peer.is_none(), "legacy field consumed");

        // Idempotent: a second fold (or a modern file) changes nothing.
        fold_legacy_peer(&mut state);
        assert_eq!(state.peers.len(), 1);
    }

    #[test]
    fn roster_tombstone_wins_over_live_and_is_order_independent() {
        let mut live = BTreeMap::new();
        live.insert("n1".to_owned(), member("http://n1", 100));
        let mut removed = BTreeMap::new();
        removed.insert("n1".to_owned(), tombstone("http://n1", 200));

        let ab = merge_roster(&live, &removed);
        let ba = merge_roster(&removed, &live);
        assert_eq!(ab, ba, "tombstone merge is commutative");
        assert!(
            ab["n1"].deleted,
            "the later tombstone wins over the live entry"
        );

        // A tie prefers the tombstone, so a removal is sticky either way it merges.
        let mut live_tie = BTreeMap::new();
        live_tie.insert("n1".to_owned(), member("http://n1", 100));
        let mut dead_tie = BTreeMap::new();
        dead_tie.insert("n1".to_owned(), tombstone("http://n1", 100));
        assert!(merge_roster(&live_tie, &dead_tie)["n1"].deleted);
        assert!(merge_roster(&dead_tie, &live_tie)["n1"].deleted);
    }

    #[test]
    fn disconnect_tombstones_the_node_and_drops_the_peer() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.note_announced("gone", "http://gone");
        ha.state.write().unwrap().peers.push(Peer {
            url: "http://gone".to_owned(),
            peer_node_id: Some("gone".to_owned()),
            ..Default::default()
        });

        ha.disconnect("gone");
        assert!(ha.peers().is_empty(), "the peer is dropped");
        assert!(
            ha.roster()["gone"].deleted,
            "left as a tombstone so a peer's gossip cannot resurrect it"
        );
    }

    #[test]
    fn a_gossiped_tombstone_drops_the_live_peer() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.note_announced("gone", "http://gone");
        ha.state.write().unwrap().peers.push(Peer {
            url: "http://gone".to_owned(),
            peer_node_id: Some("gone".to_owned()),
            ..Default::default()
        });

        let mut remote = BTreeMap::new();
        remote.insert(
            "gone".to_owned(),
            tombstone("http://gone", now_ms() + 1_000),
        );
        ha.merge_roster_in(&remote);

        assert!(
            ha.peers().is_empty(),
            "a tombstone learned from a peer also drops the live peer"
        );
        assert!(ha.roster()["gone"].deleted);
    }

    #[test]
    fn merge_roster_in_never_lets_a_peer_tombstone_us() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.set_self_url(Some("http://me".to_owned()));

        let mut remote = BTreeMap::new();
        remote.insert("me".to_owned(), tombstone("http://me", now_ms() + 10_000));
        ha.merge_roster_in(&remote);

        assert!(
            !ha.roster()["me"].deleted,
            "we are authoritative for our own liveness — a peer cannot remove us"
        );
    }

    #[test]
    fn a_passive_announce_does_not_resurrect_a_tombstone() {
        let ha = ha_with(
            temp_dir(),
            None,
            Persisted {
                node_id: "me".to_owned(),
                ..Default::default()
            },
        );
        ha.disconnect("gone");
        // A stray pull arriving from the removed node must not bring it back.
        ha.note_announced("gone", "http://gone");
        assert!(
            ha.roster()["gone"].deleted,
            "the tombstone stands; only an explicit connect rejoins"
        );
    }
}
