// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Response cache for the forwarding resolver.
//!
//! Each entry keeps the TTL the upstream reported. `moka` drops the entry once
//! that TTL elapses, and a read hands back the stored records with their TTL
//! counted down to the time remaining, as a resolver must. Negative answers
//! (NXDOMAIN / NODATA) are cached the same way.

use std::time::{Duration, Instant};

use hickory_proto::op::ResponseCode;
use hickory_proto::rr::{Name, Record, RecordType};
use moka::Expiry;
use moka::future::Cache;

/// How long past its DNS TTL an entry stays serveable while a background refresh
/// runs — the serve-stale window (RFC 8767). A just-expired popular name is
/// answered instantly from this stale copy instead of making the client wait.
const STALE_GRACE: Duration = Duration::from_secs(60);
/// The TTL stamped on a stale answer: short, so the client comes back soon and
/// picks up the refreshed record once it lands.
const STALE_TTL: u32 = 10;

/// How long before a fresh entry expires we start refreshing it in the
/// background, so a popular name is renewed before it ever goes stale. A tenth
/// of the record's own TTL, clamped so we neither thrash very short TTLs nor
/// refresh very long ones absurdly early.
fn prefetch_lead(ttl: Duration) -> Duration {
    (ttl / 10).clamp(Duration::from_secs(1), Duration::from_secs(30))
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
                        record.set_ttl(ttl);
                        record
                    })
                    .collect();
                CachedResponse::Positive(records)
            }
            EntryKind::Negative(code) => CachedResponse::Negative(*code),
        }
    }

    /// Classify the entry at `now`: a fresh hit (TTL counted down, with a
    /// prefetch flag once it nears expiry), a stale-but-serveable hit inside the
    /// serve-stale window (always flagged for refresh), or `None` once even that
    /// window has passed.
    fn lookup(&self, now: Instant) -> Option<CacheHit> {
        let elapsed = now.saturating_duration_since(self.created_at);
        if elapsed < self.ttl {
            let remaining = self.ttl - elapsed;
            let ttl = remaining.as_secs().min(u64::from(u32::MAX)) as u32;
            Some(CacheHit {
                response: self.render(ttl),
                needs_refresh: remaining <= prefetch_lead(self.ttl),
            })
        } else if elapsed < self.ttl + STALE_GRACE {
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
        // Hold the entry past its DNS TTL by the serve-stale window, so an
        // expired-but-recent answer is still there to serve while it refreshes.
        Some(value.ttl + STALE_GRACE)
    }
}

/// The response cache. Cloning shares the one underlying store (`moka` keeps it
/// behind an `Arc`), so the DNS pipeline and the web layer's "clear cache"
/// action can hold their own handles to the same cache.
#[derive(Clone)]
pub struct DnsCache {
    inner: Cache<CacheKey, CachedEntry>,
}

impl DnsCache {
    pub fn new(max_capacity: u64) -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(max_capacity)
                .expire_after(TtlExpiry)
                .build(),
        }
    }

    pub async fn insert(&self, key: CacheKey, entry: CachedEntry) {
        self.inner.insert(key, entry).await;
    }

    pub async fn get(&self, key: &CacheKey) -> Option<CacheHit> {
        self.inner.get(key).await?.lookup(Instant::now())
    }

    /// Drop every cached entry. The next query for each name goes upstream (or to
    /// a local rewrite) again. Used by the dashboard's "clear DNS cache" action.
    pub fn clear(&self) {
        self.inner.invalidate_all();
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

        match entry.lookup(t0 + Duration::from_secs(100)) {
            Some(CacheHit {
                response: CachedResponse::Positive(records),
                needs_refresh,
            }) => {
                assert_eq!(records[0].ttl(), 200, "TTL counted down");
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
        match entry.lookup(t0 + Duration::from_secs(275)) {
            Some(CacheHit {
                response: CachedResponse::Positive(records),
                needs_refresh,
            }) => {
                assert_eq!(records[0].ttl(), 25);
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
        match entry.lookup(t0 + Duration::from_secs(310)) {
            Some(CacheHit {
                response: CachedResponse::Positive(records),
                needs_refresh,
            }) => {
                assert_eq!(
                    records[0].ttl(),
                    STALE_TTL,
                    "stale answer gets the short TTL"
                );
                assert!(needs_refresh, "stale always triggers a refresh");
            }
            other => panic!("expected a stale positive hit, got {other:?}"),
        }
    }

    #[test]
    fn entry_past_the_stale_window_is_gone() {
        let t0 = Instant::now();
        let entry = entry_at(300, t0);

        // 300s TTL + 60s grace = 360s; one second past that is a miss.
        assert!(entry.lookup(t0 + Duration::from_secs(361)).is_none());
    }

    #[test]
    fn negative_entry_is_returned_fresh() {
        let t0 = Instant::now();
        let mut entry = CachedEntry::negative(ResponseCode::NXDomain, Duration::from_secs(60));
        entry.created_at = t0;

        assert_eq!(
            entry.lookup(t0 + Duration::from_secs(10)),
            Some(CacheHit {
                response: CachedResponse::Negative(ResponseCode::NXDomain),
                needs_refresh: false,
            })
        );
    }

    #[test]
    fn prefetch_lead_is_clamped() {
        assert_eq!(
            prefetch_lead(Duration::from_secs(5)),
            Duration::from_secs(1)
        );
        assert_eq!(
            prefetch_lead(Duration::from_secs(120)),
            Duration::from_secs(12)
        );
        assert_eq!(
            prefetch_lead(Duration::from_secs(86400)),
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
