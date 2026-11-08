// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Prometheus exposition endpoint (`GET /metrics`).
//!
//! JustDNS already keeps its operational figures in memory — the query store's
//! running totals ([`Stats`]), the day's latency summary ([`DayStats`]), the
//! per-resolver figures ([`ResolverStat`]) and the live [`RuntimeStatus`] — so a
//! scrape never touches DuckDB. This module turns one snapshot of those into the
//! Prometheus text format (version 0.0.4) by hand, which keeps the dependency
//! surface small: the format is just `# HELP` / `# TYPE` headers followed by
//! `name{labels} value` lines.
//!
//! The endpoint is gated by a bearer token (`JUSTDNS_METRICS_TOKEN`): with no
//! token configured it is off entirely, and with one it must be presented, so a
//! scraper is authorised without needing a login session. See [`token_matches`].

use std::fmt::Write as _;

use justdns_store::{DayStats, ResolverStat, Stats};

/// The `Content-Type` for the Prometheus text exposition format, version 0.0.4.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// The sizes of the loaded block/allow sets, by origin.
pub struct ListSizes {
    pub ads: u64,
    pub malware: u64,
    pub allow: u64,
    pub custom: u64,
}

/// Which DNS listeners are serving, as `(udp, tcp, dot, doh)`.
pub type Listeners = (bool, bool, bool, bool);

/// One point-in-time view of everything `/metrics` reports. Borrowed so the
/// handler can build it from the shared handles without cloning the store state.
pub struct Snapshot<'a> {
    /// The running release version (for `justdns_build_info`).
    pub version: &'a str,
    /// Seconds since the process started serving.
    pub uptime_secs: u64,
    /// The query store's running lifetime totals.
    pub stats: &'a Stats,
    /// The last day's latency summary (mean/median/p95).
    pub day: &'a DayStats,
    /// The per-resolver figures over the last day.
    pub resolvers: &'a [ResolverStat],
    /// Entries currently in the response cache.
    pub cache_entries: u64,
    /// Loaded block/allow list sizes.
    pub lists: ListSizes,
    /// Clients seen within the active window.
    pub active_clients: u64,
    /// Which listeners are up.
    pub listeners: Listeners,
    /// Unix time (seconds) of the last feed refresh, or `None` before the first.
    pub feed_last_refresh_unix: Option<i64>,
    /// Whether the last feed refresh succeeded, or `None` before the first.
    pub feed_ok: Option<bool>,
}

/// Escape a Prometheus label value: backslash, double-quote and newline, as the
/// exposition format requires.
fn escape_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

/// Format an `f64` metric value, guarding against a non-finite value (which the
/// exposition format does not allow here) by falling back to `0`.
fn num(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "0".to_string()
    }
}

/// Write a metric's `# HELP` and `# TYPE` header lines.
fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

