// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The JustDNS binary: read the environment, build the resolver and serve.

mod config;
mod endpoints;
mod observer;
mod policy;

use std::env;
use std::net::IpAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use arc_swap::ArcSwapOption;
use justdns_access::{AccessRequest, AccessStore};
use justdns_audit::AuditStore;
use justdns_auth::{DEFAULT_SESSION_TTL, SessionStore, UserStore};
use justdns_clients::{ClientResolver, ClientStore};
use justdns_config::{ConfigStore, CustomTransport, CustomUpstream, Settings, SpeedProfile};
use justdns_dns::cache::{CacheTuning, DnsCache};
use justdns_dns::cert_store::CertStore;
use justdns_dns::observer::QueryObserver;
use justdns_dns::pipeline::{BlockPageTarget, Pipeline, PipelineOptions};
use justdns_dns::router::{Router, SharedRouter};
use justdns_dns::server::Server;
use justdns_dns::tls::TlsManager;
use justdns_dns::upstream::{Upstream, UpstreamConfig};
use justdns_filter::{Blocker, PolicyEngine};
use justdns_lists::ListStore;
use justdns_rewrite::{RewriteStore, Rewriter};
use justdns_schedule::ScheduleStore;
use justdns_secret::Sealer;
use justdns_store::Store;
use justdns_web::{LogBuffer, LogLevelControl, RingLayer, RuntimeStatus};
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer, Registry};

use crate::config::Config;
use crate::endpoints::{BlockPageSupervisor, EndpointSupervisor};
use crate::observer::StoreObserver;

/// How many recent queries the dashboard's hot view keeps in memory.
const RECENT_CAPACITY: usize = 1000;

/// Capacity of the in-memory diagnostic-log ring shown on the System page.
const LOG_RING_CAPACITY: usize = 5000;
/// Default directory for the rotating diagnostic log — relative like the other
/// data paths, set to `/data/logs` in the container.
const DEFAULT_SYSLOG_DIR: &str = "logs";
/// Default number of daily diagnostic-log files to keep.
const DEFAULT_SYSLOG_RETENTION: usize = 7;

/// How often the subscribed external feeds are re-downloaded in the background.
const FEED_REFRESH_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
/// How long after startup the first background refresh runs, so boot is not held
/// up by the network and the cached feeds serve in the meantime.
const FEED_REFRESH_DELAY: Duration = Duration::from_secs(30);
/// Feeds smaller than this are too small for a "shrank sharply" warning to mean
/// much, so the check only fires once a feed had at least this many entries.
const FEED_SHRINK_FLOOR: usize = 100;

/// Whether a feed shrank sharply enough to warn about: it previously held a
/// meaningful number of entries and has now lost more than half of them. A big,
/// sudden drop usually means a broken or truncated source rather than a genuine
/// edit, and silently losing block rules quietly weakens protection.
fn feed_shrank(previous: usize, entries: usize) -> bool {
    previous >= FEED_SHRINK_FLOOR && entries * 2 < previous
}

