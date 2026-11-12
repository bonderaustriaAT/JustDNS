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

use duckdb::{Connection, ToSql, params, params_from_iter};

use std::collections::HashMap;

use crate::event::{DnssecStatus, QueryEvent, QuerySource};
use crate::recorder::{HourBucket, LATENCY_BOUNDS_MS, Stats};

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
    addresses     VARCHAR DEFAULT '',
    resolver      VARCHAR DEFAULT '',
    dnssec        VARCHAR DEFAULT '',
    origin        VARCHAR DEFAULT ''
);";

/// Schema upgrades for a log written by an older build, applied in order. Each
/// column added after the first release gains an empty default on an existing
/// file, so its old rows read back as having no value. `addresses` holds the IPs
/// a query resolved to; `resolver` holds the upstream that answered (empty for a
/// cache, block or local answer). Both are nullable because DuckDB will not add
/// a NOT NULL column with `IF NOT EXISTS`; every write still sets them.
/// `origin` holds the node a row came from: empty for a locally answered query
/// (the only kind before high-availability query-log sync), or a peer's node id
/// for a row pulled from that peer over the mesh. It lets the history show each
/// row's node and keeps the node-local views (rehydrated from `origin = ''`)
/// apart from the unioned explorer.
const MIGRATIONS: &str = "\
    ALTER TABLE queries ADD COLUMN IF NOT EXISTS addresses VARCHAR DEFAULT '';
    ALTER TABLE queries ADD COLUMN IF NOT EXISTS resolver VARCHAR DEFAULT '';
    ALTER TABLE queries ADD COLUMN IF NOT EXISTS dnssec VARCHAR DEFAULT '';
    ALTER TABLE queries ADD COLUMN IF NOT EXISTS origin VARCHAR DEFAULT '';";

