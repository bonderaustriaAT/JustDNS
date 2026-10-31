// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The durable query log, backed by DuckDB. Events are written in batches
//! through the appender and read back for history and to rehydrate the hot
//! in-memory view after a restart.
//!
//! Record and response codes are stored as plain `INTEGER`s: they are narrow
//! wire values, and keeping the schema to a handful of column types avoids
//! surprises when the appender matches Rust types to DuckDB's.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use duckdb::{Connection, params};

use crate::event::{QueryEvent, QuerySource};
use crate::recorder::{ActivityBucket, Stats};

/// One hour in milliseconds, used to bucket activity in SQL.
const HOUR_MS: i64 = 3_600_000;
/// Fallback client address for a log row with an unparseable address.
const UNKNOWN_CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS queries (
    at_ms         BIGINT  NOT NULL,
    client        VARCHAR NOT NULL,
    name          VARCHAR NOT NULL,
    record_type   INTEGER NOT NULL,
    response_code INTEGER NOT NULL,
    source        VARCHAR NOT NULL,
    duration_ms   BIGINT  NOT NULL,
    answers       INTEGER NOT NULL
);";

const SELECT_COLUMNS: &str =
    "at_ms, client, name, record_type, response_code, source, duration_ms, answers";

/// Errors from the DuckDB query log.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An error returned by DuckDB.
    #[error("duckdb: {0}")]
    Duck(#[from] duckdb::Error),
}

/// The persistent query log.
pub struct DuckStore {
    conn: Connection,
}