/// How often to ping each upstream to keep its connection warm between queries,
/// short enough to stay under typical DoT/DoH idle timeouts.
const UPSTREAM_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> ExitCode {
    // Diagnostic logging comes up before anything else — before the main
    // configuration is even parsed — so a configuration error is itself captured
    // in the log ring and the rotating file. Its few settings are read straight
    // from the environment here (default on, to ./logs, 7 daily files).
    let log_dir = env::var("JUSTDNS_SYSLOG_DIR").unwrap_or_else(|_| DEFAULT_SYSLOG_DIR.to_owned());
    let log_retention = env::var("JUSTDNS_SYSLOG_RETENTION")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SYSLOG_RETENTION);
    let log_to_file = env::var("JUSTDNS_SYSLOG")
        .map(|value| !syslog_disabled(&value))
        .unwrap_or(true);
    let (log_buffer, _log_guard, log_level) = init_tracing(log_to_file, &log_dir, log_retention);

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            error!(%error, "invalid configuration");
            return ExitCode::FAILURE;
        }
    };

    info!(
        listen = %config.listen,
        web = %config.web_listen,
        ipv6 = config.ipv6_enabled,
        db = %config.db_path.display(),
        config = %config.config_path.display(),
        upstreams = config.upstreams.upstreams.len(),
        "starting JustDNS"
    );

    // Live runtime status for the health and diagnostics endpoints. The version
    // is the release tag the image was built with (APP_VERSION), falling back to
    // the crate version for local builds.
    let version = env::var("APP_VERSION")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let status = RuntimeStatus::new(version);

    // The environment seeds the editable settings on first run; afterwards the
    // config file is the source of truth. IPv6 is published as a live flag the
    // pipeline reads per query.
    let seed = Settings {
        ipv6: config.ipv6_enabled,
        ..Settings::default()
    };
    // The at-rest key seals the secret files (accounts, config) on disk. It lives
    // in its own volume, generated on first boot; load_or_create mints it once and
    // reads it thereafter. A missing key against already-sealed files fails those
    // loads closed below, rather than silently starting empty.
    let sealer = match Sealer::load_or_create(&config.secret_key_path) {
        Ok(sealer) => sealer,
        Err(error) => {
            error!(%error, "cannot load or create the at-rest secret key");
            return ExitCode::FAILURE;
        }
    };

    let config_store = match ConfigStore::load(&config.config_path, seed, &sealer) {
        Ok(store) => store,
        Err(error) => {
            error!(%error, "cannot load the configuration");
            return ExitCode::FAILURE;
        }
    };

    // The account list lives next to the config. On first run it is empty, and
    // the web UI's setup screen creates the single administrator; logins are
    // tracked in memory, so a restart simply asks for the password again.
    let users = match UserStore::load(&config.auth_path, &sealer) {
        Ok(users) => users,
        Err(error) => {
            error!(%error, "cannot load the account store");
            return ExitCode::FAILURE;
        }
    };
    info!(configured = users.is_configured(), "loaded account store");
    let sessions = SessionStore::new(DEFAULT_SESSION_TTL);

    let (store, writer) = match Store::open(&config.db_path, RECENT_CAPACITY) {
        Ok(pair) => pair,
        Err(error) => {
            error!(%error, "cannot open the query log");
            return ExitCode::FAILURE;
        }
    };
    let observer: Arc<dyn QueryObserver> = Arc::new(StoreObserver::new(
        store.clone(),
        config_store.query_log_flag(),
    ));

    // The blocking engine loads the compiled-in starter lists once; each
    // category is gated by a live flag from the config store, so toggling it in
    // the dashboard takes effect on the next query without a restart.
    let blocker = Arc::new(Blocker::with_bundled_lists(
        config_store.block_ads_trackers_flag(),
        config_store.block_malware_flag(),
    ));
    info!(
        ads = blocker.ads_len(),
        malware = blocker.malware_len(),
        "loaded block lists"
    );

    // The user's own allow/block entries and feed subscriptions live on the data
    // volume. Loading them pushes the merged sets into the blocker at once, from
    // the on-disk feed caches, so no network is needed to start filtering.
    let lists = match ListStore::load(&config.lists_path, &config.lists_cache_dir, blocker.clone())
    {
        Ok(lists) => lists,
        Err(error) => {
            error!(%error, "cannot load the user lists");
            return ExitCode::FAILURE;
        }
    };
    info!(
        allow = blocker.allow_len(),
        custom = blocker.custom_len(),
        "loaded user lists"
    );

    // Refresh the subscribed feeds in the background: once shortly after boot,
    // then on a long interval. Each pass is blocking (it downloads), so it runs
    // on the blocking pool and never stalls the resolver or the web server.
    spawn_feed_refresh(lists.clone(), status.clone());

    // Local DNS rewrites (name → IP) live on the data volume too. Loading them
    // fills the shared rewriter the pipeline reads per query, so a mapping takes
    // effect at once and survives a restart.
    let rewriter = Arc::new(Rewriter::new());
    let rewrites = match RewriteStore::load(&config.rewrites_path, rewriter.clone()) {
        Ok(rewrites) => rewrites,
        Err(error) => {
            error!(%error, "cannot load the DNS rewrites");
            return ExitCode::FAILURE;
        }
    };
    info!(rewrites = rewriter.len(), "loaded DNS rewrites");

    // Named clients (devices) live on the data volume too. They are a read-time
    // label for the log and statistics, so loading them just makes the store
    // available to the web layer — nothing is pushed into the DNS path.
    let clients = match ClientStore::load(&config.clients_path) {
        Ok(clients) => clients,
        Err(error) => {
            error!(%error, "cannot load the named clients");
            return ExitCode::FAILURE;
        }
    };
    info!(clients = clients.view().len(), "loaded named clients");

    // Schedules (recurring time windows) live on the data volume too. The store
    // feeds the policy rebuild, the minute ticker and the web layer.
    let schedules = match ScheduleStore::load(&config.schedules_path) {
        Ok(schedules) => schedules,
        Err(error) => {
            error!(%error, "cannot load the schedules");
            return ExitCode::FAILURE;
        }
    };
    info!(schedules = schedules.view().len(), "loaded schedules");

    // The security audit log (logins, account and settings changes) lives on the
    // data volume so it survives restarts, and is bounded to the most recent
    // entries so it cannot grow without limit.
    let audit = match AuditStore::load(&config.audit_path) {
        Ok(audit) => audit,
        Err(error) => {
            error!(%error, "cannot load the audit log");
            return ExitCode::FAILURE;
        }
    };
    info!(audit = audit.len(), "loaded audit log");

    // The block page's pending "ask to unblock this" queue lives on the data
    // volume so it survives restarts. It feeds the dashboard queue and the web
    // layer; the block-page listeners write into it through the sink below.
    let access = match AccessStore::load(&config.access_path) {
        Ok(access) => access,
        Err(error) => {
            error!(%error, "cannot load the access-request queue");
            return ExitCode::FAILURE;
        }
    };
    info!(access = access.count(), "loaded access-request queue");

    // The client-scoped policy engine the DNS pipeline consults. The coordinator
    // folds the global lists, the groups, the devices and the schedules open right
    // now into one resolved policy per device plus a global default, and
    // republishes whenever any of them change (each store runs the rebuild through
    // its on-change hook; the ticker reruns it when a window opens or closes).
    let policy_engine = Arc::new(PolicyEngine::new());
    rebuild_policies(
        &policy_engine,
        &lists,
        &clients,
        &schedules,
        &config_store.get(),
    );
    {
        let engine = policy_engine.clone();
        let lists_handle = lists.clone();
        let clients_handle = clients.clone();
        let schedules_handle = schedules.clone();
        let config_handle = config_store.clone();
        lists.on_change(move || {
            rebuild_policies(
                &engine,
                &lists_handle,
                &clients_handle,
                &schedules_handle,
                &config_handle.get(),
            );
        });
    }
    {
        let engine = policy_engine.clone();
        let lists_handle = lists.clone();
        let clients_handle = clients.clone();
        let schedules_handle = schedules.clone();
        let config_handle = config_store.clone();
        clients.on_change(move || {
            rebuild_policies(
                &engine,
                &lists_handle,
                &clients_handle,
                &schedules_handle,
                &config_handle.get(),
            );
        });
    }
    {
        let engine = policy_engine.clone();
        let lists_handle = lists.clone();
        let clients_handle = clients.clone();
        let schedules_handle = schedules.clone();
        let config_handle = config_store.clone();
        schedules.on_change(move || {
            rebuild_policies(
                &engine,
                &lists_handle,
                &clients_handle,
                &schedules_handle,
                &config_handle.get(),
            );
        });
    }

    // The upstream pool and the cache tuning both follow the live settings: the
    // speed profile chooses which resolvers run and how eagerly answers are
    // cached, the encryption toggle picks DoT vs plaintext, and the mode decides
    // how the resolvers are combined. The router sits behind a swappable handle
    // so a dashboard change takes effect without a restart; the keepalive task
    // warms whichever router is current.
    let settings = config_store.get();
    // Reflect the configured encrypted-DNS endpoints in the status snapshot; the
    // on-change hook below keeps them current when the setting is toggled live.
    status.set_dot(settings.encrypted_dns.dot_enabled);
    status.set_doh(settings.encrypted_dns.doh_enabled);
    let overridden = config.upstreams_overridden;
    let pinned = config.upstreams.upstreams.clone();
    // DNSSEC validation switch, shared by every upstream client in the router.
    // Seeded from JUSTDNS_DNSSEC (off by default) and handed to each router the
    // config store rebuilds, so the setting survives an upstream change. Held as
    // one shared flag so a future dashboard toggle can flip validation live.
    let dnssec_validate = Arc::new(AtomicBool::new(config.dnssec_enabled));
    if config.dnssec_enabled {
        info!("DNSSEC validation enabled — signed answers are verified, Bogus is refused");
    }
    let shared_router = SharedRouter::new(Router::from_config(
        &upstreams_for(&settings, overridden, &pinned),
        dnssec_validate.clone(),
    ));
    spawn_upstream_keepalive(shared_router.clone());
    // The cache's tuning is published behind a shared handle so the same profile
    // change retunes it live. One cache, shared by the resolver pipeline and the
    // web layer's "clear cache" action (the handle is cheap to clone).
    let cache_tuning = justdns_dns::cache::shared_tuning(cache_tuning_for(settings.speed_profile));
    let cache = DnsCache::with_tuning(config.cache_capacity, cache_tuning.clone());
    // The block-page redirect target is shared between the pipeline (which reads
    // it per blocked query) and the block-page supervisor (which sets it from the
    // settings). Empty until the block page is turned on.
    let block_page_target = Arc::new(ArcSwapOption::<BlockPageTarget>::empty());
    let pipeline = Pipeline::new(
        cache.clone(),
        Arc::new(shared_router.clone()),
        PipelineOptions {
            ipv6_enabled: config_store.ipv6_flag(),
            safe_search: config_store.safe_search_flag(),
            block_null_ip: config_store.block_null_ip_flag(),
            block_page: block_page_target.clone(),
            dnssec_validate: dnssec_validate.clone(),
        },
    )
    .with_observer(observer)
    .with_policy_engine(policy_engine.clone())
    .with_rewriter(rewriter.clone());
    let server = Server::new(pipeline);

    // Encrypted client-DNS (DoT/DoH). The TLS material lives on the data volume
    // and is minted self-signed on first start; if it cannot be prepared we log
    // and carry on without the encrypted endpoints rather than failing to boot.
    // The supervisor brings the listeners up to match the settings and keeps
    // them in step on every later change (folded into the on_change below).
    // The shared certificate store backs DoT/DoH and the block page. It seeds a
    // default self-signed certificate on first start, importing the single-cert
    // era's material from `tls_dir` when present.
    let certs = match CertStore::load_or_init(
        &config.certs_dir,
        Some(config.tls_dir.as_path()),
        &settings.encrypted_dns.hostname,
        &[],
    ) {
        Ok(store) => Some(store),
        Err(error) => {
            error!(%error, dir = %config.certs_dir.display(), "cannot prepare the certificate store; DoT/DoH and the block page disabled");
            None
        }
    };
    let tls = certs.as_ref().and_then(|store| {
        match TlsManager::new(
            store.clone(),
            settings.encrypted_dns.dot_cert_id.clone(),
            settings.encrypted_dns.doh_cert_id.clone(),
        ) {
            Ok(tls) => Some(tls),
            Err(error) => {
                error!(%error, "cannot build the DoT/DoH TLS configs; encrypted endpoints disabled");
                None
            }
        }
    });
    let supervisor = tls.clone().map(|tls| {
        EndpointSupervisor::new(
            server.pipeline(),
            tls,
            config.dot_listen,
            config.doh_listen,
            &settings.encrypted_dns,
        )
    });
    // The block page lets a visitor ask for a blocked site to be released; each ask
    // is enriched and recorded into the access queue through this sink, which the
    // serving tasks hold.
    let access_sink: justdns_dns::blockpage::SharedSink = Some(Arc::new(AccessSink {
        engine: policy_engine.clone(),
        clients: clients.clone(),
        access: access.clone(),
    }));
    // The block-page web server. Like DoT/DoH it is optional and supervised; it
    // also owns the pipeline's redirect target so a blocked name points at it.
    let block_page = certs.clone().map(|store| {
        BlockPageSupervisor::new(
            store,
            block_page_target.clone(),
            access_sink.clone(),
            config.blockpage_http_listen,
            config.blockpage_https_listen,
            &settings.block_page,
        )
    });

    // Reconfigure the live engine on every settings change: rebuild the router
    // from the new profile/mode/encryption, retune the cache, and rebuild the
    // per-client policies (the ads/malware flags are part of the global default).
    // The config store runs this after each save, so the web layer needs no
    // extra wiring.
    {
        let router = shared_router.clone();
        let tuning = cache_tuning.clone();
        let engine = policy_engine.clone();
        let lists_handle = lists.clone();
        let clients_handle = clients.clone();
        let schedules_handle = schedules.clone();
        let supervisor = supervisor.clone();
        let block_page = block_page.clone();
        let dnssec_validate = dnssec_validate.clone();
        let status = status.clone();
        config_store.on_change(move |settings| {
            let upstreams = upstreams_for(settings, overridden, &pinned);
            router.store(Router::from_config(&upstreams, dnssec_validate.clone()));
            tuning.store(Arc::new(cache_tuning_for(settings.speed_profile)));
            rebuild_policies(
                &engine,
                &lists_handle,
                &clients_handle,
                &schedules_handle,
                settings,
            );
            // Start/stop the encrypted-DNS listeners and apply the selected
            // certificates to match the new settings.
            if let Some(supervisor) = &supervisor {
                supervisor.apply(&settings.encrypted_dns);
            }
            // Start/stop the block page, update its text and redirect target.
            if let Some(block_page) = &block_page {
                block_page.apply(&settings.block_page);
            }
            status.set_dot(settings.encrypted_dns.dot_enabled);
            status.set_doh(settings.encrypted_dns.doh_enabled);
            info!(
                profile = ?settings.speed_profile,
                dnssec = dnssec_validate.load(Ordering::Relaxed),
                dot = settings.encrypted_dns.dot_enabled,
                doh = settings.encrypted_dns.doh_enabled,
                "settings applied: upstream pool, cache tuning and policies rebuilt"
            );
        });
    }

    // Re-evaluate the schedules every minute and rebuild the policies when the set
    // of open windows changes, so a window opens and closes on its own.
    spawn_schedule_ticker(
        policy_engine.clone(),
        lists.clone(),
        clients.clone(),
        schedules.clone(),
        config_store.clone(),
    );

    // Plain UDP/TCP DNS and the web API run in the same process (the select!
    // below). By the time anyone can reach the diagnostics endpoint the plain
    // listeners are necessarily up, so mark them so here.
    status.set_plain_dns(true);

    // The optional bearer token that guards the Prometheus `/metrics` endpoint.
    // Unset (or blank) leaves the endpoint off entirely.
    let metrics_token: Option<Arc<str>> = env::var("JUSTDNS_METRICS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .map(Arc::from);

    // After the web layer mutates the certificate store (a renewal or a
    // regenerate), rebuild the live DoT/DoH and block-page TLS configs so the
    // changed certificate is served without a restart.
    let cert_reload: Option<Arc<dyn Fn() + Send + Sync>> = match (&tls, &block_page) {
        (None, None) => None,
        (tls, block_page) => {
            let tls = tls.clone();
            let block_page = block_page.clone();
            Some(Arc::new(move || {
                if let Some(tls) = &tls {
                    if let Err(error) = tls.reload() {
                        error!(%error, "could not reload the DoT/DoH certificates");
                    }
                }
                if let Some(block_page) = &block_page {
                    block_page.reload_cert();
                }
            }))
        }
    };

    tokio::select! {
        result = server.run(config.listen) => {
            if let Err(error) = result {
                error!(%error, "DNS server stopped");
                return ExitCode::FAILURE;
            }
        }
        result = justdns_web::serve(config.web_listen, store.clone(), config_store.clone(), blocker.clone(), lists.clone(), rewrites.clone(), clients.clone(), schedules.clone(), access.clone(), cache.clone(), users.clone(), sessions.clone(), audit.clone(), config.reports_path.clone(), certs.clone(), cert_reload.clone(), config.dot_listen.port(), config.doh_listen.port(), log_buffer.clone(), status.clone(), log_level.clone(), metrics_token.clone()) => {
            if let Err(error) = result {
                error!(%error, "web server stopped");
                return ExitCode::FAILURE;
            }
        }
        _ = shutdown() => {
            info!("shutting down");
        }
    }

    // Release our handle and let the query-log writer flush its final batch.
    drop(store);
    let _ = tokio::time::timeout(Duration::from_secs(5), writer).await;

    ExitCode::SUCCESS
}

