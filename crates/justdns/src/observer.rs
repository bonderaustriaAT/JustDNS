// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bridge the resolver's query observer to the persistent query store: turn
//! each [`QueryRecord`] the pipeline reports into a stored [`QueryEvent`].

use std::time::{SystemTime, UNIX_EPOCH};

use justdns_dns::observer::{AnswerSource, QueryObserver, QueryRecord};
use justdns_store::{QueryEvent, QuerySource, Store};

pub struct StoreObserver {
    store: Store,
}

impl StoreObserver {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

impl QueryObserver for StoreObserver {
    fn observe(&self, record: QueryRecord) {
        self.store.record(QueryEvent {
            at_ms: now_ms(),
            client: record.client,
            name: record.name,
            record_type: record.record_type,
            response_code: record.response_code,
            source: map_source(record.source),
            duration_ms: u32::try_from(record.duration.as_millis()).unwrap_or(u32::MAX),
            answers: record.answers,
        });
    }
}

fn map_source(source: AnswerSource) -> QuerySource {
    match source {
        AnswerSource::Cache => QuerySource::Cache,
        AnswerSource::Upstream => QuerySource::Upstream,
        AnswerSource::Local => QuerySource::Local,
        AnswerSource::Blocked => QuerySource::Blocked,
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
