// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The selection test: what goes on the list and what does not.
//!
//! Three stages, in order. Hard gates decide *in or out*; the score only *ranks*
//! what survives; a human makes the final call from the report. The logic here is
//! pure — it takes aggregated stats plus already-fetched coverage and probe
//! results — so every rule is unit-testable without a network.
//!
//! - **Stage 0 — hard exclusions.** Already blocked on the instance, already
//!   covered by the DACH list or an active Hagezi feed, safelisted first-party
//!   infrastructure, too little signal to judge, or (after probing) a host that
//!   does not resolve live.
//! - **Stage 1 — relevance signals.** Does it look like DACH measurement/ads at
//!   all: a `.de`/`.at`/`.ch` root, a tracking-ish name, or a high device fan-out
//!   (a third-party beacon is reached by many devices, a first-party API by one).
//! - **Stage 2 — function test.** An HTTPS fetch of the root: a human website →
//!   never the apex, only a pure measurement sub-host; a bare beacon → the root
//!   itself is safe to block.

use crate::coverage::SuffixSet;
use crate::model::DomainStat;
use crate::probe::{ResolveOutcome, SiteOutcome};

/// Thresholds and weights for the selection test. Defaults are tuned for a home
/// network's volume; every field is overridable from the environment.
#[derive(Debug, Clone)]
pub struct SelectConfig {
    /// A root needs at least this many resolved lookups to be judged at all.
    pub min_hits: u64,
    /// …and at least this many distinct devices (keeps per-device noise out).
    pub min_clients: usize,
    /// Reaching at least this many distinct devices counts as "fan-out": the
    /// third-party shape that separates a beacon from a first-party API.
    pub fanout_clients: usize,
    /// Lowercase substrings that mark a tracking/measurement/ad name.
    pub name_terms: Vec<String>,
    /// Registrable roots never proposed: first-party and shared infrastructure
    /// whose blocking would break sites (CDNs, fonts, payment, the big clouds).
    pub safelist: SuffixSet,
    /// Score weights (see [`score`]).
    pub w_clients: f64,
    pub w_hits: f64,
    pub w_days: f64,
    pub w_name: f64,
    pub w_geo: f64,
    /// Subtracted when the apex is a human site (we can still propose sub-hosts,
    /// but it is a weaker, more careful candidate).
    pub penalty_human_apex: f64,
}

impl Default for SelectConfig {
    fn default() -> Self {
        Self {
            min_hits: 5,
            min_clients: 2,
            fanout_clients: 3,
            name_terms: default_name_terms(),
            safelist: default_safelist(),
            w_clients: 3.0,
            w_hits: 1.0,
            w_days: 1.5,
            w_name: 2.0,
            w_geo: 2.0,
            penalty_human_apex: 2.0,
        }
    }
}

/// The tracking/measurement/ad name fragments the relevance test looks for.
pub fn default_name_terms() -> Vec<String> {
    [
        "track",
        "metri",
        "stat",
        "count",
        "pixel",
        "ivw",
        "teli",
        "analyt",
        "adserv",
        "werbe",
        "reichweite",
        "beacon",
        "collect",
        "telemetr",
        "tracking",
        "tag",
        "ads",
        "oewa",
        "ioam",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// A conservative safelist of first-party / shared infrastructure that must never
/// be proposed, matched by suffix. Deliberately short; the user can extend it.
pub fn default_safelist() -> SuffixSet {
    let mut set = SuffixSet::new();
    set.add_list(
        "akamai.net\nakamaiedge.net\ncloudfront.net\nfastly.net\ncloudflare.net\ncloudflare.com\n\
         gstatic.com\ngoogleapis.com\ngoogleusercontent.com\nfbcdn.net\njsdelivr.net\nunpkg.com\n\
         cdnjs.cloudflare.com\nfonts.googleapis.com\nfonts.gstatic.com\nstripe.com\npaypal.com\n\
         amazonaws.com\nazureedge.net\nwindows.net\ngithub.io\ngithubusercontent.com\n",
    );
    set
}

/// Why a root was dropped — surfaced in the report so the exclusion is auditable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExcludeReason {
    AlreadyBlocked,
    AlreadyCovered,
    Safelisted,
    TooLittleSignal,
    NoTrackerSignal,
    DoesNotResolve,
    Parked,
    ApexIsSiteNoBeaconHost,
}

impl ExcludeReason {
    /// A short German label for the report.
    pub fn label(&self) -> &'static str {
        match self {
            ExcludeReason::AlreadyBlocked => "bereits auf dieser Instanz geblockt",
            ExcludeReason::AlreadyCovered => "schon durch Liste/Hagezi abgedeckt",
            ExcludeReason::Safelisted => "Safelist (Erstanbieter-/Shared-Infrastruktur)",
            ExcludeReason::TooLittleSignal => "zu wenig Signal (Hits/Geräte unter Schwelle)",
            ExcludeReason::NoTrackerSignal => "kein DACH-Tracker-Signal (Geo/Name/Fan-out)",
            ExcludeReason::DoesNotResolve => "löst nicht auf (tot/NXDOMAIN)",
            ExcludeReason::Parked => "nur genullt/geparkt",
            ExcludeReason::ApexIsSiteNoBeaconHost => {
                "Apex ist Firmenseite, kein reiner Mess-Subhost beobachtet"
            }
        }
    }
}

