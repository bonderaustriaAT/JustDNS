// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in-memory hot view the dashboard reads: the newest queries, running
//! counters, and per-hour activity. It is kept small and guarded by a single
//! short-lived lock so the DNS path can record into it without blocking on I/O.
//!
//! Durable history lives in the DuckDB log; this view only answers "what just
//! happened", so it holds a bounded ring of recent events and the last day of
//! hourly counts.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Mutex;

use serde::Serialize;

use crate::event::{QueryEvent, QuerySource};

/// One hour of wall-clock time, in milliseconds.
const HOUR_MS: i64 = 3_600_000;
/// How many hours of activity to keep.
const ACTIVITY_HOURS: usize = 24;
/// Most distinct client addresses tracked per hour. A busy or open resolver can
/// see many clients; past this cap an hour stops taking on new ones, so the
/// per-hour tally stays bounded. On a home LAN the real count is far below it.
const MAX_CLIENTS_PER_HOUR: usize = 1024;

/// Response code for a server failure.
const RCODE_SERVFAIL: u16 = 2;
/// Response code for a non-existent name.
const RCODE_NXDOMAIN: u16 = 3;

/// Upper bounds (ms) of the latency histogram's buckets. A sample lands in the
/// first bucket whose bound it does not exceed, or the overflow bucket past the
/// last bound. Powers of two keep the buckets meaningful across the whole range
/// a resolver shows, from an instant cache hit in the first bucket to a slow
/// upstream in the tail. Every handled query is recorded, so the median reflects
/// the typical (usually cached, near-zero) answer while the p95 still exposes the
/// slow tail. Kept `pub(crate)` so the DuckDB rehydration bins exactly the same.
pub(crate) const LATENCY_BOUNDS_MS: [u64; 13] =
    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
/// Histogram bucket count: one per bound plus a final overflow bucket.
pub(crate) const LATENCY_BINS: usize = LATENCY_BOUNDS_MS.len() + 1;

/// The histogram bucket a latency falls in: the first bound it does not exceed,
/// else the overflow bucket (the last index).
pub(crate) fn latency_bin(duration_ms: u64) -> usize {
    LATENCY_BOUNDS_MS
        .iter()
        .position(|&bound| duration_ms <= bound)
        .unwrap_or(LATENCY_BOUNDS_MS.len())
}

/// The `[lower, upper]` millisecond bounds of histogram bucket `bin`, used to
/// place an interpolated percentile inside it. The overflow bucket has no finite
/// upper bound, so it reports its lower bound for both.
fn bin_bounds(bin: usize) -> (f64, f64) {
    let lo = if bin == 0 {
        0.0
    } else {
        LATENCY_BOUNDS_MS[bin - 1] as f64
    };
    let hi = LATENCY_BOUNDS_MS.get(bin).map_or(lo, |&bound| bound as f64);
    (lo, hi)
}

/// The `p`-quantile (0.0–1.0) of a merged handling-time histogram, in ms,
/// estimated by linear interpolation inside the bucket the rank falls in. Zero
/// when there are no samples. Approximate to the bucket's width, which is plenty
/// for a dashboard figure and needs only the bounded per-hour histograms.
fn percentile(hist: &[u64; LATENCY_BINS], total: u64, p: f64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    // 1-based rank of the target sample, at least the first one.
    let target = (p * total as f64).ceil().max(1.0) as u64;
    let mut cumulative = 0u64;
    for (bin, &n) in hist.iter().enumerate() {
        if n == 0 {
            continue;
        }
        if cumulative + n >= target {
            let (lo, hi) = bin_bounds(bin);
            let within = (target - cumulative) as f64 / n as f64;
            return lo + within * (hi - lo);
        }
        cumulative += n;
    }
    bin_bounds(LATENCY_BINS - 1).1
}

/// Running totals over every query seen since start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Queries handled in total.
    pub total: u64,
    /// Queries served from the cache.
    pub cache_hits: u64,
    /// Queries forwarded to an upstream.
    pub upstream: u64,
    /// Queries answered locally without a lookup.
    pub local: u64,
    /// Queries refused by the blocking engine.
    pub blocked: u64,
    /// Queries answered with NXDOMAIN.
    pub nxdomain: u64,
    /// Queries answered with SERVFAIL.
    pub servfail: u64,
}