/// Records a visitor's "ask to unblock this" from the block page into the access
/// queue. Shared with the block-page listeners through the [`AccessRequestSink`]
/// trait. It re-checks that the domain really is blocked for the asking client, so
/// the public endpoint cannot be used to plant arbitrary entries, and enriches the
/// ask with the device, group and the list/category that blocked it.
struct AccessSink {
    engine: Arc<PolicyEngine>,
    clients: ClientStore,
    access: AccessStore,
}

impl justdns_dns::blockpage::AccessRequestSink for AccessSink {
    fn request_access(&self, client: IpAddr, domain: &str, reason: Option<&str>) -> bool {
        // Only accept asks for names genuinely blocked for this client; an allow
        // already wins, so a released name explains to `None` and is refused.
        let Some(block) = self.engine.explain(client, domain) else {
            return false;
        };
        let resolver = ClientResolver::new(self.clients.snapshot());
        let (device, group) = match resolver.resolve(client) {
            Some(dev) => {
                let group_name = dev.group.as_deref().and_then(|id| {
                    self.clients
                        .groups()
                        .into_iter()
                        .find(|g| g.id == id)
                        .map(|g| g.name)
                });
                (Some(dev.name.clone()), group_name)
            }
            None => (None, None),
        };
        let created_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as i64)
            .unwrap_or(0);
        let request = AccessRequest {
            id: String::new(),
            domain: domain.to_owned(),
            client_ip: client.to_string(),
            device,
            group,
            block_category: Some(block.category.as_str().to_owned()),
            block_list: block.list.clone(),
            block_rule: Some(block.rule.clone()),
            reason: reason.map(str::to_owned),
            created_ms,
        };
        match self.access.record(request) {
            Ok(_) => true,
            Err(error) => {
                error!(%error, %domain, "could not record the access request");
                false
            }
        }
    }
}