/// The relevance signals a root raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    pub name_hit: bool,
    pub geo_dach: bool,
    pub fanout: bool,
}

/// What the test recommends adding for a surviving candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recommendation {
    /// Block the registrable root (covers all its sub-hosts). Used when the apex
    /// is itself a beacon, not a human site.
    Root(String),
    /// Block these specific measurement sub-hosts — the apex serves a human site,
    /// so only the pure beacon hosts go on the list.
    Hosts(Vec<String>),
}

/// A root that passed stage 0 and 1 and now needs network probing.
#[derive(Debug, Clone)]
pub struct Prelim {
    pub root: String,
    pub hits: u64,
    pub distinct_clients: usize,
    pub days_seen: usize,
    pub hosts: Vec<(String, u64)>,
    pub signals: Signals,
}

/// A confirmed candidate, ready for the report and a human's final check.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub root: String,
    pub recommendation: Recommendation,
    pub hits: u64,
    pub distinct_clients: usize,
    pub days_seen: usize,
    pub signals: Signals,
    pub resolve: ResolveOutcome,
    pub apex_site: SiteOutcome,
    pub score: f64,
}

/// A root dropped by the test, with the reason.
#[derive(Debug, Clone)]
pub struct Exclusion {
    pub root: String,
    pub reason: ExcludeReason,
}

/// Does any of `terms` appear in `haystack` (already lowercased)?
fn name_matches(haystack: &str, terms: &[String]) -> bool {
    terms.iter().any(|t| haystack.contains(t.as_str()))
}

/// Is this root in a DACH TLD?
fn is_dach_tld(root: &str) -> bool {
    root.ends_with(".de") || root.ends_with(".at") || root.ends_with(".ch")
}

/// Stage 0 + stage 1, pure: turn one aggregated root into either a [`Prelim`]
/// (needs probing) or an [`Exclusion`]. `blocked` is the set of names already
/// blocked on the instance; `coverage` is the DACH list plus active Hagezi feeds.
pub fn prefilter(
    root: &str,
    stat: &DomainStat,
    coverage: &SuffixSet,
    blocked: &SuffixSet,
    cfg: &SelectConfig,
) -> Result<Prelim, Exclusion> {
    let drop = |reason| {
        Err(Exclusion {
            root: root.to_owned(),
            reason,
        })
    };

    // Stage 0 — hard exclusions that need no network.
    if blocked.covers(root) {
        return drop(ExcludeReason::AlreadyBlocked);
    }
    if coverage.covers(root) {
        return drop(ExcludeReason::AlreadyCovered);
    }
    if cfg.safelist.covers(root) {
        return drop(ExcludeReason::Safelisted);
    }
    if stat.hits < cfg.min_hits || stat.distinct_clients() < cfg.min_clients {
        return drop(ExcludeReason::TooLittleSignal);
    }

    // Stage 1 — relevance signals.
    let hosts = stat.hosts_by_hits();
    let name_hay = {
        let mut hay = root.to_owned();
        for (host, _) in &hosts {
            hay.push('\n');
            hay.push_str(host);
        }
        hay
    };
    let signals = Signals {
        name_hit: name_matches(&name_hay, &cfg.name_terms),
        geo_dach: is_dach_tld(root),
        fanout: stat.distinct_clients() >= cfg.fanout_clients,
    };
    if !signals.name_hit && !signals.geo_dach && !signals.fanout {
        return drop(ExcludeReason::NoTrackerSignal);
    }

    Ok(Prelim {
        root: root.to_owned(),
        hits: stat.hits,
        distinct_clients: stat.distinct_clients(),
        days_seen: stat.days_seen(),
        hosts,
        signals,
    })
}