impl DuckStore {
    /// Open (creating if needed) the query log at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Open an in-memory query log, used for tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Append a batch of events to the log.
    pub fn insert_batch(&self, events: &[QueryEvent]) -> Result<(), StoreError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut appender = self.conn.appender("queries")?;
        for event in events {
            let client = event.client.to_string();
            appender.append_row(params![
                event.at_ms,
                client,
                event.name,
                i32::from(event.record_type),
                i32::from(event.response_code),
                event.source.label(),
                i64::from(event.duration_ms),
                i32::from(event.answers),
            ])?;
        }
        appender.flush()?;
        Ok(())
    }

    /// The most recent events, newest first, up to `limit`.
    pub fn recent(&self, limit: usize) -> Result<Vec<QueryEvent>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM queries ORDER BY at_ms DESC LIMIT ?"
        ))?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let events = stmt
            .query_map([limit], row_to_event)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }

    /// Running totals over the whole log.
    pub fn stats(&self) -> Result<Stats, StoreError> {
        let stats = self.conn.query_row(
            "SELECT
                COUNT(*),
                COUNT(*) FILTER (WHERE source = 'cache'),
                COUNT(*) FILTER (WHERE source = 'upstream'),
                COUNT(*) FILTER (WHERE source = 'local'),
                COUNT(*) FILTER (WHERE response_code = 3),
                COUNT(*) FILTER (WHERE response_code = 2)
             FROM queries",
            [],
            |row| {
                Ok(Stats {
                    total: count(row, 0)?,
                    cache_hits: count(row, 1)?,
                    upstream: count(row, 2)?,
                    local: count(row, 3)?,
                    nxdomain: count(row, 4)?,
                    servfail: count(row, 5)?,
                })
            },
        )?;
        Ok(stats)
    }

    /// Per-hour activity for the most recent 24 hours, oldest hour first.
    pub fn activity(&self) -> Result<Vec<ActivityBucket>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour, COUNT(*)
             FROM queries GROUP BY hour ORDER BY hour DESC LIMIT 24"
        ))?;
        let mut buckets = stmt
            .query_map([], |row| {
                Ok(ActivityBucket {
                    hour_ms: row.get(0)?,
                    count: count(row, 1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        buckets.reverse();
        Ok(buckets)
    }
}

/// Read a non-negative `COUNT(*)` column as a `u64`.
fn count(row: &duckdb::Row<'_>, index: usize) -> duckdb::Result<u64> {
    Ok(u64::try_from(row.get::<_, i64>(index)?).unwrap_or_default())
}

/// Map a log row back into a [`QueryEvent`], tolerating values that no longer
/// parse rather than failing the whole read.
fn row_to_event(row: &duckdb::Row<'_>) -> duckdb::Result<QueryEvent> {
    Ok(QueryEvent {
        at_ms: row.get(0)?,
        client: row.get::<_, String>(1)?.parse().unwrap_or(UNKNOWN_CLIENT),
        name: row.get(2)?,
        record_type: u16::try_from(row.get::<_, i32>(3)?).unwrap_or_default(),
        response_code: u16::try_from(row.get::<_, i32>(4)?).unwrap_or_default(),
        source: QuerySource::from_label(&row.get::<_, String>(5)?).unwrap_or(QuerySource::Upstream),
        duration_ms: u32::try_from(row.get::<_, i64>(6)?).unwrap_or_default(),
        answers: u16::try_from(row.get::<_, i32>(7)?).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn event(at_ms: i64, source: QuerySource, response_code: u16) -> QueryEvent {
        QueryEvent {
            at_ms,
            client: IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10)),
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code,
            source,
            duration_ms: 4,
            answers: 2,
        }
    }

    /// A temp DuckDB file that removes itself (and its WAL) when dropped.
    struct TempDb {
        path: std::path::PathBuf,
    }

    impl TempDb {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = format!(
                "justdns-store-{}-{}.duckdb",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            Self {
                path: std::env::temp_dir().join(unique),
            }
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(self.path.with_extension("duckdb.wal"));
        }
    }

    #[test]
    fn insert_batch_round_trips_newest_first() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                event(1, QuerySource::Upstream, 0),
                event(2, QuerySource::Cache, 0),
                event(3, QuerySource::Upstream, 3),
            ])
            .unwrap();

        let recent = store.recent(10).unwrap();
        let times: Vec<i64> = recent.iter().map(|event| event.at_ms).collect();
        assert_eq!(times, vec![3, 2, 1]);
        assert_eq!(recent[0].response_code, 3);
        assert_eq!(recent[1].source, QuerySource::Cache);
    }

    #[test]
    fn recent_respects_the_limit() {
        let store = DuckStore::open_in_memory().unwrap();
        let events: Vec<QueryEvent> = (1..=5)
            .map(|at| event(at, QuerySource::Upstream, 0))
            .collect();
        store.insert_batch(&events).unwrap();
        assert_eq!(store.recent(2).unwrap().len(), 2);
    }

    #[test]
    fn stats_aggregate_sources_and_codes() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                event(1, QuerySource::Cache, 0),
                event(2, QuerySource::Upstream, 3),
                event(3, QuerySource::Upstream, 2),
                event(4, QuerySource::Local, 0),
            ])
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.upstream, 2);
        assert_eq!(stats.local, 1);
        assert_eq!(stats.nxdomain, 1);
        assert_eq!(stats.servfail, 1);
    }

    #[test]
    fn activity_buckets_by_hour_oldest_first() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                event(5, QuerySource::Upstream, 0),
                event(HOUR_MS - 1, QuerySource::Upstream, 0),
                event(HOUR_MS + 10, QuerySource::Upstream, 0),
            ])
            .unwrap();

        let activity = store.activity().unwrap();
        assert_eq!(
            activity,
            vec![
                ActivityBucket {
                    hour_ms: 0,
                    count: 2,
                },
                ActivityBucket {
                    hour_ms: HOUR_MS,
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn data_survives_reopening_the_file() {
        let temp = TempDb::new();
        {
            let store = DuckStore::open(&temp.path).unwrap();
            store
                .insert_batch(&[event(1, QuerySource::Upstream, 0)])
                .unwrap();
        }
        let reopened = DuckStore::open(&temp.path).unwrap();
        assert_eq!(reopened.stats().unwrap().total, 1);
        assert_eq!(reopened.recent(10).unwrap().len(), 1);
    }
}