const SELECT_COLUMNS: &str = "at_ms, client, name, record_type, response_code, source, \
    duration_ms, answers, addresses, resolver, dnssec";

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

    /// Append a batch of locally answered events to the log (empty `origin`).
    pub fn insert_batch(&self, events: &[QueryEvent]) -> Result<(), StoreError> {
        self.insert_rows(events.iter().map(|event| (event, "")))
    }

    /// Append a batch of events pulled from a peer, all tagged with that peer's
    /// `origin` node id so they read back as that node's rows in the explorer and
    /// stay out of the node-local, `origin = ''` views.
    pub fn insert_remote(&self, origin: &str, events: &[QueryEvent]) -> Result<(), StoreError> {
        self.insert_rows(events.iter().map(|event| (event, origin)))
    }

    /// Append a batch of rows that may mix origins (the writer interleaves locally
    /// answered queries and peer-ingested rows on one queue).
    pub fn insert_mixed(&self, rows: &[(QueryEvent, String)]) -> Result<(), StoreError> {
        self.insert_rows(rows.iter().map(|(event, origin)| (event, origin.as_str())))
    }

    /// Append rows, each with its own `origin`. The appender needs a value for
    /// every column in order, so `origin` is written explicitly (DuckDB does not
    /// apply the column default through the appender).
    fn insert_rows<'a, I>(&self, rows: I) -> Result<(), StoreError>
    where
        I: IntoIterator<Item = (&'a QueryEvent, &'a str)>,
    {
        let mut appender = self.conn.appender("queries")?;
        let mut any = false;
        for (event, origin) in rows {
            any = true;
            let client = event.client.to_string();
            let addresses = join_addresses(&event.addresses);
            let resolver = event.resolver.as_deref().unwrap_or("");
            let dnssec = event.dnssec.map(DnssecStatus::label).unwrap_or("");
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
                resolver,
                dnssec,
                origin,
            ])?;
        }
        if any {
            appender.flush()?;
        }
        Ok(())
    }

    /// Delete every row older than `cutoff_ms` (all origins), enforcing the
    /// configured query-log retention window. Returns how many rows were removed.
    pub fn prune_before(&self, cutoff_ms: i64) -> Result<usize, StoreError> {
        let removed = self
            .conn
            .execute("DELETE FROM queries WHERE at_ms < ?", params![cutoff_ms])?;
        Ok(removed)
    }

    /// Clone a read-only handle onto the same in-process database. The clone
    /// shares the live DuckDB instance the writer owns, so it sees every row the
    /// writer has already flushed (a new row shows up within one flush interval).
    /// Opening a second [`Connection`] on the file path instead would deadlock on
    /// the file lock, so the clone is the only way to read while the writer runs.
    pub fn reader(&self) -> Result<DuckReader, StoreError> {
        Ok(DuckReader {
            conn: self.conn.try_clone()?,
        })
    }

    /// The most recent events, newest first, up to `limit`.
    pub fn recent(&self, limit: usize) -> Result<Vec<QueryEvent>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM queries WHERE origin = '' \
             ORDER BY at_ms DESC LIMIT ?"
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
                COUNT(*) FILTER (WHERE response_code = 2),
                COUNT(*) FILTER (WHERE dnssec = 'secure'),
                COUNT(*) FILTER (WHERE dnssec = 'insecure'),
                COUNT(*) FILTER (WHERE dnssec = 'bogus'),
                COUNT(*) FILTER (WHERE dnssec = 'indeterminate')
             FROM queries WHERE origin = ''",
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
                    dnssec_secure: count(row, 7)?,
                    dnssec_insecure: count(row, 8)?,
                    dnssec_bogus: count(row, 9)?,
                    dnssec_indeterminate: count(row, 10)?,
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
        // Per-hour totals for the most recent 24 hours that saw traffic. The
        // handling-time sum covers every query, cache hits included, matching the
        // hot view. SUM is cast back to BIGINT so it reads as an i64 (DuckDB
        // widens a BIGINT sum to HUGEINT otherwise); per-hour handling time never
        // approaches i64.
        let mut stmt = self.conn.prepare(&format!(
            "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour,
                    COUNT(*),
                    COUNT(*) FILTER (WHERE source = 'blocked'),
                    CAST(COALESCE(SUM(duration_ms), 0) AS BIGINT)
             FROM queries WHERE origin = '' GROUP BY hour ORDER BY hour DESC LIMIT 24"
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
                 FROM queries WHERE origin = '' AND at_ms >= ?
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

            // Seed per-client, per-domain counts for the per-client top-domains
            // view. Ordering by count puts the busiest domains first so they win
            // the per-client cap when a client visited more distinct names in an
            // hour than are kept.
            let mut stmt = self.conn.prepare(&format!(
                "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour, client, name, COUNT(*)
                 FROM queries WHERE origin = '' AND at_ms >= ?
                 GROUP BY hour, client, name ORDER BY hour, client, COUNT(*) DESC"
            ))?;
            let rows = stmt.query_map([cutoff], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    count(row, 3)?,
                ))
            })?;
            for row in rows {
                let (hour, client, name, count) = row?;
                if let Some(&i) = index.get(&hour) {
                    let client = client.parse().unwrap_or(UNKNOWN_CLIENT);
                    buckets[i].add_domain(client, &name, count);
                }
            }

            // Seed the per-hour latency histogram from every query, binned
            // exactly as the hot view bins a live answer (see `latency_bin`), so
            // the day's median and p95 survive a restart.
            let bin_case = latency_bin_case("duration_ms");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour, {bin_case} AS bin, COUNT(*)
                 FROM queries WHERE origin = '' AND at_ms >= ?
                 GROUP BY hour, bin"
            ))?;
            let rows = stmt.query_map([cutoff], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, count(row, 2)?))
            })?;
            for row in rows {
                let (hour, bin, count) = row?;
                if let (Some(&i), Ok(bin)) = (index.get(&hour), usize::try_from(bin)) {
                    if let Some(slot) = buckets[i].latency_hist.get_mut(bin) {
                        *slot += count;
                    }
                }
            }

            // Seed per-resolver answer counts and summed handling time for the
            // per-resolver response-time view. Only upstream answers carry a
            // resolver, so rows without one are skipped; ordering by count puts
            // the busiest resolvers first so they win the per-hour cap.
            let mut stmt = self.conn.prepare(&format!(
                "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour, resolver, COUNT(*),
                        CAST(COALESCE(SUM(duration_ms), 0) AS BIGINT)
                 FROM queries WHERE origin = '' AND at_ms >= ? AND resolver <> ''
                 GROUP BY hour, resolver ORDER BY hour, COUNT(*) DESC"
            ))?;
            let rows = stmt.query_map([cutoff], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    count(row, 2)?,
                    count(row, 3)?,
                ))
            })?;
            for row in rows {
                let (hour, resolver, count, duration_sum) = row?;
                if let Some(&i) = index.get(&hour) {
                    buckets[i].seed_resolver(&resolver, count, duration_sum);
                }
            }

            // Seed each resolver's latency histogram, binned exactly as the hot
            // view bins a live answer, so the per-resolver median and p95 survive
            // a restart. Reuses the same `bin_case` as the overall histogram.
            let mut stmt = self.conn.prepare(&format!(
                "SELECT at_ms - (at_ms % {HOUR_MS}) AS hour, resolver, {bin_case} AS bin, COUNT(*)
                 FROM queries WHERE origin = '' AND at_ms >= ? AND resolver <> ''
                 GROUP BY hour, resolver, bin"
            ))?;
            let rows = stmt.query_map([cutoff], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    count(row, 3)?,
                ))
            })?;
            for row in rows {
                let (hour, resolver, bin, count) = row?;
                if let (Some(&i), Ok(bin)) = (index.get(&hour), usize::try_from(bin)) {
                    buckets[i].seed_resolver_bin(&resolver, bin, count);
                }
            }
        }

        Ok(buckets)
    }
}

/// A keyset-pagination cursor into the history: the `(at_ms, rowid)` of the
/// last row of the page just read. The next page asks for rows strictly older
/// than it, which is stable even when many rows share an `at_ms`.
pub type Cursor = (i64, i64);

/// A server-side filter over the durable query log. Every field is optional; a
/// default filter (with a `limit`) matches the whole log, newest first.
#[derive(Debug, Default, Clone)]
pub struct QueryFilter {
    /// Keyset cursor: return only rows strictly older than this `(at_ms, rowid)`.
    pub before: Option<Cursor>,
    /// Lower bound on `at_ms`, inclusive.
    pub start_ms: Option<i64>,
    /// Upper bound on `at_ms`, inclusive.
    pub end_ms: Option<i64>,
    /// Match any of these sources (OR); empty matches any source.
    pub sources: Vec<QuerySource>,
    /// Match this record type, by its numeric wire value.
    pub record_type: Option<u16>,
    /// Match this response code, by its numeric wire value.
    pub response_code: Option<u16>,
    /// Match this resolver label exactly.
    pub resolver: Option<String>,
    /// Match this origin exactly: `Some("")` is this node's own rows, `Some(id)` a
    /// specific peer's, `None` every node. Lets the explorer filter by node.
    pub origin: Option<String>,
    /// Restrict to these client addresses (the log's `client` text). `None` means
    /// no restriction; `Some(list)` matches `client IN (list)`, and an **empty**
    /// list matches nothing — the explorer uses it when a device- or group-filter
    /// resolves to no observed client, so the result is honestly empty rather than
    /// unfiltered. The web layer fills this from the device/group catalogue.
    pub clients: Option<Vec<String>>,
    /// Match rows whose name or client address contains this, case-insensitively.
    pub needle: Option<String>,
    /// Maximum rows to return.
    pub limit: usize,
}