/// Rebuild the per-client policies for the current stores, folding in the
/// schedules that are open right now (evaluated in the configured time zone).
fn rebuild_policies(
    engine: &PolicyEngine,
    lists: &ListStore,
    clients: &ClientStore,
    schedules: &ScheduleStore,
    settings: &Settings,
) {
    let snapshot = schedules.view();
    let tz = justdns_schedule::parse_tz(&settings.timezone);
    let active = justdns_schedule::active_now(&snapshot, tz);
    policy::rebuild(engine, lists, clients, settings, &active);
}

/// Start the background task that re-evaluates the schedules each minute. When the
/// set of open windows changes, it rebuilds the per-client policies so a window
/// takes effect (and lifts) on its own, without an edit to trigger a rebuild.
fn spawn_schedule_ticker(
    engine: Arc<PolicyEngine>,
    lists: ListStore,
    clients: ClientStore,
    schedules: ScheduleStore,
    config: ConfigStore,
) {
    tokio::spawn(async move {
        // Align to the next minute so windows open and close close to HH:MM.
        tokio::time::sleep(Duration::from_secs(seconds_to_next_minute())).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        // Seed with the windows already open (startup rebuilt with them), so the
        // first tick only rebuilds once something actually changes.
        let mut last = {
            let tz = justdns_schedule::parse_tz(&config.get().timezone);
            justdns_schedule::active_ids_now(&schedules.view(), tz)
        };
        loop {
            ticker.tick().await;
            let settings = config.get();
            let tz = justdns_schedule::parse_tz(&settings.timezone);
            let snapshot = schedules.view();
            let now_ids = justdns_schedule::active_ids_now(&snapshot, tz);
            if now_ids != last {
                last = now_ids;
                let active = justdns_schedule::active_now(&snapshot, tz);
                policy::rebuild(&engine, &lists, &clients, &settings, &active);
                info!(
                    open = active.len(),
                    "schedule windows changed; policies rebuilt"
                );
            }
        }
    });
}

