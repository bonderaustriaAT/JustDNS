// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `justdns-harvester` — a read-only companion that mines a JustDNS instance's
//! query log for DACH ad/measurement domains the existing lists miss, and emits a
//! ranked candidate report for a human to review. It blocks nothing and opens no
//! PR; the report is its only product.
//!
//! See `README.md` for the deployment and the three-stage selection test.

mod api;
mod coverage;
mod model;
mod probe;
mod report;
mod select;
mod state;

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::api::ApiClient;
use crate::coverage::SuffixSet;
use crate::probe::{DohResolver, HttpSiteProbe, Resolver, SiteOutcome, SiteProbe};
use crate::report::Report;
use crate::select::{Candidate, Exclusion, Prelim, SelectConfig, finalize, prefilter};
use crate::state::State;

/// Runtime configuration, all from the environment so the container needs no
/// files. Secrets (the password) are read here and never logged.
struct Config {
    base_url: String,
    username: String,
    password: String,
    state_path: PathBuf,
    out_dir: PathBuf,
    interval: Duration,
    run_once: bool,
    first_run_window_days: i64,
    page_limit: usize,
    max_pages: usize,
    max_probes: usize,
    probe_timeout: Duration,
    doh_endpoint: String,
    coverage_urls: Vec<String>,
    /// Extra operator/protected domains to exclude on top of the built-in ones,
    /// from `HARVEST_OWN_DOMAINS` (comma or newline separated). These can never be
    /// proposed, so a new own service is kept out of the report by config alone.
    own_domains_extra: Vec<String>,
}

/// The coverage feeds downloaded each run so a candidate an existing list already
/// blocks is never re-proposed: the DACH list plus the representative Hagezi
/// feeds. Overridable via `HARVEST_COVERAGE_URLS`.
const DEFAULT_COVERAGE_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/bonderaustriaAT/justdns-dach-blocklist/main/dach-ads-trackers.txt",
    "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/multi-onlydomains.txt",
    "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/pro-onlydomains.txt",
    "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/tif.medium-onlydomains.txt",
];

