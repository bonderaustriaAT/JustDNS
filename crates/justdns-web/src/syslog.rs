// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in-memory diagnostic-log ring buffer and the runtime-status handle that
//! back the "System" page.
//!
//! JustDNS's operational `tracing` events (listener failures, certificate and
//! feed errors, store problems, …) go to stdout, but stdout is lost on a
//! container restart and is awkward to read. [`RingLayer`] is a `tracing` layer
//! that keeps the most recent events in a bounded [`LogBuffer`], which the admin
//! `GET /api/logs/system` endpoint serves to the UI. [`RuntimeStatus`] is a
//! small shared handle the binary updates (listener state, last feed refresh)
//! and the `GET /api/health` and `GET /api/diag` endpoints read.

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// The current time as an RFC 3339 string in UTC, to the second — the same
/// format the audit log uses, so timestamps read the same across the UI.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The lowercase name of a tracing level, as carried in a [`LogRecord`] and
/// accepted by the `level` query filter.
fn level_name(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    }
}

/// Severity rank, most severe highest, so a `level` filter can keep everything
/// at or above a threshold. Unknown names rank lowest (keep everything).
fn level_rank(name: &str) -> u8 {
    match name {
        "error" => 4,
        "warn" => 3,
        "info" => 2,
        "debug" => 1,
        _ => 0,
    }
}

/// One captured diagnostic log event.
#[derive(Clone, Debug, Serialize)]
pub struct LogRecord {
    /// RFC 3339 UTC timestamp, to the second.
    pub time: String,
    /// Severity: `error` | `warn` | `info` | `debug` | `trace`.
    pub level: &'static str,
    /// The emitting module path / target (e.g. `justdns_dns::endpoints`).
    pub target: String,
    /// The event's human-readable message.
    pub message: String,
    /// Any remaining structured fields, formatted `key=value key=value`; omitted
    /// from the JSON when there are none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fields: String,
}

/// A bounded, thread-safe ring of the most recent [`LogRecord`]s. Cloning shares
/// the one underlying buffer (it is `Arc`-backed), so the writing `tracing`
/// layer and the reading web handler hold the same ring.
#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<LogRecord>>>,
    capacity: usize,
}

impl LogBuffer {
    /// A ring holding at most `capacity` events; the oldest is dropped once it is
    /// full.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity.min(1024)))),
            capacity: capacity.max(1),
        }
    }

    fn push(&self, record: LogRecord) {
        let mut buffer = self.inner.lock().expect("log buffer is not poisoned");
        if buffer.len() == self.capacity {
            buffer.pop_front();
        }
        buffer.push_back(record);
    }

    /// The most recent events first, keeping only those at or above `min_level`
    /// (when given) and capping the result at `limit`.
    pub fn snapshot(&self, min_level: Option<&str>, limit: usize) -> Vec<LogRecord> {
        let floor = min_level.map(level_rank).unwrap_or(0);
        let buffer = self.inner.lock().expect("log buffer is not poisoned");
        buffer
            .iter()
            .rev()
            .filter(|record| level_rank(record.level) >= floor)
            .take(limit)
            .cloned()
            .collect()
    }
}

/// Collects an event's `message` and its remaining fields into strings.
#[derive(Default)]
struct RecordVisitor {
    message: String,
    fields: String,
}

impl Visit for RecordVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            // The message field's value is `fmt::Arguments`; its Debug is the
            // already-rendered text, so this is the plain message.
            let _ = write!(self.message, "{value:?}");
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            let _ = write!(self.fields, "{}={value:?}", field.name());
        }
    }
}

/// A `tracing` layer that appends every event it sees to a [`LogBuffer`]. Add it
/// alongside the stdout and file layers so the UI can show recent diagnostics.
pub struct RingLayer {
    buffer: LogBuffer,
}

impl RingLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = RecordVisitor::default();
        event.record(&mut visitor);
        self.buffer.push(LogRecord {
            time: now_rfc3339(),
            level: level_name(metadata.level()),
            target: metadata.target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
        });
    }
}