/// Seconds until the next wall-clock minute boundary, to align the schedule ticker.
fn seconds_to_next_minute() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    60 - (secs % 60)
}

/// Start the background task that re-downloads the subscribed feeds: once after
/// a short delay, then on a long interval. The download is synchronous, so each
/// pass is handed to the blocking pool.
fn spawn_feed_refresh(lists: ListStore, status: Arc<RuntimeStatus>) {
    tokio::spawn(async move {
        tokio::time::sleep(FEED_REFRESH_DELAY).await;
        let mut ticker = tokio::time::interval(FEED_REFRESH_INTERVAL);
        loop {
            // The first tick fires immediately, so the initial refresh runs once
            // the startup delay above has elapsed; later ticks space out by the
            // interval.
            ticker.tick().await;
            let worker = lists.clone();
            match tokio::task::spawn_blocking(move || worker.refresh_all()).await {
                Ok(outcomes) if !outcomes.is_empty() => {
                    let failed = outcomes.iter().filter(|o| o.error.is_some()).count();
                    info!(feeds = outcomes.len(), failed, "refreshed subscribed lists");
                    for outcome in &outcomes {
                        match (&outcome.error, outcome.count) {
                            (Some(error), _) => {
                                warn!(id = %outcome.id, error = %error, "feed refresh failed");
                            }
                            (None, Some(entries)) => {
                                info!(
                                    id = %outcome.id,
                                    entries,
                                    previous = ?outcome.previous,
                                    took_ms = ?outcome.duration_ms,
                                    "feed refreshed"
                                );
                                if let Some(previous) = outcome.previous {
                                    if feed_shrank(previous, entries) {
                                        warn!(
                                            id = %outcome.id,
                                            entries,
                                            previous,
                                            "feed shrank sharply; the source may be broken"
                                        );
                                    }
                                }
                            }
                            (None, None) => {}
                        }
                    }
                    let error = (failed > 0)
                        .then(|| format!("{failed} of {} feeds failed", outcomes.len()));
                    status.record_feed_refresh(error);
                }
                // A pass with no subscribed feeds still counts as a successful run.
                Ok(_) => status.record_feed_refresh(None),
                Err(error) => {
                    warn!(%error, "feed refresh task panicked");
                    status.record_feed_refresh(Some(error.to_string()));
                }
            }
        }
    });
}

