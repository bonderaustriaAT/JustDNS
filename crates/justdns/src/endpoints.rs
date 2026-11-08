// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Supervises the optional listeners: the encrypted client-DNS endpoints (DoT
//! and DoH) and the custom block page.
//!
//! These are toggled from the dashboard, so they are not part of the main
//! `select!`: each runs as its own task a supervisor starts and stops to match
//! the live settings. A certificate swap needs no restart — the listeners read
//! their TLS config per connection from a shared slot the supervisor updates.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use arc_swap::{ArcSwap, ArcSwapOption};
use justdns_config::{BlockPage, EncryptedDns};
use justdns_dns::blockpage::{
    BlockContent, Landing, SharedContent, SharedSink, SharedTls, serve_http, serve_https,
};
use justdns_dns::cert_store::{ALPN_HTTPS, CertStore};
use justdns_dns::doh::serve_doh;
use justdns_dns::pipeline::{BlockPageTarget, Pipeline};
use justdns_dns::router::Resolve;
use justdns_dns::server::serve_dot;
use justdns_dns::tls::TlsManager;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// The running DoT/DoH tasks.
struct State {
    dot: Option<JoinHandle<()>>,
    doh: Option<JoinHandle<()>>,
}

/// Starts, stops and re-certifies the DoT/DoH listeners to match the settings.
pub struct EndpointSupervisor<R> {
    pipeline: Arc<Pipeline<R>>,
    tls: Arc<TlsManager>,
    dot_addr: SocketAddr,
    doh_addr: SocketAddr,
    state: Mutex<State>,
}

impl<R> EndpointSupervisor<R>
where
    R: Resolve + Send + Sync + 'static,
{
    /// Build the supervisor and bring the listeners up to match `settings`.
    /// Must be called from within the Tokio runtime (it spawns tasks).
    pub fn new(
        pipeline: Arc<Pipeline<R>>,
        tls: Arc<TlsManager>,
        dot_addr: SocketAddr,
        doh_addr: SocketAddr,
        settings: &EncryptedDns,
    ) -> Arc<Self> {
        let this = Arc::new(Self {
            pipeline,
            tls,
            dot_addr,
            doh_addr,
            state: Mutex::new(State {
                dot: None,
                doh: None,
            }),
        });
        this.apply(settings);
        this
    }

    /// Reconcile the running listeners with `settings`: point DoT/DoH at their
    /// selected certificates, then start or stop each listener to match its
    /// enable flag. Idempotent, so it is safe to call on every save.
    pub fn apply(&self, settings: &EncryptedDns) {
        let mut state = self.state.lock().expect("supervisor lock poisoned");

        // Serve whatever certificate each endpoint now selects (a no-op when the
        // selection is unchanged). Done before (re)starting so a freshly enabled
        // listener comes up with the right certificate.
        if let Err(error) = self
            .tls
            .set_certs(settings.dot_cert_id.clone(), settings.doh_cert_id.clone())
        {
            error!(%error, "could not apply the selected DoT/DoH certificates");
        }

        self.reconcile_dot(&mut state, settings.dot_enabled);
        self.reconcile_doh(&mut state, settings.doh_enabled);
    }

    fn reconcile_dot(&self, state: &mut State, enabled: bool) {
        match (enabled, state.dot.is_some()) {
            (true, false) => {
                let addr = self.dot_addr;
                let pipeline = self.pipeline.clone();
                let tls = self.tls.clone();
                state.dot = Some(tokio::spawn(async move {
                    if let Err(error) = serve_dot(addr, pipeline, tls).await {
                        error!(%addr, %error, "DoT listener stopped");
                    }
                }));
                info!(%addr, "enabled DNS-over-TLS");
            }
            (false, true) => {
                if let Some(task) = state.dot.take() {
                    task.abort();
                }
                info!("disabled DNS-over-TLS");
            }
            _ => {}
        }
    }

    fn reconcile_doh(&self, state: &mut State, enabled: bool) {
        match (enabled, state.doh.is_some()) {
            (true, false) => {
                let addr = self.doh_addr;
                let pipeline = self.pipeline.clone();
                let tls = self.tls.clone();
                state.doh = Some(tokio::spawn(async move {
                    if let Err(error) = serve_doh(addr, pipeline, tls).await {
                        error!(%addr, %error, "DoH listener stopped");
                    }
                }));
                info!(%addr, "enabled DNS-over-HTTPS");
            }
            (false, true) => {
                if let Some(task) = state.doh.take() {
                    task.abort();
                }
                info!("disabled DNS-over-HTTPS");
            }
            _ => {}
        }
    }
}

/// The running block-page tasks and the certificate they were last built with.
struct BlockPageState {
    http: Option<JoinHandle<()>>,
    https: Option<JoinHandle<()>>,
    cert_id: Option<String>,
}

/// Starts, stops and re-certifies the custom block-page web server, and keeps
/// the DNS pipeline's block-page redirect target in step with the settings.
pub struct BlockPageSupervisor {
    store: Arc<CertStore>,
    /// The page text, shared live with the serving tasks.
    content: SharedContent,
    /// The HTTPS server config, shared live with the HTTPS serving task.
    https_tls: SharedTls,
    /// The DNS pipeline's redirect target — `Some` points blocked names at the
    /// block page, `None` leaves them at the null IP / NXDOMAIN.
    target: Arc<ArcSwapOption<BlockPageTarget>>,
    /// Where a visitor's "ask to unblock this" lands, if the feature is wired.
    sink: SharedSink,
    http_addr: SocketAddr,
    https_addr: SocketAddr,
    state: Mutex<BlockPageState>,
}

