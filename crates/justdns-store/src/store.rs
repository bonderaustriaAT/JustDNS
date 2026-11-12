// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query store the rest of JustDNS talks to. A cheap, cloneable handle
//! records each event into the hot in-memory view at once and streams it to a
//! background task that batches writes into the DuckDB log, so the DNS path is
//! never blocked on disk I/O. Reads are served from the hot view.

use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use tracing::warn;

use crate::duck::{
    Cursor, DuckReader, DuckStore, HistoryRow, PeriodReport, QueryFilter, StoreError,
    TrackerScanRow, natural_key,
};
use crate::event::QueryEvent;
use crate::recorder::{
    ActivityBucket, ClientActivity, ClientCount, DayStats, DomainCount, Recorder, ResolverStat,
    Stats,
};

/// Force a flush to DuckDB once this many events have accumulated.
const FLUSH_BATCH: usize = 128;
/// Flush whatever has accumulated at least this often, even below the batch.
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
/// Bound on the writer queue. Past this, new events still reach the hot view
/// but skip the durable log rather than slowing the DNS path.
const CHANNEL_CAPACITY: usize = 4096;

/// A message to the background writer: a log row (event + its origin, empty for a
/// locally answered query) or a retention prune up to a cutoff.
enum WriterMsg {
    Row(QueryEvent, String),
    Prune(i64),
}

/// A cloneable handle to the query store.
#[derive(Clone)]
pub struct Store {
    recorder: Arc<Recorder>,
    writer: mpsc::Sender<WriterMsg>,
    /// A second DuckDB connection for reading the full history. Cloned from the
    /// writer's connection before it was moved into the writer task, so it sees
    /// flushed rows. Behind a `Mutex` because a DuckDB connection is `!Sync`; a
    /// history read is a quick, blocking statement and never held across await.
    reader: Arc<Mutex<DuckReader>>,
}

impl Store {
    /// Open the log at `path`, keep `capacity` events in the hot view, and spawn
    /// the background writer. The returned [`JoinHandle`] resolves once the
    /// writer has flushed its final batch after the last handle is dropped.
    pub fn open(
        path: impl AsRef<Path>,
        capacity: usize,
    ) -> Result<(Self, JoinHandle<()>), StoreError> {
        Self::spawn(DuckStore::open(path)?, capacity)
    }

    /// Like [`Store::open`], but backed by an in-memory DuckDB log (tests).
    pub fn in_memory(capacity: usize) -> Result<(Self, JoinHandle<()>), StoreError> {
        Self::spawn(DuckStore::open_in_memory()?, capacity)
    }