/// Build the `tracing` subscriber: the stdout layer (kept), the in-memory ring
/// that backs the System-Logs page, and — unless disabled — a daily-rotating
/// file on the data volume kept to `retention` files. Returns the shared log
/// ring and, when the file sink is active, the appender's flush guard, which the
/// caller must hold for the program's lifetime.
fn init_tracing(
    to_file: bool,
    dir: &str,
    retention: usize,
) -> (LogBuffer, Option<WorkerGuard>, Arc<LogLevelControl>) {
    // Honour an explicit RUST_LOG, else default to info. When one is set we label
    // the live verbosity "custom" so the UI shows it is operator-driven rather
    // than one of the Normal/Debug/Trace presets.
    let env_value = env::var("RUST_LOG").ok().filter(|value| !value.is_empty());
    let env_filter = env_value
        .as_deref()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    let buffer = LogBuffer::new(LOG_RING_CAPACITY);

    // The rotating file sink is optional: if the directory cannot be prepared we
    // carry on with stdout + ring rather than failing to start. Boxed so both
    // branches share one layer type.
    let (file_layer, guard): (
        Option<Box<dyn Layer<Registry> + Send + Sync>>,
        Option<WorkerGuard>,
    ) = if to_file {
        match tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("justdns")
            .filename_suffix("log")
            .max_log_files(retention)
            .build(dir)
        {
            Ok(appender) => {
                let (writer, guard) = tracing_appender::non_blocking(appender);
                let layer = tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer)
                    .boxed();
                (Some(layer), Some(guard))
            }
            Err(error) => {
                eprintln!("could not open the diagnostic log directory {dir:?}: {error}");
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // Wrap the global `EnvFilter` in a reload layer so an admin can raise the
    // verbosity at runtime from the System page (via `/api/logs/level`) without a
    // restart. The handle's `reload` rebuilds tracing's callsite-interest cache,
    // so raising the level actually starts emitting previously-disabled events.
    let (filter_layer, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);

    // The optional boxed file layer is added first, directly onto the registry,
    // so its subscriber type is `Registry` (matching the `Box<dyn Layer<Registry>>`
    // above). The global `EnvFilter` gates every layer regardless of position.
    tracing_subscriber::registry()
        .with(file_layer)
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .with(RingLayer::new(buffer.clone()))
        .init();

    // Hand the web layer a closure that swaps in a freshly parsed filter, so the
    // reload handle's concrete (and unnameable) type stays contained here.
    let initial = if env_value.is_some() {
        "custom"
    } else {
        "normal"
    };
    let log_level = LogLevelControl::new(
        initial,
        Box::new(move |spec| {
            let filter = EnvFilter::try_new(spec).map_err(|error| error.to_string())?;
            reload_handle
                .reload(filter)
                .map_err(|error| error.to_string())
        }),
    );

    (buffer, guard, log_level)
}

/// Whether a `JUSTDNS_SYSLOG` value switches the rotating file sink off. The
/// in-memory ring (and stdout) stay regardless.
fn syslog_disabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "off" | "false" | "no"
    )
}