impl BlockPageSupervisor {
    /// Build the supervisor and bring the block page up to match `settings`.
    /// Must be called from within the Tokio runtime (it spawns tasks).
    pub fn new(
        store: Arc<CertStore>,
        target: Arc<ArcSwapOption<BlockPageTarget>>,
        sink: SharedSink,
        http_addr: SocketAddr,
        https_addr: SocketAddr,
        settings: &BlockPage,
    ) -> Arc<Self> {
        let content = Arc::new(ArcSwap::from_pointee(content_of(settings)));
        let https_tls = Arc::new(ArcSwap::from_pointee(
            store
                .build_config(settings.cert_id.as_deref(), ALPN_HTTPS)
                .expect("the certificate store always has a default certificate"),
        ));
        let this = Arc::new(Self {
            store,
            content,
            https_tls,
            target,
            sink,
            http_addr,
            https_addr,
            state: Mutex::new(BlockPageState {
                http: None,
                https: None,
                cert_id: settings.cert_id.clone(),
            }),
        });
        this.apply(settings);
        this
    }

    /// Reconcile the block page with `settings`: update the page text and the
    /// served certificate, set or clear the DNS redirect target, and start or
    /// stop the HTTP/HTTPS listeners. Idempotent.
    pub fn apply(&self, settings: &BlockPage) {
        let mut state = self.state.lock().expect("block-page lock poisoned");

        self.content.store(Arc::new(content_of(settings)));
        self.rebuild_cert(settings.cert_id.as_deref());
        state.cert_id = settings.cert_id.clone();

        // Point blocked names at the block page (or stop doing so).
        if settings.enabled {
            self.target.store(Some(Arc::new(BlockPageTarget {
                v4: parse_ipv4(&settings.server_ipv4),
                v6: parse_ipv6(&settings.server_ipv6),
            })));
        } else {
            self.target.store(None);
        }

        // The node always serves HTTPS on its own :443 while the block page is on,
        // even behind a reverse proxy. A blocked domain is pointed straight at this
        // node (its `server_ipv4`), not at the proxy, so an `https://` visit to a
        // blocked name lands on *this* :443 — if nothing listened there the browser
        // would get a bare connection refusal instead of the redirect. The reverse
        // proxy only fronts the landing host (where it terminates TLS with a valid
        // certificate); it never sees the direct blocked-domain hits. The
        // certificate presented here is the store default, so a blocked `https://`
        // name still warns (we hold no certificate for it) and then redirects — the
        // same, unavoidable behaviour as without a proxy.
        self.reconcile(&mut state, settings.enabled, settings.enabled);
    }

    /// Rebuild the HTTPS config from the current certificate selection, so a
    /// renewed or regenerated certificate is served live. Called by the binary's
    /// certificate-reload closure after a store mutation.
    pub fn reload_cert(&self) {
        let cert_id = self
            .state
            .lock()
            .expect("block-page lock poisoned")
            .cert_id
            .clone();
        self.rebuild_cert(cert_id.as_deref());
    }

    fn rebuild_cert(&self, cert_id: Option<&str>) {
        match self.store.build_config(cert_id, ALPN_HTTPS) {
            Ok(config) => self.https_tls.store(Arc::new(config)),
            Err(error) => error!(%error, "could not build the block-page HTTPS certificate"),
        }
    }

    fn reconcile(&self, state: &mut BlockPageState, want_http: bool, want_https: bool) {
        match (want_http, state.http.is_some()) {
            (true, false) => {
                let http_addr = self.http_addr;
                let content = self.content.clone();
                let sink = self.sink.clone();
                state.http = Some(tokio::spawn(async move {
                    if let Err(error) = serve_http(http_addr, content, sink).await {
                        error!(%http_addr, %error, "block-page HTTP listener stopped");
                    }
                }));
                info!(%http_addr, "enabled the block-page HTTP listener");
            }
            (false, true) => {
                if let Some(task) = state.http.take() {
                    task.abort();
                }
                info!("disabled the block-page HTTP listener");
            }
            _ => {}
        }
        match (want_https, state.https.is_some()) {
            (true, false) => {
                let https_addr = self.https_addr;
                let content_https = self.content.clone();
                let tls = self.https_tls.clone();
                let sink = self.sink.clone();
                state.https = Some(tokio::spawn(async move {
                    if let Err(error) = serve_https(https_addr, content_https, tls, sink).await {
                        error!(%https_addr, %error, "block-page HTTPS listener stopped");
                    }
                }));
                info!(%https_addr, "enabled the block-page HTTPS listener");
            }
            (false, true) => {
                if let Some(task) = state.https.take() {
                    task.abort();
                }
                info!("disabled the block-page HTTPS listener");
            }
            _ => {}
        }
    }
}

/// The page text from the settings.
fn content_of(settings: &BlockPage) -> BlockContent {
    BlockContent {
        title: settings.title.clone(),
        message: settings.message.clone(),
        landing: Landing::parse(&settings.redirect_url),
        trust_forwarded: settings.behind_reverse_proxy,
    }
}

/// Parse a configured IPv4 string, treating empty/invalid as "none".
fn parse_ipv4(value: &str) -> Option<std::net::Ipv4Addr> {
    match value.trim().parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => Some(v4),
        _ => None,
    }
}

/// Parse a configured IPv6 string, treating empty/invalid as "none".
fn parse_ipv6(value: &str) -> Option<std::net::Ipv6Addr> {
    match value.trim().parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => Some(v6),
        _ => None,
    }
}