    fn spawn(duck: DuckStore, capacity: usize) -> Result<(Self, JoinHandle<()>), StoreError> {
        let recorder = Arc::new(Recorder::new(capacity));
        // Rehydrate the hot view from the log so a restart continues the figures
        // rather than starting blank: the lifetime totals and the day's activity
        // come from the whole log, the recent ring from the newest events.
        recorder.rehydrate(duck.recent(capacity)?, duck.stats()?, duck.day_buckets()?);

        // Clone the read connection off the same database before the original is
        // moved into the writer task — afterwards the web layer cannot reach it.
        let reader = Arc::new(Mutex::new(duck.reader()?));
        let (writer, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let handle = tokio::spawn(writer_loop(duck, rx));
        Ok((
            Self {
                recorder,
                writer,
                reader,
            },
            handle,
        ))
    }

    /// Record a handled query: into the hot view at once, and queued for the
    /// durable log with an empty origin (this node). A full queue drops the durable
    /// write rather than blocking.
    pub fn record(&self, event: QueryEvent) {
        self.recorder.record(event.clone());
        if self
            .writer
            .try_send(WriterMsg::Row(event, String::new()))
            .is_err()
        {
            warn!("query-log writer queue full; dropping a durable log entry");
        }
    }

    /// Ingest a peer's events (pulled over the HA mesh), tagged with that peer's
    /// `origin` node id, into the **durable log only** — never the hot in-memory
    /// view, so the dashboard and its running totals stay this node's own (the
    /// explorer unions all nodes). Idempotent: `events` must be oldest-first; rows
    /// already held are skipped (the puller re-pulls its watermark inclusively).
    /// Returns the highest `at_ms` durably queued, to advance the watermark; `None`
    /// when nothing was queued. The hot view deliberately never sees these, and a
    /// restart rehydrates the dashboard from `origin = ''` alone.
    pub fn ingest_remote(&self, origin: &str, events: &[QueryEvent]) -> Option<i64> {
        let since_ms = events.first()?.at_ms;
        let mut seen = {
            let reader = self.reader.lock().expect("history reader mutex poisoned");
            reader.existing_keys(origin, since_ms).unwrap_or_default()
        };
        let mut watermark = None;
        for event in events {
            let key = natural_key(event);
            if seen.contains(&key) {
                // Already held (or a within-batch duplicate): count it as consumed.
                watermark = Some(event.at_ms);
                continue;
            }
            if self
                .writer
                .try_send(WriterMsg::Row(event.clone(), origin.to_owned()))
                .is_err()
            {
                warn!("query-log writer queue full; pausing peer ingest for this cycle");
                break;
            }
            seen.insert(key);
            watermark = Some(event.at_ms);
        }
        watermark
    }

    /// Queue a retention prune: delete every durable row older than `cutoff_ms`
    /// (all origins). Best-effort and processed in order with the writes, so it
    /// never races the appender. A full queue drops it; the next cycle retries.
    pub fn prune(&self, cutoff_ms: i64) {
        if self.writer.try_send(WriterMsg::Prune(cutoff_ms)).is_err() {
            warn!("query-log writer queue full; skipping a retention prune");
        }
    }

    /// The most recent events, newest first, up to `limit`.
    pub fn recent(&self, limit: usize) -> Vec<QueryEvent> {
        self.recorder.recent(limit)
    }

    /// The most recent events whose name or client address contains `needle`
    /// (case-insensitive), newest first, up to `limit`.
    pub fn search(&self, needle: &str, limit: usize) -> Vec<QueryEvent> {
        self.recorder.search(needle, limit)
    }

    /// Run a filtered, paginated query over the **full durable history** in
    /// DuckDB (not just the in-memory ring), newest first. Returns the matching
    /// events and, when a full page came back, the [`Cursor`] to pass as
    /// `filter.before` for the next page; `None` means the last page. The read
    /// is a quick blocking statement, so callers on an async runtime should wrap
    /// this in `spawn_blocking`.
    pub fn history(
        &self,
        filter: &QueryFilter,
    ) -> Result<(Vec<HistoryRow>, Option<Cursor>), StoreError> {
        let rows = {
            let reader = self.reader.lock().expect("history reader mutex poisoned");
            reader.query(filter)?
        };
        // A full page means there may be more; the cursor is the last row's key.
        let next = (rows.len() == filter.limit)
            .then(|| rows.last().map(|row| (row.event.at_ms, row.rowid)))
            .flatten();
        Ok((rows, next))
    }

    /// This node's own (locally answered) events with `at_ms >= since_ms`, oldest
    /// first, up to `limit` — what a node serves to a peer over the HA mesh. Only
    /// own rows, so a four-node mesh never re-forwards another node's events. The
    /// read is a quick blocking statement; callers on an async runtime should wrap
    /// it in `spawn_blocking`.
    pub fn queries_since_local(
        &self,
        since_ms: i64,
        limit: usize,
    ) -> Result<Vec<QueryEvent>, StoreError> {
        let reader = self.reader.lock().expect("history reader mutex poisoned");
        reader.since_local(since_ms, limit)
    }

    /// Every distinct client address in the durable log, up to `limit`. The
    /// explorer resolves these against the device catalogue to turn a device- or
    /// group-filter (which may be a CIDR range) into the exact addresses to match.
    /// A quick blocking read; wrap async callers in `spawn_blocking`.
    pub fn distinct_clients(&self, limit: usize) -> Result<Vec<String>, StoreError> {
        let reader = self.reader.lock().expect("history reader mutex poisoned");
        reader.distinct_clients(limit)
    }

    /// Aggregate the durable log over `[start_ms, end_ms)` for a scheduled report —
    /// totals, source breakdown, DNSSEC verdicts, summed handling time and the top
    /// `top_n` domains / blocked domains / clients, over the whole cluster. A quick
    /// blocking read; wrap async callers in `spawn_blocking`.
    pub fn period_report(
        &self,
        start_ms: i64,
        end_ms: i64,
        top_n: usize,
    ) -> Result<PeriodReport, StoreError> {
        let reader = self.reader.lock().expect("history reader mutex poisoned");
        reader.period_report(start_ms, end_ms, top_n)
    }

    /// How many **blocked** lookups each client made over `[start_ms, end_ms)`,
    /// busiest first, up to `limit` clients — the spike alert's input. Node-local
    /// only. A quick blocking read; wrap async callers in `spawn_blocking`.
    pub fn blocked_counts_by_client(
        &self,
        start_ms: i64,
        end_ms: i64,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, StoreError> {
        let reader = self.reader.lock().expect("history reader mutex poisoned");
        reader.blocked_counts_by_client(start_ms, end_ms, limit)
    }

    /// Per-client, per-name lookup counts over `[start_ms, end_ms)` for the
    /// privacy dashboard — which names each device reached and whether they were
    /// blocked — up to `limit` rows, busiest first. Node-local only. A quick
    /// blocking read; wrap async callers in `spawn_blocking`.
    pub fn tracker_scan(
        &self,
        start_ms: i64,
        end_ms: i64,
        limit: usize,
    ) -> Result<Vec<TrackerScanRow>, StoreError> {
        let reader = self.reader.lock().expect("history reader mutex poisoned");
        reader.tracker_scan(start_ms, end_ms, limit)
    }

    /// A snapshot of the running totals.
    pub fn stats(&self) -> Stats {
        self.recorder.stats()
    }

    /// Hourly activity, oldest hour first.
    pub fn activity(&self) -> Vec<ActivityBucket> {
        self.recorder.activity()
    }

    /// Totals over the last day: queries handled, how many were blocked, and the
    /// mean handling time.
    pub fn day_stats(&self) -> DayStats {
        self.recorder.day_stats()
    }

    /// The busiest clients over the last day, most queries first, up to `n`.
    pub fn top_clients(&self, n: usize) -> Vec<ClientCount> {
        self.recorder.top_clients(n)
    }

    /// The domains `client` looked up most over the last day, most lookups first,
    /// up to `n`. Empty when the client made no recorded lookups in the window.
    pub fn top_domains_for_client(&self, client: std::net::IpAddr, n: usize) -> Vec<DomainCount> {
        self.recorder.top_domains_for_client(client, n)
    }

    /// The upstream resolvers that answered over the last day, busiest first, up
    /// to `n`, each with its answer count and handling-time figures. Empty when
    /// no query was forwarded to an upstream in the window.
    pub fn resolver_stats(&self, n: usize) -> Vec<ResolverStat> {
        self.recorder.resolver_stats(n)
    }

    /// Every client seen over the last day, with its query count and when it was
    /// last seen (most recently seen first). Feeds the devices view.
    pub fn client_activity(&self) -> Vec<ClientActivity> {
        self.recorder.client_activity()
    }
}

/// Drain events into batches and persist them, flushing on size or on a timer,
/// and once more when every sender has been dropped.
async fn writer_loop(duck: DuckStore, mut rx: mpsc::Receiver<WriterMsg>) {
    let mut batch: Vec<(QueryEvent, String)> = Vec::with_capacity(FLUSH_BATCH);
    let mut ticker = time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            received = rx.recv() => match received {
                Some(WriterMsg::Row(event, origin)) => {
                    batch.push((event, origin));
                    if batch.len() >= FLUSH_BATCH {
                        flush(&duck, &mut batch);
                    }
                }
                // Flush first so the prune sees every queued row, then delete the
                // rows past the retention window on the same connection (no race
                // with the appender, which only this task drives).
                Some(WriterMsg::Prune(cutoff_ms)) => {
                    flush(&duck, &mut batch);
                    match duck.prune_before(cutoff_ms) {
                        Ok(removed) if removed > 0 => {
                            tracing::debug!(removed, "pruned query-log rows past retention")
                        }
                        Ok(_) => {}
                        Err(error) => warn!(%error, "failed to prune the query log"),
                    }
                }
                None => {
                    flush(&duck, &mut batch);
                    break;
                }
            },
            _ = ticker.tick() => flush(&duck, &mut batch),
        }
    }
}