/// Keep the upstream connections warm: prime them right away, then re-ping on an
/// interval short enough to stay under typical DoT/DoH idle timeouts, so a cache
/// miss rides an already-open session instead of paying a cold handshake. Each
/// tick warms whichever router is current, so a reconfigured pool is primed too.
fn spawn_upstream_keepalive(router: SharedRouter) {
    tokio::spawn(async move {
        router.current().warm().await;
        let mut ticker = tokio::time::interval(UPSTREAM_KEEPALIVE_INTERVAL);
        ticker.tick().await; // the immediate first tick; we just warmed above
        loop {
            ticker.tick().await;
            router.current().warm().await;
        }
    });
}

/// Map the dashboard's upstream-selection mode to the DNS core's own enum.
fn to_dns_mode(mode: justdns_config::UpstreamMode) -> justdns_dns::upstream::UpstreamMode {
    use justdns_config::UpstreamMode as Ui;
    use justdns_dns::upstream::UpstreamMode as Dns;
    match mode {
        Ui::Racing => Dns::Racing,
        Ui::Fallback => Dns::Fallback,
        Ui::Balanced => Dns::LoadBalanced,
        Ui::Rules => Dns::RuleBased,
    }
}

/// The resolvers a speed profile uses, over DoT. The encryption toggle may
/// switch these to plaintext and the mode decides how they are combined; here
/// we only choose the servers: EU-first privacy, a balanced mix, or the fastest.
fn profile_resolvers(profile: SpeedProfile) -> Vec<Upstream> {
    match profile {
        // Privacy first: DNS4EU Protective and Quad9, both no-log EU/CH resolvers.
        SpeedProfile::Privacy => vec![
            Upstream::dot(IpAddr::from([86, 54, 11, 1]), "protective.joindns4.eu"),
            Upstream::dot(IpAddr::from([9, 9, 9, 9]), "dns.quad9.net"),
        ],
        // A balanced mix: DNS4EU Protective paired with Cloudflare.
        SpeedProfile::Balanced => vec![
            Upstream::dot(IpAddr::from([86, 54, 11, 1]), "protective.joindns4.eu"),
            Upstream::dot(IpAddr::from([1, 1, 1, 1]), "cloudflare-dns.com"),
        ],
        // The fastest public resolvers: Cloudflare and Google.
        SpeedProfile::Speed => vec![
            Upstream::dot(IpAddr::from([1, 1, 1, 1]), "cloudflare-dns.com"),
            Upstream::dot(IpAddr::from([8, 8, 8, 8]), "dns.google"),
        ],
    }
}

