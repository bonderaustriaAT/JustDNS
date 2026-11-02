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

use std::collections::HashMap;

use crate::event::{QueryEvent, QuerySource};
use crate::recorder::{HourBucket, Stats};

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
    answers       INTEGER NOT NULL,
    addresses     VARCHAR DEFAULT ''
);";

/// Schema upgrades for a log written by an older build. `addresses` (the IPs a
/// query resolved to) was added after the first release, so an existing file
/// gains the column with an empty default; its old rows then read back as having
/// no recorded addresses. The column is nullable because DuckDB will not add a
/// NOT NULL column with `IF NOT EXISTS`; every write still sets it.
const MIGRATIONS: &str =
    "ALTER TABLE queries ADD COLUMN IF NOT EXISTS addresses VARCHAR DEFAULT '';";

const SELECT_COLUMNS: &str =
    "at_ms, client, name, record_type, response_code, source, duration_ms, answers, addresses";

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
        conn.execute_batch(MIGRATIONS)?;
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
            let addresses = join_addresses(&event.addresses);
            appender.append_row(params![
                event.at_ms,
                client,
                event.name,
                i32::from(event.record_type),
                i32::from(event.response_code),
                event.source.label(),
                i64::from(event.duration_ms),
                i32::from(event.answers),
                addresses,
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
                COUNT(*) FILTER (WHERE source IN ('local', 'rewrite')),
                COUNT(*) FILTER (WHERE source = 'blocked'),
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
                    blocked: count(row, 4)?,
                    nxdomain: count(row, 5)?,
                    servfail: count(row, 6)?,
                })
            },
        )?;
        Ok(stats)
    }

    /// The most recent 24 hours of activity, oldest hour first, with the extra
    /// per-hour tallies the statistics view needs: blocked count, summed handling
    /// time and per-client counts. Used to rehydrate the hot view on startup so a
    /// restart continues the day's figures instead of counting from zero.
    pub(crate) fn day_buckets(&self) -> Result<Vec<HourBucket>, StoreError> {
        // Per-hour totals for the most recent 24 hours that saw traffic. SUM is
        // cast back to BIGINT so it reads as an i64 (DuckDB widens a BIGINT sum
        // to HUGEINT otherwise); per-hour handling time never approaches i64.
        let mut stmt = self.conn.prepare(&format!(
            "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour,
                    COUNT(*),
                    COUNT(*) FILTER (WHERE source = 'blocked'),
                    CAST(COALESCE(SUM(duration_ms), 0) AS BIGINT)
             FROM queries GROUP BY hour ORDER BY hour DESC LIMIT 24"
        ))?;
        let mut buckets = stmt
            .query_map([], |row| {
                let mut bucket = HourBucket::new(row.get::<_, i64>(0)?);
                bucket.count = count(row, 1)?;
                bucket.blocked = count(row, 2)?;
                bucket.duration_sum_ms = count(row, 3)?;
                Ok(bucket)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        buckets.reverse(); // oldest hour first, matching the hot view

        // Seed per-client counts for exactly those hours. The oldest kept hour is
        // the cutoff; ordering by count puts the busiest clients first so they win
        // the per-hour cap when an hour has more distinct clients than it keeps.
        if let Some(cutoff) = buckets.first().map(|bucket| bucket.hour_ms) {
            let index: HashMap<i64, usize> = buckets
                .iter()
                .enumerate()
                .map(|(i, bucket)| (bucket.hour_ms, i))
                .collect();
            let mut stmt = self.conn.prepare(&format!(
                "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour, client, COUNT(*)
                 FROM queries WHERE at_ms >= ?
                 GROUP BY hour, client ORDER BY hour, COUNT(*) DESC"
            ))?;
            let rows = stmt.query_map([cutoff], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    count(row, 2)?,
                ))
            })?;
            for row in rows {
                let (hour, client, count) = row?;
                if let Some(&i) = index.get(&hour) {
                    let client = client.parse().unwrap_or(UNKNOWN_CLIENT);
                    buckets[i].add_client(client, count);
                }
            }
        }

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
        addresses: parse_addresses(&row.get::<_, Option<String>>(8)?.unwrap_or_default()),
    })
}

/// Join a query's resolved addresses into the comma-separated form stored in the
/// log. No addresses becomes the empty string.
fn join_addresses(addresses: &[IpAddr]) -> String {
    addresses
        .iter()
        .map(IpAddr::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse the stored comma-separated addresses back into a list, dropping any
/// fragment that no longer parses rather than failing the whole read.
fn parse_addresses(stored: &str) -> Vec<IpAddr> {
    stored
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect()
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
            addresses: vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
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
        assert_eq!(
            recent[0].addresses,
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
            "resolved addresses round-trip through the log"
        );
    }

    #[test]
    fn rewrite_source_round_trips_and_counts_as_local() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                event(1, QuerySource::Local, 0),
                event(2, QuerySource::Rewrite, 0),
            ])
            .unwrap();

        // Newest first: the rewrite (at_ms 2) comes back with its own source.
        assert_eq!(store.recent(10).unwrap()[0].source, QuerySource::Rewrite);
        // Both a plain local answer and a rewrite count toward the "local" total.
        assert_eq!(store.stats().unwrap().local, 2);
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
    fn day_buckets_by_hour_oldest_first() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                event(5, QuerySource::Upstream, 0),
                event(HOUR_MS - 1, QuerySource::Upstream, 0),
                event(HOUR_MS + 10, QuerySource::Upstream, 0),
            ])
            .unwrap();

        let buckets = store.day_buckets().unwrap();
        let shape: Vec<(i64, u64)> = buckets.iter().map(|b| (b.hour_ms, b.count)).collect();
        assert_eq!(shape, vec![(0, 2), (HOUR_MS, 1)]);
    }

    #[test]
    fn day_buckets_carry_blocked_duration_and_clients() {
        use std::net::{IpAddr, Ipv4Addr};

        let client_a = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let client_b = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 20));
        let mk = |at_ms, source, client, duration_ms| QueryEvent {
            at_ms,
            client,
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code: 0,
            source,
            duration_ms,
            answers: 1,
            addresses: Vec::new(),
        };
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(5, QuerySource::Upstream, client_a, 10),
                mk(6, QuerySource::Blocked, client_a, 2),
                mk(7, QuerySource::Cache, client_b, 0),
            ])
            .unwrap();

        let buckets = store.day_buckets().unwrap();
        assert_eq!(buckets.len(), 1, "all three fall in hour 0");
        let hour = &buckets[0];
        assert_eq!(hour.count, 3);
        assert_eq!(hour.blocked, 1, "one blocked query");
        assert_eq!(hour.duration_sum_ms, 12, "10 + 2 + 0");
        assert_eq!(hour.clients.get(&client_a), Some(&2));
        assert_eq!(hour.clients.get(&client_b), Some(&1));
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