/// Stage 2, pure: given a prelim and its already-fetched probe results, confirm
/// or drop it, and decide what exactly to recommend (the root, or specific
/// beacon sub-hosts when the apex is a human site). `host_sites` maps a probed
/// sub-host to its verdict; only hosts whose apex is a site need appear.
pub fn finalize(
    prelim: &Prelim,
    coverage: &SuffixSet,
    resolve: ResolveOutcome,
    apex_site: SiteOutcome,
    host_sites: &[(String, SiteOutcome)],
    cfg: &SelectConfig,
) -> Result<Candidate, Exclusion> {
    let drop = |reason| {
        Err(Exclusion {
            root: prelim.root.clone(),
            reason,
        })
    };

    match resolve {
        ResolveOutcome::Dead => return drop(ExcludeReason::DoesNotResolve),
        ResolveOutcome::Parked => return drop(ExcludeReason::Parked),
        ResolveOutcome::Live(_) => {}
    }

    let recommendation = if apex_site == SiteOutcome::HumanSite {
        // The apex is a company site: propose only pure measurement sub-hosts
        // that are not the apex, not already covered, and not themselves a site.
        let site_hosts: std::collections::HashSet<&str> = host_sites
            .iter()
            .filter(|(_, v)| *v == SiteOutcome::HumanSite)
            .map(|(h, _)| h.as_str())
            .collect();
        let hosts: Vec<String> = prelim
            .hosts
            .iter()
            .map(|(h, _)| h.clone())
            .filter(|h| h != &prelim.root)
            .filter(|h| !coverage.covers(h))
            .filter(|h| !site_hosts.contains(h.as_str()))
            .collect();
        if hosts.is_empty() {
            return drop(ExcludeReason::ApexIsSiteNoBeaconHost);
        }
        Recommendation::Hosts(hosts)
    } else {
        Recommendation::Root(prelim.root.clone())
    };

    let score = score(prelim, &apex_site, cfg);
    Ok(Candidate {
        root: prelim.root.clone(),
        recommendation,
        hits: prelim.hits,
        distinct_clients: prelim.distinct_clients,
        days_seen: prelim.days_seen,
        signals: prelim.signals,
        resolve,
        apex_site,
        score,
    })
}

