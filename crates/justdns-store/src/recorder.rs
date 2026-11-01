// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in-memory hot view the dashboard reads: the newest queries, running
//! counters, and per-hour activity. It is kept small and guarded by a single
//! short-lived lock so the DNS path can record into it without blocking on I/O.
//!
//! Durable history lives in the DuckDB log; this view only answers "what just
//! happened", so it holds a bounded ring of recent events and the last day of
//! hourly counts.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

use crate::event::{QueryEvent, QuerySource};

/// One hour of wall-clock time, in milliseconds.
const HOUR_MS: i64 = 3_600_000;
/// How many hours of activity to keep.
const ACTIVITY_HOURS: usize = 24;

/// Response code for a server failure.
const RCODE_SERVFAIL: u16 = 2;
/// Response code for a non-existent name.
const RCODE_NXDOMAIN: u16 = 3;

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

struct State {
    recent: VecDeque<QueryEvent>,
    stats: Stats,
    activity: VecDeque<ActivityBucket>,
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
    pub fn rehydrate(
        &self,
        recent_newest_first: Vec<QueryEvent>,
        stats: Stats,
        activity: Vec<ActivityBucket>,
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

        bump_activity(&mut state.activity, event.at_ms);

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

    /// Hourly activity, oldest hour first.
    pub fn activity(&self) -> Vec<ActivityBucket> {
        let state = self.state.lock().expect("recorder lock poisoned");
        state.activity.iter().copied().collect()
    }
}

/// Add one query to the hour its timestamp falls in, keeping the buckets sorted
/// and trimmed to the most recent [`ACTIVITY_HOURS`].
fn bump_activity(buckets: &mut VecDeque<ActivityBucket>, at_ms: i64) {
    let hour = at_ms - at_ms.rem_euclid(HOUR_MS);

    if let Some(bucket) = buckets.iter_mut().find(|bucket| bucket.hour_ms == hour) {
        bucket.count += 1;
        return;
    }

    buckets.push_back(ActivityBucket {
        hour_ms: hour,
        count: 1,
    });
    buckets
        .make_contiguous()
        .sort_by_key(|bucket| bucket.hour_ms);
    while buckets.len() > ACTIVITY_HOURS {
        buckets.pop_front();
    }
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
}
