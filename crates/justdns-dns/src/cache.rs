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

    fn remaining_ttl(&self, now: Instant) -> Option<Duration> {
        let elapsed = now.saturating_duration_since(self.created_at);
        if elapsed >= self.ttl {
            None
        } else {
            Some(self.ttl - elapsed)
        }
    }

    fn response(&self, now: Instant) -> Option<CachedResponse> {
        let remaining = self.remaining_ttl(now)?;
        match &self.kind {
            EntryKind::Positive(records) => {
                let ttl = remaining.as_secs().min(u64::from(u32::MAX)) as u32;
                let records = records
                    .iter()
                    .cloned()
                    .map(|mut record| {
                        record.set_ttl(ttl);
                        record
                    })
                    .collect();
                Some(CachedResponse::Positive(records))
            }
            EntryKind::Negative(code) => Some(CachedResponse::Negative(*code)),
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
        Some(value.ttl)
    }
}

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

    pub async fn get(&self, key: &CacheKey) -> Option<CachedResponse> {
        self.inner.get(key).await?.response(Instant::now())
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

    #[test]
    fn remaining_ttl_counts_down() {
        let t0 = Instant::now();
        let mut entry = CachedEntry::positive(vec![a_record(300)], Duration::from_secs(300));
        entry.created_at = t0;

        assert_eq!(
            entry.remaining_ttl(t0 + Duration::from_secs(100)),
            Some(Duration::from_secs(200))
        );
    }

    #[test]
    fn expired_entry_has_no_response() {
        let t0 = Instant::now();
        let mut entry = CachedEntry::positive(vec![a_record(300)], Duration::from_secs(300));
        entry.created_at = t0;

        assert_eq!(entry.remaining_ttl(t0 + Duration::from_secs(300)), None);
        assert!(entry.response(t0 + Duration::from_secs(300)).is_none());
    }

    #[test]
    fn response_counts_down_record_ttl() {
        let t0 = Instant::now();
        let mut entry = CachedEntry::positive(vec![a_record(300)], Duration::from_secs(300));
        entry.created_at = t0;

        match entry.response(t0 + Duration::from_secs(120)) {
            Some(CachedResponse::Positive(records)) => assert_eq!(records[0].ttl(), 180),
            other => panic!("expected positive response, got {other:?}"),
        }
    }

    #[test]
    fn negative_response_is_returned() {
        let t0 = Instant::now();
        let mut entry = CachedEntry::negative(ResponseCode::NXDomain, Duration::from_secs(60));
        entry.created_at = t0;

        assert_eq!(
            entry.response(t0 + Duration::from_secs(10)),
            Some(CachedResponse::Negative(ResponseCode::NXDomain))
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
            Some(CachedResponse::Positive(_))
        ));
    }

    #[tokio::test]
    async fn missing_key_misses() {
        let cache = DnsCache::new(100);
        assert!(cache.get(&key()).await.is_none());
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