/// Runtime control over the global `tracing` verbosity, so an admin can raise
/// the log level temporarily from the System page without restarting.
///
/// The binary owns the `tracing_subscriber` reload handle (that crate's `reload`
/// feature lives in `justdns`, not here); it hands this type a closure that
/// swaps in a new filter spec. This type maps the UI's verbosity labels
/// (`normal`/`debug`/`trace`) to filter specs, applies them through that closure
/// and remembers which label is active so `/api/diag` can report it. The level
/// is deliberately **not** persisted: a restart returns to the shipping default,
/// so a forgotten debug session cannot quietly fill the disk forever.
/// The closure the binary installs to apply a new filter spec to the live
/// subscriber. Boxed so the reload handle's concrete type stays in the binary.
pub type ReloadFn = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

pub struct LogLevelControl {
    current: Mutex<String>,
    reload: ReloadFn,
}

impl LogLevelControl {
    /// A control starting at verbosity label `initial`, applying changes through
    /// `reload` (which the binary builds from its reload handle).
    pub fn new(initial: impl Into<String>, reload: ReloadFn) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new(initial.into()),
            reload,
        })
    }

    /// A control that cannot change the level — for tests and for the (unreached)
    /// case where no reload handle was installed. `set` reports it as unavailable;
    /// `current` stays at `initial`.
    pub fn disabled(initial: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new(initial.into()),
            reload: Box::new(|_| Err("the log level cannot be changed at runtime".into())),
        })
    }

    /// The active verbosity label (`normal` | `debug` | `trace`, or `custom` when
    /// the process was started with an explicit `RUST_LOG`).
    pub fn current(&self) -> String {
        self.current
            .lock()
            .expect("log level is not poisoned")
            .clone()
    }

    /// Switch the active verbosity. `level` is one of `normal`/`debug`/`trace`;
    /// anything else is rejected. On success the new label is remembered.
    pub fn set(&self, level: &str) -> Result<(), String> {
        let spec = filter_spec(level)?;
        (self.reload)(&spec)?;
        *self.current.lock().expect("log level is not poisoned") = level.to_string();
        Ok(())
    }
}

/// Map a UI verbosity label to a `tracing` `EnvFilter` spec. `normal` is the
/// shipping default (everything at info). `debug`/`trace` raise JustDNS's own
/// crates while leaving dependencies at info, so the extra detail is about
/// JustDNS rather than a flood from hickory, rustls and friends.
pub fn filter_spec(level: &str) -> Result<String, String> {
    let spec = match level {
        "normal" => "info",
        "debug" => "info,justdns=debug,justdns_dns=debug,justdns_lists=debug,justdns_web=debug",
        "trace" => "info,justdns=trace,justdns_dns=trace,justdns_lists=trace,justdns_web=trace",
        other => return Err(format!("unknown log level: {other:?}")),
    };
    Ok(spec.to_string())
}

/// The last subscribed-feed refresh: when it ran and whether it failed.
#[derive(Clone, Default)]
pub struct FeedStatus {
    /// RFC 3339 time of the last refresh attempt, or `None` before the first.
    pub last_refresh: Option<String>,
    /// The error from the last refresh, or `None` when it succeeded.
    pub last_error: Option<String>,
}

/// A small shared snapshot of live runtime state, updated by the binary and read
/// by the health and diagnostics endpoints. Cheap to clone (`Arc`-backed).
pub struct RuntimeStatus {
    started: Instant,
    version: String,
    udp: AtomicBool,
    tcp: AtomicBool,
    dot: AtomicBool,
    doh: AtomicBool,
    feed: Mutex<FeedStatus>,
}