/// The number of queries handled in one wall-clock hour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ActivityBucket {
    /// Start of the hour, in milliseconds since the Unix epoch.
    pub hour_ms: i64,
    /// Queries handled during that hour.
    pub count: u64,
}

/// Figures over the last day's worth of activity buckets, for the dashboard's
/// statistics: how many queries were handled, how many of them were blocked,
/// and the mean handling time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct DayStats {
    /// Queries handled across the kept hours.
    pub total: u64,
    /// How many of them were refused by the blocking engine.
    pub blocked: u64,
    /// Mean handling time across all handled queries, in milliseconds. The many
    /// instant cache/blocked/local answers count too, so a cache-heavy day reads
    /// as fast as it really is. Zero when no query was handled.
    pub avg_duration_ms: f64,
    /// Median (50th percentile) handling time, in milliseconds — the typical
    /// query, close to zero when most are served from cache. Robust against a
    /// single slow outlier, which the mean alone would let dominate a quiet hour.
    pub median_duration_ms: f64,
    /// 95th-percentile handling time, in milliseconds: the slow tail most queries
    /// still beat, so the occasional upstream lookup is visible without skewing
    /// the headline figure.
    pub p95_duration_ms: f64,
}

/// One client and how many queries it made over the kept hours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ClientCount {
    /// The client's address.
    pub client: IpAddr,
    /// Queries it made across the kept hours.
    pub count: u64,
}

/// One client's recent footprint, for the devices view: how many queries it made
/// over the kept hours and when it was last seen. The last-seen time is precise
/// for a client still in the recent ring; for one seen earlier in the day it
/// falls back to the start of the most recent hour it appears in, which is enough
/// to tell "active now" from "seen earlier today".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ClientActivity {
    /// The client's address.
    pub client: IpAddr,
    /// Queries it made across the kept hours.
    pub count: u64,
    /// When it was last seen, in milliseconds since the Unix epoch.
    pub last_seen_ms: i64,
}

/// One wall-clock hour of activity, with the extra tallies the statistics view
/// needs on top of the plain per-hour count: blocked queries, the summed
/// handling time (to average later), and a bounded per-client count. Lives only
/// in the hot view; projected to [`ActivityBucket`] for the activity chart.
#[derive(Clone, Debug)]
pub(crate) struct HourBucket {
    pub(crate) hour_ms: i64,
    pub(crate) count: u64,
    pub(crate) blocked: u64,
    /// Summed handling time of every query this hour, in milliseconds (for the
    /// mean). Instant cache/blocked answers count too, so a cache-heavy hour
    /// reads as fast as it really is.
    pub(crate) duration_sum_ms: u64,
    /// Distribution of every query's handling time across [`LATENCY_BINS`]
    /// buckets (for the median and p95). Merged across hours to estimate the
    /// day's percentiles; the many cache hits cluster in the first bucket, so the
    /// median stays near zero while the p95 still shows the slow tail.
    pub(crate) latency_hist: [u64; LATENCY_BINS],
    pub(crate) clients: HashMap<IpAddr, u64>,
}

impl HourBucket {
    /// A fresh, empty bucket for `hour_ms`.
    pub(crate) fn new(hour_ms: i64) -> Self {
        Self {
            hour_ms,
            count: 0,
            blocked: 0,
            duration_sum_ms: 0,
            latency_hist: [0; LATENCY_BINS],
            clients: HashMap::new(),
        }
    }

    /// Record one query's handling time into the hour's mean sum and latency
    /// histogram. Called for every handled query, cache hits included.
    pub(crate) fn add_latency(&mut self, duration_ms: u64) {
        self.duration_sum_ms += duration_ms;
        self.latency_hist[latency_bin(duration_ms)] += 1;
    }

    /// Count one client visit, respecting the per-hour cap: an address already
    /// tracked keeps counting, a new one is dropped once the hour is full.
    fn bump_client(&mut self, client: IpAddr) {
        self.add_client(client, 1);
    }

