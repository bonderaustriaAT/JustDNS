// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Supervises the encrypted client-DNS listeners (DoT and DoH).
//!
//! The listeners are optional and toggled from the dashboard, so they are not
//! part of the main `select!`: each runs as its own task that this supervisor
//! starts and stops to match the live settings. A hostname change re-mints the
//! self-signed certificate; a plain certificate swap needs no restart because
//! the listeners read the TLS config per connection.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use justdns_config::{CertSource as CfgCertSource, EncryptedDns};
use justdns_dns::doh::serve_doh;
use justdns_dns::pipeline::Pipeline;
use justdns_dns::router::Resolve;
use justdns_dns::server::serve_dot;
use justdns_dns::tls::CertSource;
use justdns_dns::tls::TlsManager;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Maps the config crate's certificate source onto the DNS core's own enum.
pub fn to_tls_source(source: CfgCertSource) -> CertSource {
    match source {
        CfgCertSource::SelfSigned => CertSource::SelfSigned,
        CfgCertSource::Uploaded => CertSource::Uploaded,
    }
}

/// The running listener tasks and the settings they were last reconciled to.
struct State {
    dot: Option<JoinHandle<()>>,
    doh: Option<JoinHandle<()>>,
    hostname: String,
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
                hostname: settings.hostname.clone(),
            }),
        });
        this.apply(settings);
        this
    }

    /// Reconcile the running listeners with `settings`: re-mint the self-signed
    /// certificate on a hostname change, then start or stop each listener to
    /// match its enable flag. Idempotent, so it is safe to call on every save.
    pub fn apply(&self, settings: &EncryptedDns) {
        let mut state = self.state.lock().expect("supervisor lock poisoned");

        // A hostname change invalidates a self-signed certificate (its SAN no
        // longer matches), so re-mint it. An uploaded certificate is left alone.
        let hostname = settings.hostname.trim();
        if settings.cert_source == CfgCertSource::SelfSigned
            && settings.hostname != state.hostname
            && !hostname.is_empty()
        {
            if let Err(error) = self.tls.regenerate(hostname, &[]) {
                error!(%error, "could not regenerate the self-signed certificate");
            } else {
                info!(hostname, "re-minted self-signed certificate");
            }
        }
        state.hostname = settings.hostname.clone();

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