impl RuntimeStatus {
    /// A status handle reporting `version`, with the clock started now and every
    /// listener initially marked down (the binary flips them as they come up).
    pub fn new(version: String) -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            version,
            udp: AtomicBool::new(false),
            tcp: AtomicBool::new(false),
            dot: AtomicBool::new(false),
            doh: AtomicBool::new(false),
            feed: Mutex::new(FeedStatus::default()),
        })
    }

    /// Seconds since the process started serving.
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// The running version string (from `APP_VERSION`, falling back to the crate
    /// version at build time).
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mark the plain-DNS (UDP and TCP) listeners up or down together.
    pub fn set_plain_dns(&self, up: bool) {
        self.udp.store(up, Ordering::Relaxed);
        self.tcp.store(up, Ordering::Relaxed);
    }

    /// Mark the DNS-over-TLS listener up or down.
    pub fn set_dot(&self, up: bool) {
        self.dot.store(up, Ordering::Relaxed);
    }

    /// Mark the DNS-over-HTTPS listener up or down.
    pub fn set_doh(&self, up: bool) {
        self.doh.store(up, Ordering::Relaxed);
    }

    /// The listener state as `(udp, tcp, dot, doh)`.
    pub fn listeners(&self) -> (bool, bool, bool, bool) {
        (
            self.udp.load(Ordering::Relaxed),
            self.tcp.load(Ordering::Relaxed),
            self.dot.load(Ordering::Relaxed),
            self.doh.load(Ordering::Relaxed),
        )
    }

    /// Record that a feed refresh just ran, with its error (or `None` on success).
    pub fn record_feed_refresh(&self, error: Option<String>) {
        let mut feed = self.feed.lock().expect("feed status is not poisoned");
        feed.last_refresh = Some(now_rfc3339());
        feed.last_error = error;
    }

    /// A copy of the last-feed-refresh state.
    pub fn feed(&self) -> FeedStatus {
        self.feed
            .lock()
            .expect("feed status is not poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(level: &'static str, message: &str) -> LogRecord {
        LogRecord {
            time: "2026-11-06T00:00:00Z".into(),
            level,
            target: "test".into(),
            message: message.into(),
            fields: String::new(),
        }
    }

    #[test]
    fn ring_drops_oldest_past_capacity() {
        let buffer = LogBuffer::new(2);
        buffer.push(record("info", "one"));
        buffer.push(record("info", "two"));
        buffer.push(record("info", "three"));

        let snapshot = buffer.snapshot(None, 10);
        assert_eq!(snapshot.len(), 2);
        // Newest first; "one" was evicted.
        assert_eq!(snapshot[0].message, "three");
        assert_eq!(snapshot[1].message, "two");
    }

    #[test]
    fn snapshot_filters_by_minimum_level_and_limit() {
        let buffer = LogBuffer::new(10);
        buffer.push(record("info", "i"));
        buffer.push(record("warn", "w"));
        buffer.push(record("error", "e"));

        // Only warn and above, newest first.
        let warnings = buffer.snapshot(Some("warn"), 10);
        assert_eq!(
            warnings
                .iter()
                .map(|r| r.message.as_str())
                .collect::<Vec<_>>(),
            vec!["e", "w"],
        );

        // Limit caps the count.
        assert_eq!(buffer.snapshot(None, 1).len(), 1);
    }

    #[test]
    fn filter_spec_maps_known_levels_and_rejects_others() {
        assert_eq!(filter_spec("normal").unwrap(), "info");
        assert!(filter_spec("debug").unwrap().contains("justdns_dns=debug"));
        assert!(filter_spec("trace").unwrap().contains("justdns=trace"));
        assert!(filter_spec("loud").is_err());
    }

    #[test]
    fn disabled_control_keeps_its_label_and_refuses_to_change() {
        let control = LogLevelControl::disabled("normal");
        assert_eq!(control.current(), "normal");
        assert!(control.set("debug").is_err());
        // A rejected change must not move the reported label.
        assert_eq!(control.current(), "normal");
    }

    #[test]
    fn control_applies_a_valid_level_and_remembers_it() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static APPLIED: AtomicUsize = AtomicUsize::new(0);
        let control = LogLevelControl::new(
            "normal",
            Box::new(|_spec| {
                APPLIED.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }),
        );
        control.set("debug").unwrap();
        assert_eq!(control.current(), "debug");
        assert_eq!(APPLIED.load(Ordering::Relaxed), 1);
        // An unknown level is rejected before the reload closure runs.
        assert!(control.set("loud").is_err());
        assert_eq!(APPLIED.load(Ordering::Relaxed), 1);
        assert_eq!(control.current(), "debug");
    }
}