    /// Add `count` visits for `client`, respecting the per-hour cap. Used both by
    /// [`Self::bump_client`] (count 1, live) and by rehydration, which seeds
    /// already-aggregated per-client counts from the durable log.
    pub(crate) fn add_client(&mut self, client: IpAddr, count: u64) {
        if let Some(existing) = self.clients.get_mut(&client) {
            *existing += count;
        } else if self.clients.len() < MAX_CLIENTS_PER_HOUR {
            self.clients.insert(client, count);
        }
    }
}

struct State {
    recent: VecDeque<QueryEvent>,
    stats: Stats,
    activity: VecDeque<HourBucket>,
}

/// A bounded, lock-guarded view of recent query activity.
pub struct Recorder {
    capacity: usize,
    state: Mutex<State>,
}

impl Recorder {
    /// Create a recorder that keeps at most `capacity` recent events.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(State {
                recent: VecDeque::new(),
                stats: Stats::default(),
                activity: VecDeque::new(),
            }),
        }
    }

    /// Seed the view from the durable log on startup so a restart continues the
    /// figures instead of counting from zero: lifetime `stats` from the whole
    /// log, `activity` for the last day, and `recent` from the newest events.
    ///
    /// `recent_newest_first` is what the log returns (newest first); the ring is
    /// stored oldest-at-front, the way [`Self::record`] builds it. The totals
    /// are taken as given rather than recomputed from the recent window, so they
    /// reflect the entire history even though only the window is kept in memory.
    pub(crate) fn rehydrate(
        &self,
        recent_newest_first: Vec<QueryEvent>,
        stats: Stats,
        activity: Vec<HourBucket>,
    ) {
        let mut state = self.state.lock().expect("recorder lock poisoned");
        state.stats = stats;
        state.activity = activity.into_iter().collect();
        let mut ring: VecDeque<QueryEvent> = recent_newest_first.into_iter().rev().collect();
        while ring.len() > self.capacity {
            ring.pop_front();
        }
        state.recent = ring;
    }

    /// Record a handled query, evicting the oldest event once full.
    pub fn record(&self, event: QueryEvent) {
        let mut state = self.state.lock().expect("recorder lock poisoned");

        state.stats.total += 1;
        match event.source {
            QuerySource::Cache => state.stats.cache_hits += 1,
            QuerySource::Upstream => state.stats.upstream += 1,
            // A rewrite is answered locally too, so it counts toward the same
            // "local" total; the log keeps the finer source for each entry.
            QuerySource::Local | QuerySource::Rewrite => state.stats.local += 1,
            QuerySource::Blocked => state.stats.blocked += 1,
        }
        match event.response_code {
            RCODE_NXDOMAIN => state.stats.nxdomain += 1,
            RCODE_SERVFAIL => state.stats.servfail += 1,
            _ => {}
        }

        let blocked = event.source == QuerySource::Blocked;
        bump_activity(
            &mut state.activity,
            event.at_ms,
            blocked,
            event.duration_ms,
            event.client,
        );

        state.recent.push_back(event);
        while state.recent.len() > self.capacity {
            state.recent.pop_front();
        }
    }

    /// The most recent events, newest first, up to `limit`.
    pub fn recent(&self, limit: usize) -> Vec<QueryEvent> {
        let state = self.state.lock().expect("recorder lock poisoned");
        state.recent.iter().rev().take(limit).cloned().collect()
    }

    /// The most recent events whose queried name or client address contains
    /// `needle` (case-insensitive), newest first, up to `limit`.
    pub fn search(&self, needle: &str, limit: usize) -> Vec<QueryEvent> {
        let needle = needle.to_lowercase();
        let state = self.state.lock().expect("recorder lock poisoned");
        state
            .recent
            .iter()
            .rev()
            .filter(|event| {
                event.name.to_lowercase().contains(&needle)
                    || event.client.to_string().contains(&needle)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// A snapshot of the running totals.
    pub fn stats(&self) -> Stats {
        self.state.lock().expect("recorder lock poisoned").stats
    }

    /// Hourly activity, oldest hour first — the plain per-hour counts the
    /// activity chart draws, projected from the richer internal buckets.
    pub fn activity(&self) -> Vec<ActivityBucket> {
        let state = self.state.lock().expect("recorder lock poisoned");
        state
            .activity
            .iter()
            .map(|bucket| ActivityBucket {
                hour_ms: bucket.hour_ms,
                count: bucket.count,
            })
            .collect()
    }

    /// Totals over the kept hours (the last day): queries handled, how many were
    /// blocked, and the handling-time figures across every query (mean, median,
    /// p95) — cache hits included, so a cache-heavy day reads as fast as it is.
    pub fn day_stats(&self) -> DayStats {
        let state = self.state.lock().expect("recorder lock poisoned");
        let mut total = 0u64;
        let mut blocked = 0u64;
        let mut duration_sum = 0u64;
        let mut hist = [0u64; LATENCY_BINS];
        for bucket in &state.activity {
            total += bucket.count;
            blocked += bucket.blocked;
            duration_sum += bucket.duration_sum_ms;
            for (bin, &n) in bucket.latency_hist.iter().enumerate() {
                hist[bin] += n;
            }
        }
        let avg_duration_ms = if total == 0 {
            0.0
        } else {
            duration_sum as f64 / total as f64
        };
        DayStats {
            total,
            blocked,
            avg_duration_ms,
            median_duration_ms: percentile(&hist, total, 0.5),
            p95_duration_ms: percentile(&hist, total, 0.95),
        }
    }

    /// The busiest clients over the kept hours, most queries first, up to `n`.
    /// Counts are summed across the hourly buckets.
    pub fn top_clients(&self, n: usize) -> Vec<ClientCount> {
        let state = self.state.lock().expect("recorder lock poisoned");
        let mut totals: HashMap<IpAddr, u64> = HashMap::new();
        for bucket in &state.activity {
            for (&client, &count) in &bucket.clients {
                *totals.entry(client).or_insert(0) += count;
            }
        }
        let mut ranked: Vec<ClientCount> = totals
            .into_iter()
            .map(|(client, count)| ClientCount { client, count })
            .collect();
        // Most queries first; ties broken by address so the order is stable.
        ranked.sort_by(|a, b| b.count.cmp(&a.count).then(a.client.cmp(&b.client)));
        ranked.truncate(n);
        ranked
    }

    /// Every client seen over the kept hours, with its query count and when it
    /// was last seen. The count is summed across the hourly buckets (the same
    /// source as [`Self::top_clients`]); the last-seen time is the exact newest
    /// timestamp from the recent ring where the client still appears, falling
    /// back to the most recent hour bucket it is in. Used by the devices view to
    /// list and age clients.
    pub fn client_activity(&self) -> Vec<ClientActivity> {
        let state = self.state.lock().expect("recorder lock poisoned");

        // Query counts and a coarse last-seen (the hour start) from the buckets,
        // which span the whole kept day.
        let mut totals: HashMap<IpAddr, u64> = HashMap::new();
        let mut last_seen: HashMap<IpAddr, i64> = HashMap::new();
        for bucket in &state.activity {
            for (&client, &count) in &bucket.clients {
                *totals.entry(client).or_insert(0) += count;
                let seen = last_seen.entry(client).or_insert(bucket.hour_ms);
                *seen = (*seen).max(bucket.hour_ms);
            }
        }
        // Refine last-seen with the exact timestamps still in the recent ring.
        for event in &state.recent {
            let seen = last_seen.entry(event.client).or_insert(event.at_ms);
            *seen = (*seen).max(event.at_ms);
            // A client only in the ring (not yet in a bucket, e.g. right at an
            // hour edge) still gets counted so the list never misses it.
            totals.entry(event.client).or_insert(0);
        }

        let mut out: Vec<ClientActivity> = totals
            .into_iter()
            .map(|(client, count)| ClientActivity {
                client,
                count,
                last_seen_ms: last_seen.get(&client).copied().unwrap_or_default(),
            })
            .collect();
        // Most recently seen first; ties broken by address for a stable order.
        out.sort_by(|a, b| {
            b.last_seen_ms
                .cmp(&a.last_seen_ms)
                .then(a.client.cmp(&b.client))
        });
        out
    }
}

/// Add one query to the hour its timestamp falls in, keeping the buckets sorted
/// and trimmed to the most recent [`ACTIVITY_HOURS`]. Updates the hour's count,
/// its blocked tally, its summed handling time and its per-client count.
fn bump_activity(
    buckets: &mut VecDeque<HourBucket>,
    at_ms: i64,
    blocked: bool,
    duration_ms: u32,
    client: IpAddr,
) {
    let hour = at_ms - at_ms.rem_euclid(HOUR_MS);

    let bucket = match buckets.iter_mut().find(|bucket| bucket.hour_ms == hour) {
        Some(bucket) => bucket,
        None => {
            buckets.push_back(HourBucket::new(hour));
            buckets
                .make_contiguous()
                .sort_by_key(|bucket| bucket.hour_ms);
            while buckets.len() > ACTIVITY_HOURS {
                buckets.pop_front();
            }
            // Re-find after the sort/trim: the new bucket may have moved, or even
            // been trimmed away if it was older than every kept hour.
            match buckets.iter_mut().find(|bucket| bucket.hour_ms == hour) {
                Some(bucket) => bucket,
                None => return,
            }
        }
    };

    bucket.count += 1;
    if blocked {
        bucket.blocked += 1;
    }
    bucket.add_latency(u64::from(duration_ms));
    bucket.bump_client(client);
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr};

    fn event(at_ms: i64, source: QuerySource, response_code: u16) -> QueryEvent {
        QueryEvent {
            at_ms,
            client: IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10)),
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code,
            source,
            duration_ms: 3,
            answers: 1,
            addresses: Vec::new(),
        }
    }

    #[test]
    fn recent_returns_newest_first_and_evicts_when_full() {
        let recorder = Recorder::new(2);
        recorder.record(event(1, QuerySource::Upstream, 0));
        recorder.record(event(2, QuerySource::Upstream, 0));
        recorder.record(event(3, QuerySource::Upstream, 0));

        let recent = recorder.recent(10);
        let times: Vec<i64> = recent.iter().map(|event| event.at_ms).collect();
        assert_eq!(
            times,
            vec![3, 2],
            "newest first, oldest evicted past capacity"
        );
    }

    #[test]
    fn recent_honours_the_limit() {
        let recorder = Recorder::new(100);
        for at in 1..=5 {
            recorder.record(event(at, QuerySource::Upstream, 0));
        }
        assert_eq!(recorder.recent(3).len(), 3);
    }

    #[test]
    fn search_filters_by_name_or_client_newest_first() {
        let recorder = Recorder::new(100);
        let mut first = event(1, QuerySource::Upstream, 0);
        first.name = "news.example.".to_owned();
        first.client = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));
        let mut second = event(2, QuerySource::Cache, 0);
        second.name = "shop.other.".to_owned();
        second.client = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 20));
        let mut third = event(3, QuerySource::Upstream, 0);
        third.name = "news.again.".to_owned();
        third.client = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        recorder.record(first);
        recorder.record(second);
        recorder.record(third);

        // Name match, case-insensitive, newest first.
        let names: Vec<String> = recorder
            .search("NEWS", 10)
            .into_iter()
            .map(|event| event.name)
            .collect();
        assert_eq!(names, vec!["news.again.", "news.example."]);

        // Client-address match.
        let by_client = recorder.search("192.168.20", 10);
        assert_eq!(by_client.len(), 2);

        // The limit is honoured on matches.
        assert_eq!(recorder.search("192.168.20", 1).len(), 1);
    }

    #[test]
    fn stats_count_sources_and_response_codes() {
        let recorder = Recorder::new(100);
        recorder.record(event(1, QuerySource::Cache, 0));
        recorder.record(event(2, QuerySource::Upstream, 3));
        recorder.record(event(3, QuerySource::Upstream, 2));
        recorder.record(event(4, QuerySource::Local, 0));

        let stats = recorder.stats();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.upstream, 2);
        assert_eq!(stats.local, 1);
        assert_eq!(stats.nxdomain, 1);
        assert_eq!(stats.servfail, 1);
    }

    #[test]
    fn stats_survive_eviction() {
        let recorder = Recorder::new(1);
        recorder.record(event(1, QuerySource::Cache, 0));
        recorder.record(event(2, QuerySource::Upstream, 0));

        assert_eq!(
            recorder.recent(10).len(),
            1,
            "only the newest event is kept"
        );
        assert_eq!(
            recorder.stats().total,
            2,
            "counters outlive the ring buffer"
        );
    }

    #[test]
    fn activity_groups_by_hour_and_sorts() {
        let recorder = Recorder::new(100);
        // Two in hour 0, one in hour 1, recorded out of order.
        recorder.record(event(HOUR_MS + 10, QuerySource::Upstream, 0));
        recorder.record(event(5, QuerySource::Upstream, 0));
        recorder.record(event(HOUR_MS - 1, QuerySource::Upstream, 0));

        let activity = recorder.activity();
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
    fn activity_keeps_only_the_last_day() {
        let recorder = Recorder::new(10_000);
        for hour in 0..30 {
            recorder.record(event(hour * HOUR_MS, QuerySource::Upstream, 0));
        }
        let activity = recorder.activity();
        assert_eq!(activity.len(), ACTIVITY_HOURS);
        assert_eq!(activity.first().unwrap().hour_ms, 6 * HOUR_MS);
        assert_eq!(activity.last().unwrap().hour_ms, 29 * HOUR_MS);
    }

    /// A query event with a chosen client, source and handling time.
    fn event_with(
        at_ms: i64,
        source: QuerySource,
        client: Ipv4Addr,
        duration_ms: u32,
    ) -> QueryEvent {
        QueryEvent {
            at_ms,
            client: IpAddr::V4(client),
            name: "example.com.".to_owned(),
            record_type: 1,
            response_code: 0,
            source,
            duration_ms,
            answers: 1,
            addresses: Vec::new(),
        }
    }

    #[test]
    fn day_stats_mean_counts_every_query() {
        let recorder = Recorder::new(100);
        let a = Ipv4Addr::new(10, 0, 0, 1);
        recorder.record(event_with(5, QuerySource::Upstream, a, 10));
        recorder.record(event_with(6, QuerySource::Blocked, a, 2));
        recorder.record(event_with(HOUR_MS + 1, QuerySource::Cache, a, 0));

        let day = recorder.day_stats();
        assert_eq!(day.total, 3);
        assert_eq!(day.blocked, 1);
        // Every query counts, cache and blocked included: (10 + 2 + 0) / 3.
        assert!((day.avg_duration_ms - 4.0).abs() < 1e-9, "{day:?}");
    }

    #[test]
    fn day_stats_mean_stays_low_when_cache_heavy() {
        // The reported complaint: with lots of cache hits the figure should read
        // fast, not high. A flood of instant cache hits and a few slow upstream
        // lookups keeps the mean near zero, because the cache hits count too.
        let recorder = Recorder::new(2000);
        let a = Ipv4Addr::new(10, 0, 0, 1);
        for at in 0..1000 {
            recorder.record(event_with(at, QuerySource::Cache, a, 0));
        }
        for at in 0..10 {
            recorder.record(event_with(1000 + at, QuerySource::Upstream, a, 100));
        }

        let day = recorder.day_stats();
        assert_eq!(day.total, 1010);
        // 1000 hits at 0 ms + 10 at 100 ms → mean ≈ 1 ms, not 100.
        assert!(day.avg_duration_ms < 5.0, "mean stays low: {day:?}");
        // The median is the typical (cached) query, near zero.
        assert!(
            day.median_duration_ms <= 1.0,
            "median {}",
            day.median_duration_ms
        );
    }

    #[test]
    fn day_stats_median_is_robust_while_p95_shows_the_tail() {
        // 80 instant cache hits and 20 slow (200 ms) lookups. The median still
        // sits in the first bucket — one-in-five slow answers cannot drag it up —
        // while the p95 climbs into the slow tail, so the occasional slow lookup
        // is visible without skewing the headline mean.
        let recorder = Recorder::new(2000);
        let a = Ipv4Addr::new(10, 0, 0, 1);
        for at in 0..80 {
            recorder.record(event_with(at, QuerySource::Cache, a, 0));
        }
        for at in 0..20 {
            recorder.record(event_with(100 + at, QuerySource::Upstream, a, 200));
        }

        let day = recorder.day_stats();
        assert_eq!(day.total, 100);
        assert!(
            day.median_duration_ms <= 1.0,
            "median stays near zero: {}",
            day.median_duration_ms
        );
        assert!(
            day.p95_duration_ms >= 100.0,
            "p95 surfaces the slow tail: {}",
            day.p95_duration_ms
        );
    }

    #[test]
    fn day_stats_average_is_zero_without_queries() {
        let day = Recorder::new(10).day_stats();
        assert_eq!(day.avg_duration_ms, 0.0);
        assert_eq!(day.median_duration_ms, 0.0);
        assert_eq!(day.p95_duration_ms, 0.0);
    }

    #[test]
    fn latency_bin_places_samples_in_the_right_bucket() {
        assert_eq!(latency_bin(0), 0, "0 ms → first bucket (<=1)");
        assert_eq!(latency_bin(1), 0);
        assert_eq!(latency_bin(2), 1);
        assert_eq!(latency_bin(10), 4, "<=16 ms bucket");
        assert_eq!(latency_bin(4096), LATENCY_BOUNDS_MS.len() - 1);
        assert_eq!(
            latency_bin(4097),
            LATENCY_BOUNDS_MS.len(),
            "overflow bucket"
        );
        assert_eq!(latency_bin(99_999), LATENCY_BINS - 1);
    }

    #[test]
    fn top_clients_rank_by_count_across_hours_then_address() {
        let recorder = Recorder::new(100);
        let a = Ipv4Addr::new(10, 0, 0, 1);
        let b = Ipv4Addr::new(10, 0, 0, 2);
        let c = Ipv4Addr::new(10, 0, 0, 3);
        // a: 3 (spread across two hours), b: 2, c: 2 — c loses the tie by address.
        recorder.record(event_with(1, QuerySource::Upstream, a, 1));
        recorder.record(event_with(2, QuerySource::Upstream, a, 1));
        recorder.record(event_with(HOUR_MS + 1, QuerySource::Upstream, a, 1));
        recorder.record(event_with(3, QuerySource::Upstream, b, 1));
        recorder.record(event_with(4, QuerySource::Upstream, b, 1));
        recorder.record(event_with(5, QuerySource::Upstream, c, 1));
        recorder.record(event_with(6, QuerySource::Upstream, c, 1));

        let top = recorder.top_clients(2);
        assert_eq!(top.len(), 2);
        assert_eq!(
            top[0],
            ClientCount {
                client: IpAddr::V4(a),
                count: 3
            }
        );
        // b and c both have 2; b (…0.2) sorts before c (…0.3).
        assert_eq!(
            top[1],
            ClientCount {
                client: IpAddr::V4(b),
                count: 2
            }
        );
    }

    #[test]
    fn client_activity_reports_count_and_newest_last_seen() {
        let recorder = Recorder::new(100);
        let a = Ipv4Addr::new(10, 0, 0, 1);
        let b = Ipv4Addr::new(10, 0, 0, 2);
        // a: two queries, newest at t=50; b: one at t=30.
        recorder.record(event_with(10, QuerySource::Upstream, a, 1));
        recorder.record(event_with(50, QuerySource::Upstream, a, 1));
        recorder.record(event_with(30, QuerySource::Upstream, b, 1));

        let activity = recorder.client_activity();
        assert_eq!(activity.len(), 2);
        // Most recently seen first: a (last seen 50) before b (30).
        assert_eq!(activity[0].client, IpAddr::V4(a));
        assert_eq!(activity[0].count, 2);
        assert_eq!(activity[0].last_seen_ms, 50, "exact newest timestamp");
        assert_eq!(activity[1].client, IpAddr::V4(b));
        assert_eq!(activity[1].count, 1);
        assert_eq!(activity[1].last_seen_ms, 30);
    }
}
