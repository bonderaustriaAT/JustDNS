// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Response cache for the forwarding resolver.
//!
//! Each entry keeps the TTL the upstream reported. `moka` drops the entry once
//! that TTL elapses, and a read hands back the stored records with their TTL
//! counted down to the time remaining, as a resolver must. Negative answers
//! (NXDOMAIN / NODATA) are cached the same way.

use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{Name, Record, RecordType};
use moka::Expiry;
use moka::future::Cache;

/// The TTL stamped on a stale answer: short, so the client comes back soon and
/// picks up the refreshed record once it lands.
const STALE_TTL: u32 = 10;

/// The longest serve-stale window any profile uses. Entries are physically
/// retained this far past their DNS TTL regardless of the active profile, so
/// switching to a more aggressive profile can still serve entries cached under
/// a gentler one; the active [`CacheTuning`] decides how much of that window is
/// actually honoured on each lookup.
const MAX_STALE_GRACE: Duration = Duration::from_secs(300);

/// How eagerly the cache serves and refreshes entries. Chosen by the speed
/// profile and published live (behind an [`ArcSwap`]) so a profile change takes
/// effect on the very next lookup without rebuilding the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheTuning {
    /// Serve an expired-but-recent answer while a refresh runs (RFC 8767).
    pub serve_stale: bool,
    /// How long past the DNS TTL a stale answer may still be served. Capped at
    /// [`MAX_STALE_GRACE`] by the physical retention.
    pub stale_grace: Duration,
    /// Refresh a fresh entry in the background once it nears expiry.
    pub prefetch: bool,
    /// Prefetch lead = record TTL / this divisor, clamped to the bounds below.
    pub prefetch_divisor: u32,
    /// Lower and upper clamp on the prefetch lead.
    pub prefetch_min: Duration,
    pub prefetch_max: Duration,
}

impl CacheTuning {
    /// Privacy first: never serve past the TTL and never prefetch, so no answer
    /// is older than the upstream allows and no background queries are made.
    pub fn privacy() -> Self {
        Self {
            serve_stale: false,
            stale_grace: Duration::ZERO,
            prefetch: false,
            prefetch_divisor: 10,
            prefetch_min: Duration::from_secs(1),
            prefetch_max: Duration::from_secs(30),
        }
    }

    /// The balanced default: a short serve-stale window and a gentle prefetch,
    /// matching the behaviour JustDNS shipped before profiles existed.
    pub fn balanced() -> Self {
        Self {
            serve_stale: true,
            stale_grace: Duration::from_secs(60),
            prefetch: true,
            prefetch_divisor: 10,
            prefetch_min: Duration::from_secs(1),
            prefetch_max: Duration::from_secs(30),
        }
    }

    /// Speed first: a long serve-stale window and an eager prefetch that renews
    /// popular names well before they expire, keeping the cache warm the longest.
    pub fn speed() -> Self {
        Self {
            serve_stale: true,
            stale_grace: MAX_STALE_GRACE,
            prefetch: true,
            prefetch_divisor: 4,
            prefetch_min: Duration::from_secs(2),
            prefetch_max: Duration::from_secs(60),
        }
    }

    /// How long before a fresh entry expires to start refreshing it, so a
    /// popular name is renewed before it ever goes stale. A share of the
    /// record's own TTL, clamped so very short TTLs are not thrashed and very
    /// long ones are not refreshed absurdly early.
    fn prefetch_lead(&self, ttl: Duration) -> Duration {
        (ttl / self.prefetch_divisor.max(1)).clamp(self.prefetch_min, self.prefetch_max)
    }
}

impl Default for CacheTuning {
    fn default() -> Self {
        Self::balanced()
    }
}

/// A cheap, cloneable live handle to the active [`CacheTuning`]. Stored in the
/// config layer and read by the cache on every lookup.
pub type SharedTuning = Arc<ArcSwap<CacheTuning>>;

/// Build a fresh [`SharedTuning`] starting from `tuning`. Lets the binary seed
/// and later swap the cache's tuning without depending on `arc-swap` directly.
pub fn shared_tuning(tuning: CacheTuning) -> SharedTuning {
    Arc::new(ArcSwap::from_pointee(tuning))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub name: Name,
    pub record_type: RecordType,
}