/// Write the pending batch to the log, keeping it even if a write fails so the
/// hot view and the log cannot silently diverge on a transient error.
fn flush(duck: &DuckStore, batch: &mut Vec<(QueryEvent, String)>) {
    if batch.is_empty() {
        return;
    }
    match duck.insert_mixed(batch) {
        Ok(()) => batch.clear(),
        Err(error) => warn!(%error, "failed to write a batch to the query log"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn event(at_ms: i64) -> QueryEvent {
        QueryEvent {
            at_ms,
            client: IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10)),
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code: 0,
            source: crate::QuerySource::Upstream,
            duration_ms: 3,
            answers: 1,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        }
    }

    fn temp_path() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "justdns-store-glue-{}-{}.duckdb",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[tokio::test]
    async fn record_updates_the_hot_view_immediately() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(event(1));
        store.record(event(2));

        assert_eq!(store.stats().total, 2);
        assert_eq!(store.recent(10).len(), 2);
    }

    #[tokio::test]
    async fn ingest_remote_fills_the_log_but_not_the_hot_view() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(event(1)); // a local query
        let peer = [event(2), event(3)];
        let watermark = store.ingest_remote("nodeB", &peer);
        assert_eq!(watermark, Some(3), "watermark is the newest ingested at_ms");

        // The hot view (dashboard totals) stays this node's own: just the local one.
        assert_eq!(store.stats().total, 1, "peer rows never reach the hot view");
        assert_eq!(store.recent(10).len(), 1);

        // Re-ingesting reports the same watermark so the puller advances; the hot
        // view is still untouched. (DB-level dedup by natural key is covered in the
        // duck tests, which flush and read back.)
        assert_eq!(store.ingest_remote("nodeB", &peer), Some(3));
        assert_eq!(store.stats().total, 1, "still only the local query");
    }

    #[tokio::test]
    async fn events_persist_and_rehydrate_after_restart() {
        let path = temp_path();
        {
            let (store, writer) = Store::open(&path, 100).unwrap();
            store.record(event(1));
            store.record(event(2));
            drop(store); // close the writer queue
            writer.await.unwrap(); // let the final batch flush
        }

        let (reopened, _writer) = Store::open(&path, 100).unwrap();
        assert_eq!(
            reopened.stats().total,
            2,
            "counters rehydrated from the log"
        );
        assert_eq!(reopened.recent(10).len(), 2);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("duckdb.wal"));
    }

    #[tokio::test]
    async fn lifetime_totals_survive_beyond_the_recent_window() {
        let path = temp_path();
        {
            // A hot view that only keeps 2 events, but 5 go to the durable log.
            let (store, writer) = Store::open(&path, 2).unwrap();
            for at in 1..=5 {
                store.record(event(at));
            }
            drop(store);
            writer.await.unwrap();
        }

        let (reopened, _writer) = Store::open(&path, 2).unwrap();
        // The total reflects the whole log, not just the rehydrated window.
        assert_eq!(
            reopened.stats().total,
            5,
            "running totals survive a restart (and an update) in full"
        );
        // The recent ring is still bounded by the capacity.
        assert_eq!(reopened.recent(10).len(), 2);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("duckdb.wal"));
    }
}