/// The ranking score. Only orders the shortlist — it never decides in or out.
pub fn score(prelim: &Prelim, apex_site: &SiteOutcome, cfg: &SelectConfig) -> f64 {
    let mut s = cfg.w_clients * ((1 + prelim.distinct_clients) as f64).ln()
        + cfg.w_hits * ((1 + prelim.hits) as f64).ln()
        + cfg.w_days * prelim.days_seen as f64
        + if prelim.signals.name_hit {
            cfg.w_name
        } else {
            0.0
        }
        + if prelim.signals.geo_dach {
            cfg.w_geo
        } else {
            0.0
        };
    if *apex_site == SiteOutcome::HumanSite {
        s -= cfg.penalty_human_apex;
    }
    (s * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(hits: u64, clients: &[&str], days: &[i64], hosts: &[(&str, u64)]) -> DomainStat {
        let mut s = DomainStat::default();
        for (i, (host, count)) in hosts.iter().enumerate() {
            let day = days.get(i).copied().unwrap_or(days[0]);
            let client = clients
                .get(i % clients.len())
                .copied()
                .unwrap_or("10.0.0.1");
            for _ in 0..*count {
                s.observe(host, client, day * crate::model::MS_PER_DAY + 1);
            }
        }
        // force totals/clients to the intended shape
        s.hits = hits;
        s.clients = clients.iter().map(|c| c.to_string()).collect();
        s.days = days.iter().copied().collect();
        s
    }

    fn cfg() -> SelectConfig {
        SelectConfig::default()
    }

    #[test]
    fn drops_already_blocked_and_covered() {
        let mut blocked = SuffixSet::new();
        blocked.add_list("tracker.de\n");
        let mut coverage = SuffixSet::new();
        coverage.add_list("ioam.de\n");
        let s = stat(50, &["a", "b", "c"], &[1, 2], &[("x.tracker.de", 50)]);
        assert_eq!(
            prefilter("tracker.de", &s, &coverage, &blocked, &cfg())
                .unwrap_err()
                .reason,
            ExcludeReason::AlreadyBlocked
        );
        let s2 = stat(50, &["a", "b", "c"], &[1, 2], &[("script.ioam.de", 50)]);
        assert_eq!(
            prefilter("ioam.de", &s2, &coverage, &blocked, &cfg())
                .unwrap_err()
                .reason,
            ExcludeReason::AlreadyCovered
        );
    }

    #[test]
    fn drops_safelisted_and_low_signal() {
        let empty = SuffixSet::new();
        let s = stat(100, &["a", "b", "c"], &[1, 2], &[("x.cloudfront.net", 100)]);
        assert_eq!(
            prefilter("cloudfront.net", &s, &empty, &empty, &cfg())
                .unwrap_err()
                .reason,
            ExcludeReason::Safelisted
        );
        // one device, two hits → too little
        let mut thin = DomainStat::default();
        thin.observe("foo.de", "10.0.0.1", crate::model::MS_PER_DAY);
        thin.observe("foo.de", "10.0.0.1", crate::model::MS_PER_DAY);
        assert_eq!(
            prefilter("foo.de", &thin, &empty, &empty, &cfg())
                .unwrap_err()
                .reason,
            ExcludeReason::TooLittleSignal
        );
    }

    #[test]
    fn keeps_a_dach_tracker_and_drops_a_bland_domain() {
        let empty = SuffixSet::new();
        // DACH TLD + tracking name + fan-out → candidate.
        let s = stat(
            80,
            &["a", "b", "c", "d"],
            &[1, 2, 3],
            &[("count.example.at", 80)],
        );
        let prelim = prefilter("example.at", &s, &empty, &empty, &cfg()).unwrap();
        assert!(prelim.signals.geo_dach && prelim.signals.name_hit && prelim.signals.fanout);
        // .com, no tracking name, and below the fan-out threshold (2 devices) →
        // enough volume to judge, but no relevance signal at all.
        let mut bland = DomainStat::default();
        for i in 0..20 {
            let client = if i % 2 == 0 { "10.0.0.5" } else { "10.0.0.6" };
            bland.observe("api.shop.com", client, crate::model::MS_PER_DAY);
        }
        assert_eq!(bland.distinct_clients(), 2);
        assert_eq!(
            prefilter("shop.com", &bland, &empty, &empty, &cfg())
                .unwrap_err()
                .reason,
            ExcludeReason::NoTrackerSignal
        );
    }

    #[test]
    fn finalize_resolves_recommendation_by_function() {
        let empty = SuffixSet::new();
        let prelim = Prelim {
            root: "etracker.com".into(),
            hits: 120,
            distinct_clients: 6,
            days_seen: 4,
            hosts: vec![
                ("code.etracker.com".into(), 110),
                ("etracker.com".into(), 10),
            ],
            signals: Signals {
                name_hit: true,
                geo_dach: false,
                fanout: true,
            },
        };
        // Apex is a human site → recommend the beacon sub-host, not the apex.
        let c = finalize(
            &prelim,
            &empty,
            ResolveOutcome::Live(vec!["62.201.164.113".into()]),
            SiteOutcome::HumanSite,
            &[],
            &cfg(),
        )
        .unwrap();
        assert_eq!(
            c.recommendation,
            Recommendation::Hosts(vec!["code.etracker.com".into()])
        );

        // Apex is itself a beacon → recommend the root.
        let prelim2 = Prelim {
            root: "oewabox.at".into(),
            hits: 60,
            distinct_clients: 5,
            days_seen: 3,
            hosts: vec![("oewabox.at".into(), 60)],
            signals: Signals {
                name_hit: true,
                geo_dach: true,
                fanout: true,
            },
        };
        let c2 = finalize(
            &prelim2,
            &empty,
            ResolveOutcome::Live(vec!["13.248.243.5".into()]),
            SiteOutcome::Beacon,
            &[],
            &cfg(),
        )
        .unwrap();
        assert_eq!(c2.recommendation, Recommendation::Root("oewabox.at".into()));
    }

    #[test]
    fn finalize_drops_dead_and_parked() {
        let empty = SuffixSet::new();
        let prelim = Prelim {
            root: "dead.de".into(),
            hits: 60,
            distinct_clients: 5,
            days_seen: 3,
            hosts: vec![("dead.de".into(), 60)],
            signals: Signals {
                name_hit: false,
                geo_dach: true,
                fanout: true,
            },
        };
        assert_eq!(
            finalize(
                &prelim,
                &empty,
                ResolveOutcome::Dead,
                SiteOutcome::Unreachable,
                &[],
                &cfg()
            )
            .unwrap_err()
            .reason,
            ExcludeReason::DoesNotResolve
        );
        assert_eq!(
            finalize(
                &prelim,
                &empty,
                ResolveOutcome::Parked,
                SiteOutcome::Unreachable,
                &[],
                &cfg()
            )
            .unwrap_err()
            .reason,
            ExcludeReason::Parked
        );
    }
}