impl CacheKey {
    pub fn new(name: &Name, record_type: RecordType) -> Self {
        // Lower-casing up front keeps `Hash`/`Eq` consistent regardless of the
        // casing a client used in the query.
        Self {
            name: name.to_lowercase(),
            record_type,
        }
    }
}

#[derive(Clone, Debug)]
enum EntryKind {
    Positive(Vec<Record>),
    Negative(ResponseCode),
}

#[derive(Clone, Debug)]
pub struct CachedEntry {
    kind: EntryKind,
    ttl: Duration,
    created_at: Instant,
}

/// A cache hit: the stored records with their TTL counted down, or the cached
/// negative response code.
#[derive(Clone, Debug, PartialEq)]
pub enum CachedResponse {
    Positive(Vec<Record>),
    Negative(ResponseCode),
}

/// A cache lookup result: the response to serve, plus whether the caller should
/// kick off a background refresh — because the entry is stale (within the
/// serve-stale window) or fresh but close enough to expiry to prefetch now.
#[derive(Clone, Debug, PartialEq)]
pub struct CacheHit {
    pub response: CachedResponse,
    pub needs_refresh: bool,
}

impl CachedEntry {
    pub fn positive(records: Vec<Record>, ttl: Duration) -> Self {
        Self::new(EntryKind::Positive(records), ttl)
    }

    pub fn negative(response_code: ResponseCode, ttl: Duration) -> Self {
        Self::new(EntryKind::Negative(response_code), ttl)
    }

    fn new(kind: EntryKind, ttl: Duration) -> Self {
        Self {
            kind,
            ttl,
            created_at: Instant::now(),
        }
    }

    /// Render the stored answer with every record's TTL set to `ttl` seconds.
    fn render(&self, ttl: u32) -> CachedResponse {
        match &self.kind {
            EntryKind::Positive(records) => {
                let records = records
                    .iter()
                    .cloned()
                    .map(|mut record| {
                        record.ttl = ttl;
                        record
                    })
                    .collect();
                CachedResponse::Positive(records)
            }
            EntryKind::Negative(code) => CachedResponse::Negative(*code),
        }
    }

    /// Classify the entry at `now` under the active `tuning`: a fresh hit (TTL
    /// counted down, flagged for prefetch once it nears expiry if the profile
    /// prefetches), a stale-but-serveable hit inside the profile's serve-stale
    /// window (always flagged for refresh), or `None` once no hit applies.
    fn lookup(&self, now: Instant, tuning: &CacheTuning) -> Option<CacheHit> {
        let elapsed = now.saturating_duration_since(self.created_at);
        if elapsed < self.ttl {
            let remaining = self.ttl - elapsed;
            let ttl = remaining.as_secs().min(u64::from(u32::MAX)) as u32;
            Some(CacheHit {
                response: self.render(ttl),
                needs_refresh: tuning.prefetch && remaining <= tuning.prefetch_lead(self.ttl),
            })
        } else if tuning.serve_stale && elapsed < self.ttl + tuning.stale_grace {
            Some(CacheHit {
                response: self.render(STALE_TTL),
                needs_refresh: true,
            })
        } else {
            None
        }
    }
}

struct TtlExpiry;

impl Expiry<CacheKey, CachedEntry> for TtlExpiry {
    fn expire_after_create(
        &self,
        _key: &CacheKey,
        value: &CachedEntry,
        _created_at: Instant,
    ) -> Option<Duration> {
        // Hold the entry past its DNS TTL by the widest serve-stale window any
        // profile uses, so an expired-but-recent answer is still there to serve
        // while it refreshes — even after a switch to a more aggressive profile.
        Some(value.ttl + MAX_STALE_GRACE)
    }
}

/// The response cache. Cloning shares the one underlying store (`moka` keeps it
/// behind an `Arc`), so the DNS pipeline and the web layer's "clear cache"
/// action can hold their own handles to the same cache.
#[derive(Clone)]
pub struct DnsCache {
    inner: Cache<CacheKey, CachedEntry>,
    /// The live serve/refresh behaviour, read on every lookup so a profile
    /// change takes effect at once. Cloning the cache shares this handle.
    tuning: SharedTuning,
}