fn env_var(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env_var(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let base_url =
            env_var("JUSTDNS_URL").ok_or("JUSTDNS_URL is required (e.g. https://dns.example)")?;
        let username = env_var("JUSTDNS_USER").ok_or("JUSTDNS_USER is required")?;
        let password = env_var("JUSTDNS_PASSWORD").ok_or("JUSTDNS_PASSWORD is required")?;

        let run_once = env_parse("HARVEST_ONCE", 0u8) != 0;
        let interval_secs = env_parse("HARVEST_INTERVAL_SECS", 3600u64);
        let coverage_urls = env_var("HARVEST_COVERAGE_URLS")
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| {
                DEFAULT_COVERAGE_URLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        Ok(Self {
            base_url,
            username,
            password,
            state_path: env_var("HARVEST_STATE")
                .unwrap_or_else(|| "/data/harvester/state.json".into())
                .into(),
            out_dir: env_var("HARVEST_OUT")
                .unwrap_or_else(|| "/data/harvester/reports".into())
                .into(),
            interval: Duration::from_secs(interval_secs.max(60)),
            run_once: run_once || interval_secs == 0,
            first_run_window_days: env_parse("HARVEST_WINDOW_DAYS", 7i64).max(1),
            page_limit: env_parse("HARVEST_PAGE_LIMIT", 1000usize).clamp(50, 5000),
            max_pages: env_parse("HARVEST_MAX_PAGES", 50usize).clamp(1, 1000),
            max_probes: env_parse("HARVEST_MAX_PROBES", 60usize).clamp(1, 1000),
            probe_timeout: Duration::from_secs(
                env_parse("HARVEST_PROBE_TIMEOUT_SECS", 6u64).max(1),
            ),
            doh_endpoint: env_var("HARVEST_DOH_ENDPOINT")
                .unwrap_or_else(|| "https://cloudflare-dns.com/dns-query".into()),
            coverage_urls,
            own_domains_extra: env_var("HARVEST_OWN_DOMAINS")
                .map(|raw| {
                    raw.split([',', '\n'])
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Download the coverage feeds into one suffix set. A feed that fails to download
/// is warned about and skipped — coverage is then merely less complete.
fn load_coverage(agent: &ureq::Agent, urls: &[String]) -> SuffixSet {
    let mut set = SuffixSet::new();
    for url in urls {
        match agent.get(url).call() {
            Ok(mut response) => match response.body_mut().read_to_string() {
                Ok(body) => set.add_list(&body),
                Err(error) => warn!(%url, %error, "coverage feed read failed; skipping"),
            },
            Err(error) => warn!(%url, %error, "coverage feed download failed; skipping"),
        }
    }
    if set.is_empty() {
        warn!(
            "coverage set is empty — every feed failed to download; candidates may be over-reported this run"
        );
    }
    info!(
        entries = set.len(),
        feeds = urls.len(),
        "coverage set built"
    );
    set
}

/// A cheap pre-score to order which prelims get the (bounded) network probes.
fn pre_score(p: &Prelim) -> f64 {
    let mut s = (p.distinct_clients as f64) * 2.0 + ((1 + p.hits) as f64).ln();
    if p.signals.name_hit {
        s += 2.0;
    }
    if p.signals.geo_dach {
        s += 2.0;
    }
    s
}

/// One harvest cycle: poll, aggregate, run the selection test with bounded
/// probing, write the report, persist state.
fn run_cycle(
    cfg: &Config,
    client: &mut ApiClient,
    agent: &ureq::Agent,
    resolver: &dyn Resolver,
    site_probe: &dyn SiteProbe,
    sel: &SelectConfig,
) -> Result<(), String> {
    let mut state = State::load(&cfg.state_path).map_err(|e| format!("load state: {e}"))?;

    // First run: only backfill the configured window, not the whole history.
    let now_ms = (now_unix() as i64) * 1000;
    if state.last_upstream_ms == 0 {
        state.last_upstream_ms = now_ms - cfg.first_run_window_days * model::MS_PER_DAY;
    }
    if state.last_blocked_ms == 0 {
        state.last_blocked_ms = now_ms - cfg.first_run_window_days * model::MS_PER_DAY;
    }

    // Poll the resolved (candidate) feed and the blocked (exclusion) feed.
    let resolved = client
        .poll_since(
            "upstream",
            Some(0),
            state.last_upstream_ms,
            cfg.page_limit,
            cfg.max_pages,
        )
        .map_err(|e| format!("poll upstream: {e}"))?;
    let blocked = client
        .poll_since(
            "blocked",
            None,
            state.last_blocked_ms,
            cfg.page_limit,
            cfg.max_pages,
        )
        .map_err(|e| format!("poll blocked: {e}"))?;
    info!(
        resolved = resolved.len(),
        blocked = blocked.len(),
        "polled new rows"
    );

    for row in &resolved {
        if row.source == "upstream" {
            state.observe_resolved(&row.name, &row.client, row.at_ms);
        }
    }
    for row in &blocked {
        state.observe_blocked(&row.name, row.at_ms);
    }

    // Load coverage and the instance's own blocked set.
    let coverage = load_coverage(agent, &cfg.coverage_urls);
    let blocked_set = state.blocked_suffix_set();

    // Stage 0 + 1 over every known root (pure).
    let mut prelims: Vec<Prelim> = Vec::new();
    let mut exclusions: Vec<Exclusion> = Vec::new();
    for (root, stat) in &state.domains {
        match prefilter(root, stat, &coverage, &blocked_set, sel) {
            Ok(prelim) => prelims.push(prelim),
            Err(excl) => exclusions.push(excl),
        }
    }
    prelims.sort_by(|a, b| {
        pre_score(b)
            .partial_cmp(&pre_score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Stage 2: bounded network probing over the highest-ranked prelims.
    let mut candidates: Vec<Candidate> = Vec::new();
    let probe_count = prelims.len().min(cfg.max_probes);
    if prelims.len() > cfg.max_probes {
        info!(
            total = prelims.len(),
            probing = cfg.max_probes,
            "more prelims than the probe cap; the rest wait for the next run"
        );
    }
    for prelim in prelims.into_iter().take(probe_count) {
        let resolve = resolver.resolve(&prelim.root);
        let apex_site = site_probe.probe(&prelim.root);
        // Only when the apex is a human site do we need per-sub-host verdicts, to
        // keep only the pure measurement sub-hosts. Bounded to the busiest few.
        let host_sites: Vec<(String, SiteOutcome)> = if apex_site == SiteOutcome::HumanSite {
            prelim
                .hosts
                .iter()
                .filter(|(h, _)| h != &prelim.root)
                .take(4)
                .map(|(h, _)| (h.clone(), site_probe.probe(h)))
                .collect()
        } else {
            Vec::new()
        };
        match finalize(&prelim, &coverage, resolve, apex_site, &host_sites, sel) {
            Ok(candidate) => candidates.push(candidate),
            Err(excl) => exclusions.push(excl),
        }
    }

    let report = Report::build(now_unix(), candidates, exclusions);
    match report.write(&cfg.out_dir) {
        Ok((md, _json)) => info!(
            candidates = report.candidate_count,
            report = %md.display(),
            "wrote candidate report"
        ),
        Err(error) => error!(%error, "failed to write report"),
    }

    state
        .save(&cfg.state_path)
        .map_err(|e| format!("save state: {e}"))?;
    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = match Config::from_env() {
        Ok(cfg) => cfg,
        Err(error) => {
            error!(%error, "invalid configuration");
            std::process::exit(2);
        }
    };
    info!(
        url = %cfg.base_url,
        once = cfg.run_once,
        out = %cfg.out_dir.display(),
        "justdns-harvester starting (read-only, report-only)"
    );

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut client = ApiClient::new(&cfg.base_url, &cfg.username, &cfg.password, agent.clone());
    let resolver = DohResolver::new(cfg.doh_endpoint.clone(), cfg.probe_timeout);
    let site_probe = HttpSiteProbe::new(cfg.probe_timeout);
    let mut sel = SelectConfig::default();
    for domain in &cfg.own_domains_extra {
        sel.own_domains.add_list(&format!("{domain}\n"));
    }
    if !cfg.own_domains_extra.is_empty() {
        info!(
            count = cfg.own_domains_extra.len(),
            "extra own domains excluded"
        );
    }

    loop {
        match run_cycle(&cfg, &mut client, &agent, &resolver, &site_probe, &sel) {
            Ok(()) => info!("harvest cycle complete"),
            Err(error) => error!(%error, "harvest cycle failed"),
        }
        if cfg.run_once {
            break;
        }
        std::thread::sleep(cfg.interval);
    }
}