/// A read-only handle onto the query log, holding its own DuckDB connection
/// cloned from the writer's (see [`DuckStore::reader`]). Reads here serve the
/// history view without touching the writer's connection.
pub struct DuckReader {
    conn: Connection,
}

impl DuckReader {
    /// Run a filtered, keyset-paginated history query, newest first. Returns at
    /// most `filter.limit` rows, each paired with its `rowid` so the caller can
    /// form the next page's [`Cursor`]. WHERE clauses are appended only for the
    /// filters that are set, so an otherwise-empty filter reads like `recent`.
    pub fn query(&self, filter: &QueryFilter) -> Result<Vec<HistoryRow>, StoreError> {
        let mut wheres: Vec<String> = Vec::new();
        let mut binds: Vec<Box<dyn ToSql>> = Vec::new();

        if let Some((at_ms, rowid)) = filter.before {
            // Expanded tuple comparison `(at_ms, rowid) < (?, ?)`, written out so
            // it does not lean on row-value comparison support.
            wheres.push("(at_ms < ? OR (at_ms = ? AND rowid < ?))".to_owned());
            binds.push(Box::new(at_ms));
            binds.push(Box::new(at_ms));
            binds.push(Box::new(rowid));
        }
        if let Some(start) = filter.start_ms {
            wheres.push("at_ms >= ?".to_owned());
            binds.push(Box::new(start));
        }
        if let Some(end) = filter.end_ms {
            wheres.push("at_ms <= ?".to_owned());
            binds.push(Box::new(end));
        }
        if !filter.sources.is_empty() {
            let placeholders = vec!["?"; filter.sources.len()].join(", ");
            wheres.push(format!("source IN ({placeholders})"));
            for source in &filter.sources {
                binds.push(Box::new(source.label().to_owned()));
            }
        }
        if let Some(record_type) = filter.record_type {
            wheres.push("record_type = ?".to_owned());
            binds.push(Box::new(i32::from(record_type)));
        }
        if let Some(response_code) = filter.response_code {
            wheres.push("response_code = ?".to_owned());
            binds.push(Box::new(i32::from(response_code)));
        }
        if let Some(resolver) = &filter.resolver {
            wheres.push("resolver = ?".to_owned());
            binds.push(Box::new(resolver.clone()));
        }
        if let Some(origin) = &filter.origin {
            wheres.push("origin = ?".to_owned());
            binds.push(Box::new(origin.clone()));
        }
        if let Some(clients) = &filter.clients {
            if clients.is_empty() {
                // A device/group filter that matched no observed client: return
                // nothing rather than silently dropping the constraint.
                wheres.push("1 = 0".to_owned());
            } else {
                let placeholders = vec!["?"; clients.len()].join(", ");
                wheres.push(format!("client IN ({placeholders})"));
                for client in clients {
                    binds.push(Box::new(client.clone()));
                }
            }
        }
        if let Some(needle) = &filter.needle {
            // Substring match on name or client, case-insensitive. The needle is
            // escaped so a literal `%`/`_`/`\` in it is not read as a wildcard.
            wheres.push("(name ILIKE ? ESCAPE '\\' OR client ILIKE ? ESCAPE '\\')".to_owned());
            let pattern = format!("%{}%", escape_like(needle));
            binds.push(Box::new(pattern.clone()));
            binds.push(Box::new(pattern));
        }

        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", wheres.join(" AND "))
        };
        let limit = i64::try_from(filter.limit).unwrap_or(i64::MAX);
        binds.push(Box::new(limit));

        let sql = format!(
            "SELECT {SELECT_COLUMNS}, origin, rowid FROM queries{where_sql} \
             ORDER BY at_ms DESC, rowid DESC LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds.iter()), |row| {
                let event = row_to_event(row)?;
                let origin: String = row.get::<_, Option<String>>(11)?.unwrap_or_default();
                let rowid: i64 = row.get(12)?;
                Ok(HistoryRow {
                    event,
                    origin,
                    rowid,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// This node's own (`origin = ''`) events with `at_ms >= since_ms`, **oldest
    /// first**, up to `limit`. This is what a node serves to a peer over the mesh:
    /// only its own rows, so a four-node full mesh never re-forwards another node's
    /// events and cannot loop. Oldest-first lets the puller advance its watermark.
    pub fn since_local(&self, since_ms: i64, limit: usize) -> Result<Vec<QueryEvent>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM queries \
             WHERE origin = '' AND at_ms >= ? ORDER BY at_ms ASC, rowid ASC LIMIT ?"
        ))?;
        let events = stmt
            .query_map(params![since_ms, limit], row_to_event)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }

    /// Every distinct client address in the log (the raw `client` text), up to
    /// `limit`. The explorer uses this to turn a device- or group-filter — which
    /// may name a CIDR range — into the concrete set of observed addresses to match
    /// in SQL, so the filter respects the same most-specific-device resolution the
    /// log's name column uses.
    pub fn distinct_clients(&self, limit: usize) -> Result<Vec<String>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT client FROM queries LIMIT ?")?;
        let clients = stmt
            .query_map(params![limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(clients)
    }

    /// The natural keys already present for `origin` at `at_ms >= since_ms`, so an
    /// ingest can skip rows it already holds (the puller re-pulls from its watermark
    /// inclusively, so the boundary millisecond can repeat). The key matches
    /// [`natural_key`] on a [`QueryEvent`].
    pub fn existing_keys(
        &self,
        origin: &str,
        since_ms: i64,
    ) -> Result<std::collections::HashSet<String>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT at_ms, client, name, record_type, response_code, source, \
             duration_ms, answers FROM queries WHERE origin = ? AND at_ms >= ?",
        )?;
        let keys = stmt
            .query_map(params![origin, since_ms], |row| {
                Ok(format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}",
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, i32>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i32>(7)?,
                ))
            })?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        Ok(keys)
    }

    /// Aggregate the log over the half-open window `[start_ms, end_ms)` for a
    /// scheduled report: totals, the source breakdown, DNSSEC verdicts, summed
    /// handling time, and the top `top_n` queried domains, blocked domains and
    /// clients. Counted over **every origin** (the whole cluster, matching the
    /// explorer's unioned view), not just this node's own rows. The top-client
    /// entries carry the raw client address; the web layer resolves device names.
    pub fn period_report(
        &self,
        start_ms: i64,
        end_ms: i64,
        top_n: usize,
    ) -> Result<PeriodReport, StoreError> {
        let mut report = self.conn.query_row(
            "SELECT
                COUNT(*),
                COUNT(*) FILTER (WHERE source = 'cache'),
                COUNT(*) FILTER (WHERE source = 'upstream'),
                COUNT(*) FILTER (WHERE source = 'local'),
                COUNT(*) FILTER (WHERE source = 'rewrite'),
                COUNT(*) FILTER (WHERE source = 'blocked'),
                CAST(COALESCE(SUM(duration_ms), 0) AS BIGINT),
                COUNT(*) FILTER (WHERE dnssec = 'secure'),
                COUNT(*) FILTER (WHERE dnssec = 'insecure'),
                COUNT(*) FILTER (WHERE dnssec = 'bogus'),
                COUNT(*) FILTER (WHERE dnssec = 'indeterminate')
             FROM queries WHERE at_ms >= ? AND at_ms < ?",
            params![start_ms, end_ms],
            |row| {
                Ok(PeriodReport {
                    total: count(row, 0)?,
                    cache: count(row, 1)?,
                    upstream: count(row, 2)?,
                    local: count(row, 3)?,
                    rewrite: count(row, 4)?,
                    blocked: count(row, 5)?,
                    duration_sum_ms: count(row, 6)?,
                    dnssec_secure: count(row, 7)?,
                    dnssec_insecure: count(row, 8)?,
                    dnssec_bogus: count(row, 9)?,
                    dnssec_indeterminate: count(row, 10)?,
                    top_domains: Vec::new(),
                    top_blocked: Vec::new(),
                    top_clients: Vec::new(),
                })
            },
        )?;

        let top_n = i64::try_from(top_n).unwrap_or(i64::MAX);
        report.top_domains = self.top_in_window(
            "SELECT name, COUNT(*) c FROM queries \
             WHERE at_ms >= ? AND at_ms < ? GROUP BY name ORDER BY c DESC, name LIMIT ?",
            start_ms,
            end_ms,
            top_n,
        )?;
        report.top_blocked = self.top_in_window(
            "SELECT name, COUNT(*) c FROM queries \
             WHERE at_ms >= ? AND at_ms < ? AND source = 'blocked' \
             GROUP BY name ORDER BY c DESC, name LIMIT ?",
            start_ms,
            end_ms,
            top_n,
        )?;
        report.top_clients = self.top_in_window(
            "SELECT client, COUNT(*) c FROM queries \
             WHERE at_ms >= ? AND at_ms < ? GROUP BY client ORDER BY c DESC, client LIMIT ?",
            start_ms,
            end_ms,
            top_n,
        )?;
        Ok(report)
    }

    /// How many **blocked** lookups each client made over `[start_ms, end_ms)`,
    /// busiest first, limited to `limit` clients. Node-local only (`origin = ''`),
    /// so it reflects what this node itself answered — the spike alert watches a
    /// node's own clients, not peers'. The client is the raw address; the web
    /// layer resolves device names.
    pub fn blocked_counts_by_client(
        &self,
        start_ms: i64,
        end_ms: i64,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.top_in_window(
            "SELECT client, COUNT(*) c FROM queries \
             WHERE at_ms >= ? AND at_ms < ? AND source = 'blocked' AND origin = '' \
             GROUP BY client ORDER BY c DESC, client LIMIT ?",
            start_ms,
            end_ms,
            limit,
        )
    }

    /// A per-client, per-name count over `[start_ms, end_ms)`, split by whether
    /// the lookup was blocked, busiest first and capped at `limit` rows. Node-local
    /// only (`origin = ''`). This is the raw material for the privacy dashboard:
    /// the web layer maps each name to the company behind it and sums per device.
    /// The cap keeps the scan bounded; because it is ordered by count, the
    /// high-frequency tracker names (the ones that matter) are kept.
    pub fn tracker_scan(
        &self,
        start_ms: i64,
        end_ms: i64,
        limit: usize,
    ) -> Result<Vec<TrackerScanRow>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT client, name, source = 'blocked' AS blocked, COUNT(*) c \
             FROM queries \
             WHERE at_ms >= ? AND at_ms < ? AND origin = '' \
             GROUP BY client, name, blocked \
             ORDER BY c DESC, name LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![start_ms, end_ms, limit], |row| {
                Ok(TrackerScanRow {
                    client: row.get::<_, String>(0)?,
                    name: row.get::<_, String>(1)?,
                    blocked: row.get::<_, bool>(2)?,
                    count: count(row, 3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Helper for [`Self::period_report`]: run a `(label, COUNT(*))` top-list query
    /// bound to a window, returning `(label, count)` pairs in the query's order.
    fn top_in_window(
        &self,
        sql: &str,
        start_ms: i64,
        end_ms: i64,
        limit: i64,
    ) -> Result<Vec<(String, u64)>, StoreError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![start_ms, end_ms, limit], |row| {
                Ok((row.get::<_, String>(0)?, count(row, 1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// A scheduled report's aggregate over one time window: the counts that make up
/// the summary plus the top-N lists. Produced by [`DuckReader::period_report`];
/// the web layer turns it into the stored, device-name-resolved report.
#[derive(Debug, Clone, Default)]
pub struct PeriodReport {
    pub total: u64,
    pub cache: u64,
    pub upstream: u64,
    pub local: u64,
    pub rewrite: u64,
    pub blocked: u64,
    pub duration_sum_ms: u64,
    pub dnssec_secure: u64,
    pub dnssec_insecure: u64,
    pub dnssec_bogus: u64,
    pub dnssec_indeterminate: u64,
    /// Most-queried names, busiest first: `(name, count)`.
    pub top_domains: Vec<(String, u64)>,
    /// Most-blocked names, busiest first: `(name, count)`.
    pub top_blocked: Vec<(String, u64)>,
    /// Busiest clients, busiest first: `(client address, count)`.
    pub top_clients: Vec<(String, u64)>,
}

/// One row of [`DuckReader::tracker_scan`]: a client, a queried name, whether the
/// lookup was blocked, and how many times it happened in the window.
#[derive(Debug, Clone)]
pub struct TrackerScanRow {
    pub client: String,
    pub name: String,
    pub blocked: bool,
    pub count: u64,
}

/// One row of filtered history: the event, the node it came from (empty for this
/// node), and its `rowid` for keyset pagination.
#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub event: QueryEvent,
    pub origin: String,
    pub rowid: i64,
}

/// The dedup key for a query-log row: the fields that together identify one
/// recorded answer within a node. Matches [`DuckReader::existing_keys`].
pub fn natural_key(event: &QueryEvent) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        event.at_ms,
        event.client,
        event.name,
        i32::from(event.record_type),
        i32::from(event.response_code),
        event.source.label(),
        i64::from(event.duration_ms),
        i32::from(event.answers),
    )
}

/// Escape the LIKE/ILIKE metacharacters in a user-supplied needle so it matches
/// literally under an `ESCAPE '\'` clause. `\` must be escaped first.
fn escape_like(needle: &str) -> String {
    needle
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Build the SQL `CASE` that maps `column` (a duration in ms) to a latency
/// histogram bucket index, from the very bounds the hot view's `latency_bin`
/// uses — so a rehydrated row lands in the same bucket a live one would. The
/// final `ELSE` is the overflow bucket past the last bound.
fn latency_bin_case(column: &str) -> String {
    let mut sql = String::from("CASE");
    for (bin, bound) in LATENCY_BOUNDS_MS.iter().enumerate() {
        sql.push_str(&format!(" WHEN {column} <= {bound} THEN {bin}"));
    }
    sql.push_str(&format!(" ELSE {} END", LATENCY_BOUNDS_MS.len()));
    sql
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
        resolver: row
            .get::<_, Option<String>>(9)?
            .filter(|label| !label.is_empty()),
        dnssec: row
            .get::<_, Option<String>>(10)?
            .as_deref()
            .and_then(DnssecStatus::from_label),
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
            resolver: None,
            dnssec: None,
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
            resolver: None,
            dnssec: None,
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
        // Every query feeds the handling-time figures: 10 + 2 + 0.
        assert_eq!(hour.duration_sum_ms, 12, "all three durations summed");
        // The histogram bins all three: two instant (0 and 2 ms) in the low
        // buckets, the 10 ms one in the <=16 ms bucket.
        assert_eq!(hour.latency_hist[0], 1, "0 ms in the <=1 ms bucket");
        assert_eq!(hour.latency_hist[1], 1, "2 ms in the <=2 ms bucket");
        assert_eq!(hour.latency_hist[4], 1, "10 ms in the <=16 ms bucket");
        assert_eq!(hour.clients.get(&client_a), Some(&2));
        assert_eq!(hour.clients.get(&client_b), Some(&1));
    }

    #[test]
    fn day_buckets_seed_per_client_domain_counts() {
        use std::net::{IpAddr, Ipv4Addr};

        let client_a = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let client_b = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 20));
        let mk = |at_ms, client, name: &str| QueryEvent {
            at_ms,
            client,
            name: name.to_owned(),
            record_type: 1,
            response_code: 0,
            source: QuerySource::Upstream,
            duration_ms: 1,
            answers: 1,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        };
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(5, client_a, "news.example."),
                mk(6, client_a, "news.example."),
                mk(7, client_a, "ads.example."),
                mk(8, client_b, "shop.example."),
            ])
            .unwrap();

        let buckets = store.day_buckets().unwrap();
        assert_eq!(buckets.len(), 1, "all four fall in hour 0");
        let domains = &buckets[0].client_domains;
        // client_a's two distinct domains are seeded with their counts.
        assert_eq!(
            domains.get(&client_a).and_then(|d| d.get("news.example.")),
            Some(&2)
        );
        assert_eq!(
            domains.get(&client_a).and_then(|d| d.get("ads.example.")),
            Some(&1)
        );
        // client_b's single domain, kept apart from client_a.
        assert_eq!(
            domains.get(&client_b).and_then(|d| d.get("shop.example.")),
            Some(&1)
        );
        assert!(
            domains
                .get(&client_b)
                .and_then(|d| d.get("news.example."))
                .is_none()
        );
    }

    #[test]
    fn day_buckets_seed_per_resolver_stats() {
        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let mk = |at_ms, resolver: Option<&str>, duration_ms, source| QueryEvent {
            at_ms,
            client,
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code: 0,
            source,
            duration_ms,
            answers: 1,
            addresses: Vec::new(),
            resolver: resolver.map(str::to_owned),
            dnssec: None,
        };
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(1, Some("dns.quad9.net"), 10, QuerySource::Upstream),
                mk(2, Some("dns.quad9.net"), 20, QuerySource::Upstream),
                mk(
                    3,
                    Some("protective.joindns4.eu"),
                    100,
                    QuerySource::Upstream,
                ),
                // A cache hit carries no resolver and must not seed anything.
                mk(4, None, 0, QuerySource::Cache),
            ])
            .unwrap();

        let buckets = store.day_buckets().unwrap();
        assert_eq!(buckets.len(), 1, "all fall in hour 0");
        let resolvers = &buckets[0].resolvers;
        assert_eq!(
            resolvers.len(),
            2,
            "only the two upstreams, not the cache hit"
        );

        let quad9 = resolvers.get("dns.quad9.net").expect("quad9 seeded");
        assert_eq!(quad9.count, 2);
        assert_eq!(quad9.duration_sum_ms, 30, "10 + 20 ms summed");
        // 10 ms → <=16 bucket, 20 ms → <=32 bucket.
        assert_eq!(quad9.latency_hist[4], 1, "10 ms in the <=16 ms bucket");
        assert_eq!(quad9.latency_hist[5], 1, "20 ms in the <=32 ms bucket");

        let dns4eu = resolvers
            .get("protective.joindns4.eu")
            .expect("dns4eu seeded");
        assert_eq!(dns4eu.count, 1);
        assert_eq!(dns4eu.duration_sum_ms, 100);
        assert_eq!(dns4eu.latency_hist[7], 1, "100 ms in the <=128 ms bucket");
    }

    #[test]
    fn resolver_round_trips_through_the_log() {
        let store = DuckStore::open_in_memory().unwrap();
        let mut upstream = event(1, QuerySource::Upstream, 0);
        upstream.resolver = Some("dns.quad9.net".to_owned());
        let cached = event(2, QuerySource::Cache, 0); // no resolver
        store.insert_batch(&[upstream, cached]).unwrap();

        let recent = store.recent(10).unwrap();
        // Newest first: the cache hit has no resolver, the upstream one does.
        assert_eq!(recent[0].resolver, None);
        assert_eq!(recent[1].resolver.as_deref(), Some("dns.quad9.net"));
    }

    /// A filter that reads everything, newest first, with a generous page size.
    fn all(limit: usize) -> QueryFilter {
        QueryFilter {
            limit,
            ..Default::default()
        }
    }

    #[test]
    fn empty_filter_matches_recent() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                event(1, QuerySource::Upstream, 0),
                event(2, QuerySource::Cache, 0),
                event(3, QuerySource::Blocked, 3),
            ])
            .unwrap();
        let reader = store.reader().unwrap();
        let rows = reader.query(&all(10)).unwrap();
        let times: Vec<i64> = rows.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(times, vec![3, 2, 1], "newest first, same as recent");
    }

    #[test]
    fn filter_by_client_set_and_distinct_clients() {
        let mk = |at_ms, client: IpAddr| QueryEvent {
            at_ms,
            client,
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code: 0,
            source: QuerySource::Upstream,
            duration_ms: 1,
            answers: 1,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        };
        let a = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let b = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 11));
        let c = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 12));
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[mk(1, a), mk(2, b), mk(3, a), mk(4, c)])
            .unwrap();
        let reader = store.reader().unwrap();

        // distinct_clients lists each address once.
        let mut clients = reader.distinct_clients(100).unwrap();
        clients.sort();
        assert_eq!(
            clients,
            vec![
                "192.168.20.10".to_owned(),
                "192.168.20.11".to_owned(),
                "192.168.20.12".to_owned()
            ]
        );

        // A client set restricts to just those addresses (a and c here).
        let rows = reader
            .query(&QueryFilter {
                clients: Some(vec!["192.168.20.10".to_owned(), "192.168.20.12".to_owned()]),
                ..all(10)
            })
            .unwrap();
        let times: Vec<i64> = rows.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(times, vec![4, 3, 1], "only a's and c's rows, newest first");

        // An empty set matches nothing — a device filter that resolved to no
        // observed client must not read as "no filter".
        let none = reader
            .query(&QueryFilter {
                clients: Some(Vec::new()),
                ..all(10)
            })
            .unwrap();
        assert!(none.is_empty(), "empty client set returns no rows");
    }

    #[test]
    fn period_report_aggregates_over_the_window() {
        let mk = |at_ms, client: IpAddr, name: &str, source| QueryEvent {
            at_ms,
            client,
            name: name.to_owned(),
            record_type: 1,
            response_code: 0,
            source,
            duration_ms: 10,
            answers: 1,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        };
        let a = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let b = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 11));
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(5, a, "old.example.", QuerySource::Upstream), // before window
                mk(100, a, "ads.example.", QuerySource::Blocked),
                mk(110, a, "ads.example.", QuerySource::Blocked),
                mk(120, b, "news.example.", QuerySource::Cache),
                mk(200, b, "shop.example.", QuerySource::Upstream),
                mk(300, a, "future.example.", QuerySource::Upstream), // at end bound, excluded
            ])
            .unwrap();
        let reader = store.reader().unwrap();

        // Window [100, 300): the four middle rows, old.example excluded (too early)
        // and future.example excluded (end is exclusive).
        let report = reader.period_report(100, 300, 10).unwrap();
        assert_eq!(report.total, 4);
        assert_eq!(report.blocked, 2);
        assert_eq!(report.cache, 1);
        assert_eq!(report.upstream, 1);
        assert_eq!(report.duration_sum_ms, 40, "4 rows × 10 ms");

        // Top blocked: only ads.example, twice.
        assert_eq!(report.top_blocked, vec![("ads.example.".to_owned(), 2)]);
        // Busiest client is a (two blocked + nothing else in window = 2) vs b (2).
        // a has 2 (both blocked), b has 2 (cache + upstream) → tie broken by address.
        let clients: Vec<&str> = report.top_clients.iter().map(|(c, _)| c.as_str()).collect();
        assert!(clients.contains(&"192.168.20.10"));
        assert!(clients.contains(&"192.168.20.11"));
        // Top domains include ads.example with the highest count.
        assert_eq!(
            report.top_domains.first().unwrap(),
            &("ads.example.".to_owned(), 2)
        );
    }

    #[test]
    fn blocked_counts_by_client_counts_only_blocked_in_window() {
        let mk = |at_ms, client: IpAddr, source| QueryEvent {
            at_ms,
            client,
            name: "x.example.".to_owned(),
            record_type: 1,
            response_code: 0,
            source,
            duration_ms: 1,
            answers: 0,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        };
        let noisy = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let quiet = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(5, noisy, QuerySource::Blocked), // before window
                mk(100, noisy, QuerySource::Blocked),
                mk(110, noisy, QuerySource::Blocked),
                mk(120, noisy, QuerySource::Blocked),
                mk(130, noisy, QuerySource::Upstream), // not blocked, ignored
                mk(140, quiet, QuerySource::Blocked),
                mk(300, noisy, QuerySource::Blocked), // at end bound, excluded
            ])
            .unwrap();
        // A peer's blocked row must not count toward this node's spike view.
        store
            .insert_remote("peer-1", &[mk(150, noisy, QuerySource::Blocked)])
            .unwrap();
        let reader = store.reader().unwrap();

        let counts = reader.blocked_counts_by_client(100, 300, 10).unwrap();
        // noisy: 3 blocked in window (not the upstream, not the out-of-window ones,
        // not the peer row); quiet: 1. Busiest first.
        assert_eq!(
            counts,
            vec![("10.0.0.1".to_owned(), 3), ("10.0.0.2".to_owned(), 1)]
        );
    }

    #[test]
    fn filter_by_source_type_code_resolver_and_time() {
        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let mk = |at_ms, source, record_type, response_code, resolver: Option<&str>| QueryEvent {
            at_ms,
            client,
            name: "example.com.".to_owned(),
            record_type,
            response_code,
            source,
            duration_ms: 1,
            answers: 1,
            addresses: Vec::new(),
            resolver: resolver.map(str::to_owned),
            dnssec: None,
        };
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(10, QuerySource::Blocked, 1, 3, None),
                mk(20, QuerySource::Upstream, 1, 0, Some("dns.quad9.net")),
                mk(30, QuerySource::Upstream, 28, 0, Some("dns.quad9.net")),
                mk(40, QuerySource::Cache, 1, 0, None),
                mk(50, QuerySource::Upstream, 1, 0, Some("one.one.one.one")),
            ])
            .unwrap();
        let reader = store.reader().unwrap();

        // Source OR: blocked and cache only.
        let rows = reader
            .query(&QueryFilter {
                sources: vec![QuerySource::Blocked, QuerySource::Cache],
                ..all(10)
            })
            .unwrap();
        let times: Vec<i64> = rows.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(times, vec![40, 10]);

        // Record type (28 = AAAA).
        let rows = reader
            .query(&QueryFilter {
                record_type: Some(28),
                ..all(10)
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event.at_ms, 30);

        // Response code (3 = NXDOMAIN).
        let rows = reader
            .query(&QueryFilter {
                response_code: Some(3),
                ..all(10)
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event.at_ms, 10);

        // Resolver.
        let rows = reader
            .query(&QueryFilter {
                resolver: Some("dns.quad9.net".to_owned()),
                ..all(10)
            })
            .unwrap();
        let times: Vec<i64> = rows.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(times, vec![30, 20]);

        // Time range (inclusive both ends).
        let rows = reader
            .query(&QueryFilter {
                start_ms: Some(20),
                end_ms: Some(40),
                ..all(10)
            })
            .unwrap();
        let times: Vec<i64> = rows.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(times, vec![40, 30, 20]);
    }

    #[test]
    fn needle_matches_name_or_client() {
        let mk = |at_ms, client: &str, name: &str| QueryEvent {
            at_ms,
            client: client.parse().unwrap(),
            name: name.to_owned(),
            record_type: 1,
            response_code: 0,
            source: QuerySource::Upstream,
            duration_ms: 1,
            answers: 1,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        };
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[
                mk(1, "192.168.20.10", "news.example."),
                mk(2, "192.168.20.20", "ADS.example."),
                mk(3, "10.0.0.5", "shop.test."),
            ])
            .unwrap();
        let reader = store.reader().unwrap();

        // Case-insensitive name match.
        let rows = reader
            .query(&QueryFilter {
                needle: Some("ads".to_owned()),
                ..all(10)
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event.at_ms, 2);

        // Client-address substring match.
        let rows = reader
            .query(&QueryFilter {
                needle: Some("192.168.20".to_owned()),
                ..all(10)
            })
            .unwrap();
        let times: Vec<i64> = rows.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(times, vec![2, 1]);

        // A literal `%` must not act as a wildcard (it is escaped).
        let rows = reader
            .query(&QueryFilter {
                needle: Some("%".to_owned()),
                ..all(10)
            })
            .unwrap();
        assert!(rows.is_empty(), "literal % matches nothing here");
    }

    #[test]
    fn keyset_pagination_walks_every_row_once() {
        let store = DuckStore::open_in_memory().unwrap();
        // Ten rows, several sharing an at_ms, to exercise the rowid tie-break.
        let events: Vec<QueryEvent> = (0..10)
            .map(|i| event(100 + (i / 3), QuerySource::Upstream, 0))
            .collect();
        store.insert_batch(&events).unwrap();
        let reader = store.reader().unwrap();

        // Page through three at a time, following the cursor.
        let mut seen: Vec<i64> = Vec::new();
        let mut before: Option<Cursor> = None;
        loop {
            let rows = reader
                .query(&QueryFilter {
                    before,
                    limit: 3,
                    ..Default::default()
                })
                .unwrap();
            if rows.is_empty() {
                break;
            }
            for row in &rows {
                seen.push(row.event.at_ms);
            }
            let last = rows.last().unwrap();
            before = Some((last.event.at_ms, last.rowid));
            if rows.len() < 3 {
                break;
            }
        }

        assert_eq!(seen.len(), 10, "every row returned exactly once");
        // Monotonically non-increasing (newest first across page boundaries).
        assert!(seen.windows(2).all(|w| w[0] >= w[1]), "stays newest-first");
        let full = reader.query(&all(100)).unwrap();
        let expected: Vec<i64> = full.iter().map(|r| r.event.at_ms).collect();
        assert_eq!(seen, expected, "paginated order matches a single read");
    }

    #[test]
    fn origin_round_trips_and_node_local_views_exclude_peers() {
        let store = DuckStore::open_in_memory().unwrap();
        // Two local rows and one pulled from peer "nodeB".
        store
            .insert_batch(&[
                event(1, QuerySource::Upstream, 0),
                event(2, QuerySource::Cache, 0),
            ])
            .unwrap();
        store
            .insert_remote("nodeB", &[event(3, QuerySource::Upstream, 0)])
            .unwrap();

        // The explorer sees all three, each carrying its origin.
        let reader = store.reader().unwrap();
        let rows = reader.query(&all(10)).unwrap();
        assert_eq!(rows.len(), 3, "explorer unions local and peer rows");
        let peer_row = rows.iter().find(|r| r.event.at_ms == 3).unwrap();
        assert_eq!(peer_row.origin, "nodeB");
        let local_row = rows.iter().find(|r| r.event.at_ms == 1).unwrap();
        assert_eq!(local_row.origin, "", "a local row has an empty origin");

        // Filtering by origin narrows to one node.
        let only_peer = reader
            .query(&QueryFilter {
                origin: Some("nodeB".to_owned()),
                ..all(10)
            })
            .unwrap();
        assert_eq!(only_peer.len(), 1);
        assert_eq!(only_peer[0].event.at_ms, 3);

        // Node-local views (rehydrate sources) count only this node's rows.
        assert_eq!(store.stats().unwrap().total, 2, "stats exclude peer rows");
        assert_eq!(store.recent(10).unwrap().len(), 2, "recent excludes peers");

        // A node serves only its own rows to peers, oldest-first.
        let served = reader.since_local(0, 10).unwrap();
        let served_times: Vec<i64> = served.iter().map(|e| e.at_ms).collect();
        assert_eq!(served_times, vec![1, 2], "own rows only, oldest first");
    }

    #[test]
    fn insert_remote_is_idempotent_by_natural_key() {
        let store = DuckStore::open_in_memory().unwrap();
        let batch = [
            event(5, QuerySource::Upstream, 0),
            event(6, QuerySource::Cache, 0),
        ];
        store.insert_remote("nodeB", &batch).unwrap();

        // Re-ingesting the same rows: the caller filters against existing_keys.
        let reader = store.reader().unwrap();
        let existing = reader.existing_keys("nodeB", 0).unwrap();
        let fresh: Vec<_> = batch
            .iter()
            .filter(|e| !existing.contains(&natural_key(e)))
            .cloned()
            .collect();
        assert!(fresh.is_empty(), "every row is recognised as already held");
        assert_eq!(
            existing.len(),
            2,
            "existing_keys reports both peer rows for this origin"
        );
    }

    #[test]
    fn prune_before_removes_old_rows_all_origins() {
        let store = DuckStore::open_in_memory().unwrap();
        store
            .insert_batch(&[event(100, QuerySource::Upstream, 0)])
            .unwrap();
        store
            .insert_remote("nodeB", &[event(150, QuerySource::Upstream, 0)])
            .unwrap();
        store
            .insert_batch(&[event(900, QuerySource::Upstream, 0)])
            .unwrap();

        let removed = store.prune_before(200).unwrap();
        assert_eq!(removed, 2, "both rows older than the cutoff go, any origin");
        let reader = store.reader().unwrap();
        let left = reader.query(&all(10)).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(
            left[0].event.at_ms, 900,
            "only the row at/after the cutoff stays"
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
