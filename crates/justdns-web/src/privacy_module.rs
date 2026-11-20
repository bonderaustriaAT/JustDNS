// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runtime for the opt-in Privacy module.
//!
//! The module bundles two features that only do work once an admin turns it on:
//!
//!  * **Chaff** — a steady trickle of decoy lookups sent to the configured
//!    upstreams so the real query stream is harder to profile from the outside.
//!    The decoys go out over the same [`SharedRouter`] as real traffic, so on
//!    the wire they are indistinguishable from a browser's own lookups. They
//!    bypass the resolver pipeline, so they never land in the DNS query log or
//!    the counters — instead they are recorded in this module's own in-memory
//!    log, which the UI shows under a separate tab. Mixing decoys into the real
//!    log would misrepresent what the devices on the network actually asked for.
//!
//! The runtime handle below holds only what the API has to read back: the chaff
//! log and a running total. The on/off switch and the intensity live in the
//! synced settings (`modules.privacy`), so the generator reads them straight
//! from the config on each tick and a cluster-wide enable needs no extra wiring.
//! When the module is off the generator parks on a cheap sleep, so a node that
//! never turns it on carries no added load.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use justdns_config::ConfigStore;
use justdns_dns::router::SharedRouter;

/// How many chaff entries the in-memory log keeps. Old entries fall off the
/// back; this is a live view, not durable history.
const CHAFF_LOG_CAP: usize = 500;

/// Decoy lookups are capped well below a real browser's rate: even at the top of
/// the slider the generator stays a background trickle, never a flood that would
/// stress the upstreams or stand out by volume.
const MAX_INTENSITY: u32 = 120;

/// One recorded decoy lookup, shown in the module's own log tab.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChaffEntry {
    /// Unix milliseconds the decoy was sent.
    pub ts_ms: u64,
    /// The queried name.
    pub name: String,
    /// The record type, e.g. `A` or `AAAA`.
    pub record_type: String,
    /// Whether the upstream answered (a failed decoy is still noise on the wire).
    pub answered: bool,
}

/// Shared, cheap-to-clone runtime outputs of the Privacy module. Held by the
/// API handlers (to read) and by the background generator (to write).
#[derive(Clone, Default)]
pub struct PrivacyRuntime {
    chaff_log: Arc<Mutex<VecDeque<ChaffEntry>>>,
    chaff_total: Arc<AtomicU64>,
}