/// Render the full exposition document for one [`Snapshot`].
pub fn render(snapshot: &Snapshot) -> String {
    let s = snapshot.stats;
    let mut out = String::with_capacity(2048);

    // Build info — a constant gauge carrying the version as a label, the usual
    // Prometheus idiom for exposing a build string.
    header(
        &mut out,
        "justdns_build_info",
        "gauge",
        "Build information; the value is always 1.",
    );
    let _ = writeln!(
        out,
        "justdns_build_info{{version=\"{}\"}} 1",
        escape_label(snapshot.version)
    );

    header(
        &mut out,
        "justdns_uptime_seconds",
        "gauge",
        "Seconds since the server started serving.",
    );
    let _ = writeln!(out, "justdns_uptime_seconds {}", snapshot.uptime_secs);

    // Query counters — monotonic lifetime totals.
    header(
        &mut out,
        "justdns_queries_total",
        "counter",
        "DNS queries handled in total.",
    );
    let _ = writeln!(out, "justdns_queries_total {}", s.total);

    header(
        &mut out,
        "justdns_cache_hits_total",
        "counter",
        "Queries served from the response cache.",
    );
    let _ = writeln!(out, "justdns_cache_hits_total {}", s.cache_hits);

    header(
        &mut out,
        "justdns_queries_upstream_total",
        "counter",
        "Queries forwarded to an upstream resolver.",
    );
    let _ = writeln!(out, "justdns_queries_upstream_total {}", s.upstream);

    header(
        &mut out,
        "justdns_queries_local_total",
        "counter",
        "Queries answered locally without a lookup (rewrites, suppressed records).",
    );
    let _ = writeln!(out, "justdns_queries_local_total {}", s.local);

    header(
        &mut out,
        "justdns_queries_blocked_total",
        "counter",
        "Queries refused by the blocking engine.",
    );
    let _ = writeln!(out, "justdns_queries_blocked_total {}", s.blocked);

    header(
        &mut out,
        "justdns_responses_nxdomain_total",
        "counter",
        "Answers returned as NXDOMAIN.",
    );
    let _ = writeln!(out, "justdns_responses_nxdomain_total {}", s.nxdomain);

    header(
        &mut out,
        "justdns_responses_servfail_total",
        "counter",
        "Answers returned as SERVFAIL.",
    );
    let _ = writeln!(out, "justdns_responses_servfail_total {}", s.servfail);

    // DNSSEC verdicts — one counter with a `verdict` label.
    header(
        &mut out,
        "justdns_dnssec_verdicts_total",
        "counter",
        "DNSSEC verdicts recorded, by outcome (only while validation is on).",
    );
    let _ = writeln!(
        out,
        "justdns_dnssec_verdicts_total{{verdict=\"secure\"}} {}",
        s.dnssec_secure
    );
    let _ = writeln!(
        out,
        "justdns_dnssec_verdicts_total{{verdict=\"insecure\"}} {}",
        s.dnssec_insecure
    );
    let _ = writeln!(
        out,
        "justdns_dnssec_verdicts_total{{verdict=\"bogus\"}} {}",
        s.dnssec_bogus
    );
    let _ = writeln!(
        out,
        "justdns_dnssec_verdicts_total{{verdict=\"indeterminate\"}} {}",
        s.dnssec_indeterminate
    );

    // Gauges.
    header(
        &mut out,
        "justdns_cache_entries",
        "gauge",
        "Entries currently held in the response cache.",
    );
    let _ = writeln!(out, "justdns_cache_entries {}", snapshot.cache_entries);

    header(
        &mut out,
        "justdns_active_clients",
        "gauge",
        "Clients that queried within the active window.",
    );
    let _ = writeln!(out, "justdns_active_clients {}", snapshot.active_clients);

    header(
        &mut out,
        "justdns_list_entries",
        "gauge",
        "Loaded block/allow list entries, by origin.",
    );
    let _ = writeln!(
        out,
        "justdns_list_entries{{list=\"ads\"}} {}",
        snapshot.lists.ads
    );
    let _ = writeln!(
        out,
        "justdns_list_entries{{list=\"malware\"}} {}",
        snapshot.lists.malware
    );
    let _ = writeln!(
        out,
        "justdns_list_entries{{list=\"allow\"}} {}",
        snapshot.lists.allow
    );
    let _ = writeln!(
        out,
        "justdns_list_entries{{list=\"custom\"}} {}",
        snapshot.lists.custom
    );

    header(
        &mut out,
        "justdns_listener_up",
        "gauge",
        "Whether a DNS listener is serving (1) or not (0).",
    );
    let (udp, tcp, dot, doh) = snapshot.listeners;
    let _ = writeln!(out, "justdns_listener_up{{listener=\"udp\"}} {}", udp as u8);
    let _ = writeln!(out, "justdns_listener_up{{listener=\"tcp\"}} {}", tcp as u8);
    let _ = writeln!(out, "justdns_listener_up{{listener=\"dot\"}} {}", dot as u8);
    let _ = writeln!(out, "justdns_listener_up{{listener=\"doh\"}} {}", doh as u8);

    // Feed refresh. Only emitted once a refresh has actually run.
    if let Some(ts) = snapshot.feed_last_refresh_unix {
        header(
            &mut out,
            "justdns_feed_last_refresh_timestamp_seconds",
            "gauge",
            "Unix time of the last block/allow list refresh.",
        );
        let _ = writeln!(out, "justdns_feed_last_refresh_timestamp_seconds {ts}");
    }
    if let Some(ok) = snapshot.feed_ok {
        header(
            &mut out,
            "justdns_feed_last_refresh_success",
            "gauge",
            "Whether the last list refresh succeeded (1) or failed (0).",
        );
        let _ = writeln!(out, "justdns_feed_last_refresh_success {}", ok as u8);
    }

    // Latency summary (whole-instance), as gauges — the store keeps the day's
    // mean/median/p95, not raw buckets, so this is a summary rather than a
    // Prometheus histogram.
    header(
        &mut out,
        "justdns_query_duration_avg_ms",
        "gauge",
        "Mean query handling time over the last day, in milliseconds.",
    );
    let _ = writeln!(
        out,
        "justdns_query_duration_avg_ms {}",
        num(snapshot.day.avg_duration_ms)
    );

    header(
        &mut out,
        "justdns_query_duration_ms",
        "gauge",
        "Query handling time over the last day by quantile, in milliseconds.",
    );
    let _ = writeln!(
        out,
        "justdns_query_duration_ms{{quantile=\"0.5\"}} {}",
        num(snapshot.day.median_duration_ms)
    );
    let _ = writeln!(
        out,
        "justdns_query_duration_ms{{quantile=\"0.95\"}} {}",
        num(snapshot.day.p95_duration_ms)
    );

    // Per-resolver figures. Cardinality is bounded by the configured upstreams.
    if !snapshot.resolvers.is_empty() {
        header(
            &mut out,
            "justdns_resolver_queries_total",
            "gauge",
            "Answers given by each upstream resolver over the last day.",
        );
        for r in snapshot.resolvers {
            let _ = writeln!(
                out,
                "justdns_resolver_queries_total{{resolver=\"{}\"}} {}",
                escape_label(&r.resolver),
                r.count
            );
        }
        header(
            &mut out,
            "justdns_resolver_duration_ms",
            "gauge",
            "Per-resolver handling time over the last day by quantile, in milliseconds.",
        );
        for r in snapshot.resolvers {
            let label = escape_label(&r.resolver);
            let _ = writeln!(
                out,
                "justdns_resolver_duration_ms{{resolver=\"{label}\",quantile=\"0.5\"}} {}",
                num(r.median_duration_ms)
            );
            let _ = writeln!(
                out,
                "justdns_resolver_duration_ms{{resolver=\"{label}\",quantile=\"0.95\"}} {}",
                num(r.p95_duration_ms)
            );
        }
    }

    out
}

