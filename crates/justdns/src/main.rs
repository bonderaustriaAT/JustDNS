// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The JustDNS binary: read the environment, build the resolver and serve.

mod config;
mod observer;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use justdns_auth::{DEFAULT_SESSION_TTL, SessionStore, UserStore};
use justdns_config::{ConfigStore, Settings};
use justdns_dns::cache::DnsCache;
use justdns_dns::observer::QueryObserver;
use justdns_dns::pipeline::{Pipeline, PipelineOptions};
use justdns_dns::router::Router;
use justdns_dns::server::Server;
use justdns_filter::Blocker;
use justdns_lists::ListStore;
use justdns_rewrite::{RewriteStore, Rewriter};
use justdns_store::Store;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::observer::StoreObserver;

/// How many recent queries the dashboard's hot view keeps in memory.
const RECENT_CAPACITY: usize = 1000;

/// How often the subscribed external feeds are re-downloaded in the background.
const FEED_REFRESH_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
/// How long after startup the first background refresh runs, so boot is not held
/// up by the network and the cached feeds serve in the meantime.
const FEED_REFRESH_DELAY: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

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

    // The environment seeds the editable settings on first run; afterwards the
    // config file is the source of truth. IPv6 is published as a live flag the
    // pipeline reads per query.
    let seed = Settings {
        ipv6: config.ipv6_enabled,
        ..Settings::default()
    };
    let config_store = match ConfigStore::load(&config.config_path, seed) {
        Ok(store) => store,
        Err(error) => {
            error!(%error, "cannot load the configuration");
            return ExitCode::FAILURE;
        }
    };

    // The account list lives next to the config. On first run it is empty, and
    // the web UI's setup screen creates the single administrator; logins are
    // tracked in memory, so a restart simply asks for the password again.
    let users = match UserStore::load(&config.auth_path) {
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
    let observer: Arc<dyn QueryObserver> = Arc::new(StoreObserver::new(store.clone()));

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
    spawn_feed_refresh(lists.clone());

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

    let router = Router::from_config(&config.upstreams);
    // One cache, shared by the resolver pipeline and the web layer's "clear
    // cache" action (the handle is cheap to clone — the store is behind an Arc).
    let cache = DnsCache::new(config.cache_capacity);
    let pipeline = Pipeline::new(
        cache.clone(),
        router,
        PipelineOptions {
            ipv6_enabled: config_store.ipv6_flag(),
            safe_search: config_store.safe_search_flag(),
        },
    )
    .with_observer(observer)
    .with_blocker(blocker.clone())
    .with_rewriter(rewriter.clone());
    let server = Server::new(pipeline);

    tokio::select! {
        result = server.run(config.listen) => {
            if let Err(error) = result {
                error!(%error, "DNS server stopped");
                return ExitCode::FAILURE;
            }
        }
        result = justdns_web::serve(config.web_listen, store.clone(), config_store.clone(), blocker.clone(), lists.clone(), rewrites.clone(), cache.clone(), users.clone(), sessions.clone()) => {
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

/// Start the background task that re-downloads the subscribed feeds: once after
/// a short delay, then on a long interval. The download is synchronous, so each
/// pass is handed to the blocking pool.
fn spawn_feed_refresh(lists: ListStore) {
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
                    for outcome in outcomes.iter().filter(|o| o.error.is_some()) {
                        warn!(id = %outcome.id, error = ?outcome.error, "feed refresh failed");
                    }
                }
                Ok(_) => {}
                Err(error) => warn!(%error, "feed refresh task panicked"),
            }
        }
    });
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