impl PrivacyRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a decoy lookup, dropping the oldest once the log is full.
    fn record(&self, entry: ChaffEntry) {
        self.chaff_total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut log) = self.chaff_log.lock() {
            if log.len() >= CHAFF_LOG_CAP {
                log.pop_front();
            }
            log.push_back(entry);
        }
    }

    /// The most recent decoy lookups, newest first, up to `limit`.
    pub fn recent_chaff(&self, limit: usize) -> Vec<ChaffEntry> {
        match self.chaff_log.lock() {
            Ok(log) => log.iter().rev().take(limit).cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Total decoys sent since this node started (node-local, not persisted).
    pub fn chaff_total(&self) -> u64 {
        self.chaff_total.load(Ordering::Relaxed)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The decoy name pool. These are high-traffic, benign hostnames across a spread
/// of categories (cloud, video, shopping, news, social, telemetry) so a decoy
/// blends in with ordinary browsing instead of forming a recognisable pattern.
/// The pool is deliberately plain: the goal is cover traffic, not a fixed
/// signature an observer could learn to filter out.
const DECOY_POOL: &[&str] = &[
    "www.google.com",
    "www.youtube.com",
    "www.cloudflare.com",
    "www.amazon.com",
    "www.amazon.de",
    "www.wikipedia.org",
    "www.reddit.com",
    "www.github.com",
    "www.microsoft.com",
    "www.apple.com",
    "www.netflix.com",
    "www.spotify.com",
    "www.bing.com",
    "duckduckgo.com",
    "www.ebay.de",
    "www.zalando.de",
    "www.spiegel.de",
    "www.heise.de",
    "orf.at",
    "www.derstandard.at",
    "www.bbc.co.uk",
    "www.nytimes.com",
    "www.instagram.com",
    "www.linkedin.com",
    "www.twitch.tv",
    "www.paypal.com",
    "outlook.office365.com",
    "api.github.com",
    "fonts.googleapis.com",
    "cdn.jsdelivr.net",
    "registry.npmjs.org",
    "www.dropbox.com",
    "www.twitch.tv",
    "www.gmx.net",
    "www.willhaben.at",
    "www.booking.com",
    "maps.googleapis.com",
    "play.google.com",
    "www.wetter.com",
    "www.idealo.de",
];

/// Pick the next decoy name, cycling through the pool with a per-tick offset so
/// consecutive ticks do not repeat the same name. A tiny xorshift keeps it
/// dependency-free; the order does not need to be cryptographically random,
/// only spread out.
fn next_decoy(state: &mut u64) -> &'static str {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    let idx = (*state as usize) % DECOY_POOL.len();
    DECOY_POOL[idx]
}


/// Spawn the chaff generator. It runs for the life of the process on its own
/// Tokio task but stays parked while the module is off or the intensity is zero,
/// so it adds no measurable load until an admin opts in. When active it sends
/// one decoy every `60 / intensity` seconds (intensity = decoys per minute),
/// re-reading the live settings each tick so the slider and the on/off switch
/// take effect without a restart.
pub fn spawn_chaff_generator(runtime: PrivacyRuntime, config: ConfigStore, resolver: SharedRouter) {
    tokio::spawn(async move {
        // A short warm-up so the upstreams and listeners are up first.
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut rng: u64 = now_ms() | 1;
        let mut seq: u64 = 0;
        loop {
            let privacy = config.get().modules.privacy;
            if !privacy.enabled {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            let intensity = privacy.chaff_intensity.clamp(0, MAX_INTENSITY);
            if intensity == 0 {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            let host = next_decoy(&mut rng);
            let want_aaaa = seq % 2 == 1;
            seq = seq.wrapping_add(1);
            let answered = resolver.resolve_host(host, want_aaaa).await;
            runtime.record(ChaffEntry {
                ts_ms: now_ms(),
                name: host.to_string(),
                record_type: if want_aaaa { "AAAA" } else { "A" }.to_string(),
                answered,
            });

            // Spacing between decoys, with light jitter so the trickle is not a
            // metronome an observer could lock onto.
            let base_ms = (60_000 / intensity.max(1)) as u64;
            let jitter = rng % (base_ms / 4 + 1);
            tokio::time::sleep(Duration::from_millis(base_ms.saturating_sub(base_ms / 8) + jitter))
                .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaff_log_is_bounded_and_newest_first() {
        let rt = PrivacyRuntime::new();
        for i in 0..(CHAFF_LOG_CAP + 50) {
            rt.record(ChaffEntry {
                ts_ms: i as u64,
                name: format!("h{i}.example"),
                record_type: "A".into(),
                answered: true,
            });
        }
        let recent = rt.recent_chaff(10);
        assert_eq!(recent.len(), 10);
        // Newest first: the last pushed entry leads.
        assert_eq!(recent[0].ts_ms, (CHAFF_LOG_CAP + 49) as u64);
        assert_eq!(rt.chaff_total(), (CHAFF_LOG_CAP + 50) as u64);
        // Bounded: the full log never exceeds the cap.
        assert_eq!(rt.recent_chaff(10_000).len(), CHAFF_LOG_CAP);
    }

    #[test]
    fn decoys_spread_across_the_pool() {
        let mut rng: u64 = 123_456_789;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            seen.insert(next_decoy(&mut rng));
        }
        // The walk visits a broad spread of the pool, not one or two names.
        assert!(seen.len() > DECOY_POOL.len() / 2);
    }
}
