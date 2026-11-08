// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bridge the resolver's query observer to the persistent query store: turn
//! each [`QueryRecord`] the pipeline reports into a stored [`QueryEvent`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use justdns_dns::observer::{
    AnswerSource, DnssecStatus as DnsDnssecStatus, QueryObserver, QueryRecord,
};
use justdns_store::{DnssecStatus, QueryEvent, QuerySource, Store};

pub struct StoreObserver {
    store: Store,
    /// Live "record queries" switch from the config store. When off, an answered
    /// query is dropped instead of logged, so the query log and its statistics
    /// stop gaining new rows without a restart.
    query_log: Arc<AtomicBool>,
}

impl StoreObserver {
    pub fn new(store: Store, query_log: Arc<AtomicBool>) -> Self {
        Self { store, query_log }
    }
}

impl QueryObserver for StoreObserver {
    fn observe(&self, record: QueryRecord) {
        if !self.query_log.load(Ordering::Relaxed) {
            return;
        }
        self.store.record(QueryEvent {
            at_ms: now_ms(),
            client: record.client,
            name: record.name,
            record_type: record.record_type,
            response_code: record.response_code,
            source: map_source(record.source),
            duration_ms: u32::try_from(record.duration.as_millis()).unwrap_or(u32::MAX),
            answers: record.answers,
            addresses: record.addresses,
            resolver: record.resolver,
            dnssec: record.dnssec.map(map_dnssec),
        });
    }
}

fn map_source(source: AnswerSource) -> QuerySource {
    match source {
        AnswerSource::Cache => QuerySource::Cache,
        AnswerSource::Upstream => QuerySource::Upstream,
        AnswerSource::Local => QuerySource::Local,
        AnswerSource::Rewrite => QuerySource::Rewrite,
        AnswerSource::Blocked => QuerySource::Blocked,
    }
}

fn map_dnssec(status: DnsDnssecStatus) -> DnssecStatus {
    match status {
        DnsDnssecStatus::Secure => DnssecStatus::Secure,
        DnsDnssecStatus::Insecure => DnssecStatus::Insecure,
        DnsDnssecStatus::Bogus => DnssecStatus::Bogus,
        DnsDnssecStatus::Indeterminate => DnssecStatus::Indeterminate,
    }
}

/// Milliseconds since the Unix epoch, clamped into range on the impossible
/// chance the clock is absurd.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    fn sample() -> QueryRecord {
        QueryRecord {
            client: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            name: "kek.at.".to_owned(),
            record_type: 1,
            response_code: 0,
            source: AnswerSource::Blocked,
            duration: Duration::from_millis(1),
            answers: 0,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        }
    }

    #[tokio::test]
    async fn records_a_query_when_logging_is_on() {
        let (store, _writer) = Store::in_memory(16).unwrap();
        let flag = Arc::new(AtomicBool::new(true));
        let observer = StoreObserver::new(store.clone(), flag);
        observer.observe(sample());
        assert_eq!(store.recent(16).len(), 1, "a logged query is recorded");
    }

    #[tokio::test]
    async fn drops_a_query_when_logging_is_off() {
        let (store, _writer) = Store::in_memory(16).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let observer = StoreObserver::new(store.clone(), flag.clone());
        observer.observe(sample());
        assert_eq!(store.recent(16).len(), 0, "logging off keeps no record");

        // Flipping it back on takes effect on the next query, no restart.
        flag.store(true, Ordering::Relaxed);
        observer.observe(sample());
        assert_eq!(store.recent(16).len(), 1, "the switch is live");
    }
}