impl DnsCache {
    /// A cache with the balanced tuning, owning its own (unshared) tuning handle.
    /// Used where there is nothing to reconfigure it live — tests, mainly.
    pub fn new(max_capacity: u64) -> Self {
        Self::with_tuning(
            max_capacity,
            Arc::new(ArcSwap::from_pointee(CacheTuning::balanced())),
        )
    }

    /// A cache driven by a shared, swappable [`CacheTuning`], so the config
    /// layer can retune it live when the speed profile changes.
    pub fn with_tuning(max_capacity: u64, tuning: SharedTuning) -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(max_capacity)
                .expire_after(TtlExpiry)
                .build(),
            tuning,
        }
    }

    pub async fn insert(&self, key: CacheKey, entry: CachedEntry) {
        self.inner.insert(key, entry).await;
    }

    pub async fn get(&self, key: &CacheKey) -> Option<CacheHit> {
        let tuning = self.tuning.load();
        self.inner.get(key).await?.lookup(Instant::now(), &tuning)
    }

    /// Drop every cached entry. The next query for each name goes upstream (or to
    /// a local rewrite) again. Used by the dashboard's "clear DNS cache" action.
    pub fn clear(&self) {
        self.inner.invalidate_all();
    }

    /// The approximate number of cached entries, for the diagnostics view. The
    /// count is eventually consistent (moka applies evictions lazily), which is
    /// fine for a status readout.
    pub fn len(&self) -> u64 {
        self.inner.entry_count()
    }

    /// Whether the cache currently holds no entries (approximate, see [`len`]).
    ///
    /// [`len`]: Self::len
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::rr::RData;
    use hickory_proto::rr::rdata::A;
    use std::net::Ipv4Addr;

    fn a_record(ttl: u32) -> Record {
        let name = Name::from_ascii("example.com.").unwrap();
        Record::from_rdata(name, ttl, RData::A(A(Ipv4Addr::new(93, 184, 216, 34))))
    }

    fn key() -> CacheKey {
        CacheKey::new(&Name::from_ascii("example.com.").unwrap(), RecordType::A)
    }

    /// A positive entry with its creation pinned to `created_at`, so time-based
    /// classification can be tested deterministically.
    fn entry_at(ttl: u64, created_at: Instant) -> CachedEntry {
        let mut entry = CachedEntry::positive(vec![a_record(ttl as u32)], Duration::from_secs(ttl));
        entry.created_at = created_at;
        entry
    }

    #[test]
    fn fresh_entry_counts_down_without_refresh() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0);

        match entry.lookup(t0 + Duration::from_secs(100), &CacheTuning::balanced()) {
            Some(CacheHit {
                response: CachedResponse::Positive(records),
                needs_refresh,
            }) => {
                assert_eq!(records[0].ttl, 200, "TTL counted down");
                assert!(!needs_refresh, "far from expiry, no prefetch");
            }
            other => panic!("expected a fresh positive hit, got {other:?}"),
        }
    }

    #[test]
    fn entry_near_expiry_flags_prefetch() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0); // prefetch lead = 300/10 = 30s

        // 25s left (< 30s lead): still fresh, but flagged for a background refresh.
        match entry.lookup(t0 + Duration::from_secs(275), &CacheTuning::balanced()) {
            Some(CacheHit {
                response: CachedResponse::Positive(records),
                needs_refresh,
            }) => {
                assert_eq!(records[0].ttl, 25);
                assert!(needs_refresh, "near expiry should prefetch");
            }
            other => panic!("expected a fresh positive hit, got {other:?}"),
        }
    }

    #[test]
    fn expired_entry_is_served_stale_and_refreshed() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0);

        // 10s past expiry, inside the 60s serve-stale window.
        match entry.lookup(t0 + Duration::from_secs(310), &CacheTuning::balanced()) {
            Some(CacheHit {
                response: CachedResponse::Positive(records),
                needs_refresh,
            }) => {
                assert_eq!(records[0].ttl, STALE_TTL, "stale answer gets the short TTL");
                assert!(needs_refresh, "stale always triggers a refresh");
            }
            other => panic!("expected a stale positive hit, got {other:?}"),
        }
    }

    #[test]
    fn entry_past_the_stale_window_is_gone() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0);

        // 300s TTL + 60s balanced grace = 360s; one second past that is a miss.
        assert!(
            entry
                .lookup(t0 + Duration::from_secs(361), &CacheTuning::balanced())
                .is_none()
        );
    }

    #[test]
    fn privacy_never_serves_stale_or_prefetches() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0);
        let privacy = CacheTuning::privacy();

        // Near expiry: a fresh hit, but no background refresh under privacy.
        match entry.lookup(t0 + Duration::from_secs(299), &privacy) {
            Some(CacheHit { needs_refresh, .. }) => {
                assert!(!needs_refresh, "privacy never prefetches")
            }
            other => panic!("expected a fresh hit, got {other:?}"),
        }
        // One second past the TTL is already a miss — nothing is served stale.
        assert!(
            entry
                .lookup(t0 + Duration::from_secs(301), &privacy)
                .is_none(),
            "privacy never serves a stale answer"
        );
    }

    #[test]
    fn speed_serves_stale_far_past_the_balanced_window() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0);

        // 200s past expiry: a miss under balanced (60s), still stale-served under
        // speed (300s grace).
        assert!(
            entry
                .lookup(t0 + Duration::from_secs(500), &CacheTuning::balanced())
                .is_none()
        );
        match entry.lookup(t0 + Duration::from_secs(500), &CacheTuning::speed()) {
            Some(CacheHit { needs_refresh, .. }) => {
                assert!(needs_refresh, "a stale speed hit refreshes")
            }
            other => panic!("expected a stale hit under speed, got {other:?}"),
        }
    }

    #[test]
    fn negative_entry_is_returned_fresh() {
        let t0 = Instant::now();
        let mut entry = CachedEntry::negative(ResponseCode::NXDomain, Duration::from_secs(60));
        entry.created_at = t0;

        assert_eq!(
            entry.lookup(t0 + Duration::from_secs(10), &CacheTuning::balanced()),
            Some(CacheHit {
                response: CachedResponse::Negative(ResponseCode::NXDomain),
                needs_refresh: false,
            })
        );
    }

    #[test]
    fn prefetch_lead_is_clamped() {
        let balanced = CacheTuning::balanced();
        assert_eq!(
            balanced.prefetch_lead(Duration::from_secs(5)),
            Duration::from_secs(1)
        );
        assert_eq!(
            balanced.prefetch_lead(Duration::from_secs(120)),
            Duration::from_secs(12)
        );
        assert_eq!(
            balanced.prefetch_lead(Duration::from_secs(86400)),
            Duration::from_secs(30)
        );
    }

    #[tokio::test]
    async fn insert_then_get_hits() {
        let cache = DnsCache::new(100);
        cache
            .insert(
                key(),
                CachedEntry::positive(vec![a_record(300)], Duration::from_secs(300)),
            )
            .await;

        assert!(matches!(
            cache.get(&key()).await,
            Some(CacheHit {
                response: CachedResponse::Positive(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn missing_key_misses() {
        let cache = DnsCache::new(100);
        assert!(cache.get(&key()).await.is_none());
    }

    #[tokio::test]
    async fn clear_drops_every_entry() {
        let cache = DnsCache::new(100);
        cache
            .insert(
                key(),
                CachedEntry::positive(vec![a_record(300)], Duration::from_secs(300)),
            )
            .await;
        assert!(cache.get(&key()).await.is_some());

        cache.clear();
        assert!(cache.get(&key()).await.is_none(), "cleared");
    }

    #[tokio::test]
    async fn key_matching_ignores_case() {
        let cache = DnsCache::new(100);
        let upper = CacheKey::new(&Name::from_ascii("EXAMPLE.COM.").unwrap(), RecordType::A);
        cache
            .insert(
                upper,
                CachedEntry::positive(vec![a_record(300)], Duration::from_secs(300)),
            )
            .await;

        assert!(cache.get(&key()).await.is_some());
    }
}
