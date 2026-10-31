// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query store the rest of JustDNS talks to. A cheap, cloneable handle
//! records each event into the hot in-memory view at once and streams it to a
//! background task that batches writes into the DuckDB log, so the DNS path is
//! never blocked on disk I/O. Reads are served from the hot view.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use tracing::warn;

use crate::duck::{DuckStore, StoreError};
use crate::event::QueryEvent;
use crate::recorder::{ActivityBucket, Recorder, Stats};

/// Force a flush to DuckDB once this many events have accumulated.
const FLUSH_BATCH: usize = 128;
/// Flush whatever has accumulated at least this often, even below the batch.
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
/// Bound on the writer queue. Past this, new events still reach the hot view
/// but skip the durable log rather than slowing the DNS path.
const CHANNEL_CAPACITY: usize = 4096;

/// A cloneable handle to the query store.
#[derive(Clone)]
pub struct Store {
    recorder: Arc<Recorder>,
    writer: mpsc::Sender<QueryEvent>,
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
        // Rehydrate the hot view from the log so a restart does not start blank.
        for event in duck.recent(capacity)?.into_iter().rev() {
            recorder.record(event);
        }

        let (writer, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let handle = tokio::spawn(writer_loop(duck, rx));
        Ok((Self { recorder, writer }, handle))
    }

    /// Record a handled query: into the hot view at once, and queued for the
    /// durable log. A full queue drops the durable write rather than blocking.
    pub fn record(&self, event: QueryEvent) {
        self.recorder.record(event.clone());
        if self.writer.try_send(event).is_err() {
            warn!("query-log writer queue full; dropping a durable log entry");
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

    /// A snapshot of the running totals.
    pub fn stats(&self) -> Stats {
        self.recorder.stats()
    }

    /// Hourly activity, oldest hour first.
    pub fn activity(&self) -> Vec<ActivityBucket> {
        self.recorder.activity()
    }
}

/// Drain events into batches and persist them, flushing on size or on a timer,
/// and once more when every sender has been dropped.
async fn writer_loop(duck: DuckStore, mut rx: mpsc::Receiver<QueryEvent>) {
    let mut batch: Vec<QueryEvent> = Vec::with_capacity(FLUSH_BATCH);
    let mut ticker = time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            received = rx.recv() => match received {
                Some(event) => {
                    batch.push(event);
                    if batch.len() >= FLUSH_BATCH {
                        flush(&duck, &mut batch);
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
fn flush(duck: &DuckStore, batch: &mut Vec<QueryEvent>) {
    if batch.is_empty() {
        return;
    }
    match duck.insert_batch(batch) {
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
}