/// The cache tuning a speed profile stands for.
fn cache_tuning_for(profile: SpeedProfile) -> CacheTuning {
    match profile {
        SpeedProfile::Privacy => CacheTuning::privacy(),
        SpeedProfile::Balanced => CacheTuning::balanced(),
        SpeedProfile::Speed => CacheTuning::speed(),
    }
}

/// Turn one hand-entered resolver into an [`Upstream`], or `None` when it does
/// not make sense: an address that will not parse, or DoT/DoH without a
/// certificate name (TLS cannot verify a nameless host). A half-finished entry
/// the user is still typing is simply skipped rather than breaking the pool.
fn map_custom(entry: &CustomUpstream) -> Option<Upstream> {
    let address: IpAddr = entry.address.trim().parse().ok()?;
    let with_port = |mut upstream: Upstream| {
        if let Some(port) = entry.port {
            upstream.port = port;
        }
        upstream
    };
    match entry.transport {
        CustomTransport::Plain => Some(with_port(Upstream::plain(address))),
        CustomTransport::Dot | CustomTransport::Doh => {
            let name = entry.server_name.as_deref().map(str::trim).unwrap_or("");
            if name.is_empty() {
                return None;
            }
            Some(with_port(match entry.transport {
                CustomTransport::Doh => Upstream::doh(address, name),
                _ => Upstream::dot(address, name),
            }))
        }
    }
}

/// The upstream pool to run for `settings`. The user's own resolvers win when
/// that switch is on and at least one entry is valid; otherwise the explicitly
/// pinned env resolvers (`JUSTDNS_UPSTREAMS`) or the profile's resolvers are
/// used, with the encryption toggle choosing DoT vs plaintext. The mode always
/// chooses how the resolvers are combined.
fn upstreams_for(settings: &Settings, overridden: bool, pinned: &[Upstream]) -> UpstreamConfig {
    let mode = to_dns_mode(settings.upstream_mode);

    if settings.custom_upstreams_enabled {
        let custom: Vec<Upstream> = settings
            .custom_upstreams
            .iter()
            .filter_map(map_custom)
            .collect();
        // Only replace the defaults once there is something usable, so an empty
        // or still-being-typed list never leaves the resolver with no upstream.
        if !custom.is_empty() {
            return UpstreamConfig {
                mode,
                upstreams: custom,
                rules: Vec::new(),
            };
        }
    }

    let base: Vec<Upstream> = if overridden {
        pinned.to_vec()
    } else {
        profile_resolvers(settings.speed_profile)
    };
    let upstreams = base
        .iter()
        .map(|upstream| upstream.with_encryption(settings.encrypt_upstream))
        .collect();
    UpstreamConfig {
        mode,
        upstreams,
        rules: Vec::new(),
    }
}

/// Resolve when the process is asked to stop: SIGTERM from `docker stop`, or
/// Ctrl-C from an interactive run.
async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut term = match signal(SignalKind::terminate()) {
            Ok(term) => term,
            Err(error) => {
                error!(%error, "cannot listen for SIGTERM");
                return;
            }
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_shrink_warns_only_on_a_sharp_drop_from_a_meaningful_size() {
        // A big feed losing more than half is a warning.
        assert!(feed_shrank(50_000, 10_000));
        assert!(feed_shrank(100, 49));
        // Losing exactly half or less is ordinary churn, no warning.
        assert!(!feed_shrank(100, 50));
        assert!(!feed_shrank(50_000, 49_000));
        // Tiny feeds never warn, however steep the relative drop.
        assert!(!feed_shrank(99, 1));
        assert!(!feed_shrank(10, 0));
        // Growth is never a shrink.
        assert!(!feed_shrank(1_000, 2_000));
    }
}
