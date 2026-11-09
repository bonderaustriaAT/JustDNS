// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Live TLS material for the DoT and DoH listeners.
//!
//! Backed by the shared [`crate::cert_store::CertStore`]: DoT and DoH each
//! select a stored certificate by id, and this manager holds the built
//! [`rustls::ServerConfig`] for each behind an [`ArcSwap`], so a certificate
//! change or a new selection takes effect for new connections without rebinding
//! the listeners. The two configs differ only in their advertised ALPN.

use std::io;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use rustls::ServerConfig;

use crate::cert_store::{ALPN_DOH, ALPN_DOT, CertStore};

/// Live TLS material for the DoT and DoH listeners. Cheap to clone the handle
/// via [`Arc`].
pub struct TlsManager {
    store: Arc<CertStore>,
    dot_config: ArcSwap<ServerConfig>,
    doh_config: ArcSwap<ServerConfig>,
    /// The certificate ids DoT and DoH currently serve (`None` = the store's
    /// default), kept so [`reload`](Self::reload) can rebuild after a store
    /// change without being told the ids again.
    dot_id: Mutex<Option<String>>,
    doh_id: Mutex<Option<String>>,
}

impl TlsManager {
    /// Builds the DoT and DoH configs from the certificates `dot_id`/`doh_id`
    /// select (each falling back to the store default when `None` or unknown).
    pub fn new(
        store: Arc<CertStore>,
        dot_id: Option<String>,
        doh_id: Option<String>,
    ) -> io::Result<Arc<Self>> {
        let dot = store.build_config(dot_id.as_deref(), ALPN_DOT)?;
        let doh = store.build_config(doh_id.as_deref(), ALPN_DOH)?;
        Ok(Arc::new(Self {
            store,
            dot_config: ArcSwap::from_pointee(dot),
            doh_config: ArcSwap::from_pointee(doh),
            dot_id: Mutex::new(dot_id),
            doh_id: Mutex::new(doh_id),
        }))
    }

    /// The server config for the DoT listener (ALPN `dot`).
    pub fn dot_config(&self) -> Arc<ServerConfig> {
        self.dot_config.load_full()
    }

    /// The server config for the DoH listener (ALPN `h2`, `http/1.1`).
    pub fn doh_config(&self) -> Arc<ServerConfig> {
        self.doh_config.load_full()
    }

    /// The shared certificate store behind this manager.
    pub fn store(&self) -> &Arc<CertStore> {
        &self.store
    }

    /// Points DoT and DoH at (possibly new) certificate ids and rebuilds both
    /// configs for new connections.
    pub fn set_certs(&self, dot_id: Option<String>, doh_id: Option<String>) -> io::Result<()> {
        *self.dot_id.lock().unwrap() = dot_id;
        *self.doh_id.lock().unwrap() = doh_id;
        self.reload()
    }

    /// Rebuilds both configs from the current ids. Call after a store mutation
    /// (a renewal or a regenerate) so a changed certificate is served live.
    pub fn reload(&self) -> io::Result<()> {
        let dot_id = self.dot_id.lock().unwrap().clone();
        let doh_id = self.doh_id.lock().unwrap().clone();
        self.dot_config.store(Arc::new(
            self.store.build_config(dot_id.as_deref(), ALPN_DOT)?,
        ));
        self.doh_config.store(Arc::new(
            self.store.build_config(doh_id.as_deref(), ALPN_DOH)?,
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cert_store::DEFAULT_ID;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "justdns-tls-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        dir
    }

    #[test]
    fn builds_and_swaps_configs() {
        let dir = temp_dir();
        let store = CertStore::load_or_init(&dir, None, "dns.lan", &[]).expect("store");
        let tls = TlsManager::new(store.clone(), None, None).expect("tls");
        assert_eq!(tls.dot_config().alpn_protocols[0], b"dot");
        assert_eq!(tls.doh_config().alpn_protocols[0], b"h2");

        // Point DoT at a second certificate and rebuild.
        let id = store.generate("Zweit", "dns.lan", &[]).expect("generate");
        tls.set_certs(Some(id), None).expect("set");
        assert_eq!(tls.dot_config().alpn_protocols[0], b"dot");

        // A renewal of the default is picked up by reload.
        assert!(store.exists(DEFAULT_ID));
        tls.reload().expect("reload");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