/// Whether `provided` equals `expected` in constant time, so a wrong token
/// cannot be guessed byte-by-byte from the response timing. The length is
/// allowed to leak (an unequal length returns at once); a high-entropy token
/// makes that harmless.
pub fn token_matches(expected: &str, provided: &str) -> bool {
    let e = expected.as_bytes();
    let p = provided.as_bytes();
    if e.len() != p.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in e.iter().zip(p.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stats() -> Stats {
        Stats {
            total: 100,
            cache_hits: 60,
            upstream: 30,
            local: 5,
            blocked: 5,
            nxdomain: 3,
            servfail: 1,
            dnssec_secure: 20,
            dnssec_insecure: 70,
            dnssec_bogus: 2,
            dnssec_indeterminate: 0,
        }
    }

    fn sample_snapshot<'a>(
        stats: &'a Stats,
        day: &'a DayStats,
        resolvers: &'a [ResolverStat],
    ) -> Snapshot<'a> {
        Snapshot {
            version: "v9.9.9",
            uptime_secs: 42,
            stats,
            day,
            resolvers,
            cache_entries: 7,
            lists: ListSizes {
                ads: 41,
                malware: 2,
                allow: 1,
                custom: 0,
            },
            active_clients: 4,
            listeners: (true, true, false, false),
            feed_last_refresh_unix: Some(1_700_000_000),
            feed_ok: Some(true),
        }
    }

    #[test]
    fn render_has_headers_and_values() {
        let stats = sample_stats();
        let day = DayStats {
            total: 100,
            blocked: 5,
            avg_duration_ms: 1.5,
            median_duration_ms: 0.0,
            p95_duration_ms: 12.0,
        };
        let text = render(&sample_snapshot(&stats, &day, &[]));

        assert!(text.contains("# TYPE justdns_queries_total counter"));
        assert!(text.contains("\njustdns_queries_total 100\n"));
        assert!(text.contains("justdns_cache_hits_total 60"));
        assert!(text.contains("justdns_dnssec_verdicts_total{verdict=\"secure\"} 20"));
        assert!(text.contains("justdns_dnssec_verdicts_total{verdict=\"insecure\"} 70"));
        assert!(text.contains("justdns_listener_up{listener=\"udp\"} 1"));
        assert!(text.contains("justdns_listener_up{listener=\"dot\"} 0"));
        assert!(text.contains("justdns_build_info{version=\"v9.9.9\"} 1"));
        assert!(text.contains("justdns_query_duration_ms{quantile=\"0.95\"} 12"));
        assert!(text.contains("justdns_feed_last_refresh_success 1"));
    }

    #[test]
    fn render_escapes_resolver_labels_and_omits_empty_sections() {
        let stats = sample_stats();
        let day = DayStats::default();
        let resolvers = vec![ResolverStat {
            resolver: "weird\"name".to_string(),
            count: 9,
            avg_duration_ms: 2.0,
            median_duration_ms: 1.0,
            p95_duration_ms: 8.0,
        }];
        let text = render(&sample_snapshot(&stats, &day, &resolvers));
        assert!(text.contains("justdns_resolver_queries_total{resolver=\"weird\\\"name\"} 9"));

        // With no resolvers, the per-resolver sections are left out entirely.
        let empty = render(&sample_snapshot(&stats, &day, &[]));
        assert!(!empty.contains("justdns_resolver_queries_total"));
    }

    #[test]
    fn token_compare_is_exact() {
        assert!(token_matches("s3cr3t-token", "s3cr3t-token"));
        assert!(!token_matches("s3cr3t-token", "s3cr3t-toké"));
        assert!(!token_matches("s3cr3t-token", "wrong"));
        assert!(!token_matches("s3cr3t-token", ""));
        assert!(!token_matches("", "anything"));
    }
}
