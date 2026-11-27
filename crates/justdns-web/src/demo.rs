// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Public demo mode (driven by `JUSTDNS_DEMO`): switch on a representative set of
//! curated lists and feed a steady stream of synthetic queries into the store, so
//! a read-only visitor lands on a live, populated dashboard — the query log,
//! the overview totals and the activity graph all move on their own. A no-op
//! unless started, and never wired on a normal node.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use justdns_lists::ListStore;
use justdns_store::{DnssecStatus, PhishingFlag, PhishingKind, QueryEvent, QuerySource, Store};
use tracing::warn;

/// Curated presets switched on for the demo, so the Lists page shows an active,
/// representative set. Idempotent: re-enabling an already-on preset is harmless.
const DEMO_PRESETS: &[&str] = &["hagezi-multi", "hagezi-tif", "justdns-dach", "nrd-7"];

/// The fake clients the synthetic traffic comes from. Stable across the run so
/// the Clients view and per-client shares look like a real small network.
const DEMO_CLIENTS: &[Ipv4Addr] = &[
    Ipv4Addr::new(10, 10, 0, 11),
    Ipv4Addr::new(10, 10, 0, 12),
    Ipv4Addr::new(10, 10, 0, 17),
    Ipv4Addr::new(10, 10, 0, 23),
    Ipv4Addr::new(10, 10, 0, 42),
    Ipv4Addr::new(10, 10, 0, 56),
];

/// Everyday names that resolve normally (served from cache or upstream).
const ALLOWED: &[&str] = &[
    "github.com.",
    "api.github.com.",
    "cloudflare.com.",
    "www.google.com.",
    "apple.com.",
    "wikipedia.org.",
    "orf.at.",
    "derstandard.at.",
    "spotify.com.",
    "signal.org.",
    "mozilla.org.",
    "debian.org.",
];

/// Ad and tracker names that a blocklist refuses before any lookup.
const BLOCKED: &[&str] = &[
    "ads.doubleclick.net.",
    "googleadservices.com.",
    "analytics.tiktok.com.",
    "graph.facebook.com.",
    "telemetry.microsoft.com.",
    "app-measurement.com.",
    "ad.doubleclick.net.",
    "tracking.example-adnet.com.",
];

/// Lookalike names paired with the brand each imitates, for phishing hits.
const LOOKALIKES: &[(&str, &str)] = &[
    ("paypa1-secure.com.", "PayPal"),
    ("micros0ft-login.com.", "Microsoft"),
    ("app!e-id.com.", "Apple"),
    ("amaz0n-support.com.", "Amazon"),
    ("secure-gooogle.com.", "Google"),
];

/// Freshly-registered-looking names, for newly-registered-domain hits.
const NRD: &[&str] = &[
    "win-a-prize-now.top.",
    "account-verify-2026.live.",
    "fast-refund-portal.xyz.",
    "delivery-tracking-dhl.click.",
];

/// A tiny xorshift64 PRNG. Enough to vary the synthetic stream; a demo does not
/// need a real random source, and avoiding one keeps this off the dependency list.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15)
            | 1;
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n` (n > 0).
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Pick one element of a non-empty slice.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// Start demo mode: seed the curated lists, then run the synthetic feed forever on
/// its own thread. Call only when `JUSTDNS_DEMO` is on.
pub fn start(store: Store, lists: ListStore) {
    seed_lists(&lists);
    thread::spawn(move || run(store));
}

/// Switch the demo preset set on. Each failure is logged and skipped so one bad
/// key never stops the rest.
fn seed_lists(lists: &ListStore) {
    for key in DEMO_PRESETS {
        if let Err(error) = lists.set_preset(key, true) {
            warn!(%error, preset = key, "demo: could not enable preset");
        }
    }
}

/// The feed loop: one synthetic query every so often, with the occasional short
/// burst, so the live views keep moving at a natural pace.
fn run(store: Store) {
    // A short warm-up so the stores and listeners are up before the first row.
    thread::sleep(Duration::from_secs(3));
    let mut rng = Rng::new();
    loop {
        let burst = 1 + rng.below(3);
        for _ in 0..burst {
            store.record(synthesize(&mut rng));
        }
        // 500–1500 ms between bursts.
        thread::sleep(Duration::from_millis(500 + rng.below(1000)));
    }
}

/// Build one plausible query event from the pools above.
fn synthesize(rng: &mut Rng) -> QueryEvent {
    let client = IpAddr::V4(*rng.pick(DEMO_CLIENTS));
    let record_type = if rng.below(4) == 0 { 28 } else { 1 }; // AAAA sometimes, else A.
    let duration_ms = 1 + rng.below(40) as u32;

    // Weighted category roll: mostly clean traffic, some blocks, a little phishing.
    let roll = rng.below(100);
    let (name, source, response_code, answers, addresses, resolver, dnssec, phishing) =
        if roll < 66 {
            // Allowed: cache or upstream, a couple of answer addresses.
            let name = (*rng.pick(ALLOWED)).to_owned();
            let from_cache = rng.below(2) == 0;
            let source = if from_cache {
                QuerySource::Cache
            } else {
                QuerySource::Upstream
            };
            let resolver = if from_cache {
                None
            } else {
                Some((*rng.pick(&["cloudflare-dot", "dns.google", "quad9-dot"])).to_owned())
            };
            let dnssec = if from_cache {
                None
            } else if rng.below(2) == 0 {
                Some(DnssecStatus::Secure)
            } else {
                Some(DnssecStatus::Insecure)
            };
            let addresses = sample_addresses(rng, record_type);
            let answers = addresses.len() as u16;
            (name, source, 0, answers, addresses, resolver, dnssec, None)
        } else if roll < 86 {
            // Blocked by a list: refused before any lookup.
            let name = (*rng.pick(BLOCKED)).to_owned();
            (name, QuerySource::Blocked, 0, 0, Vec::new(), None, None, None)
        } else if roll < 94 {
            // Phishing lookalike: warned (answer stands) or blocked.
            let (name, brand) = *rng.pick(LOOKALIKES);
            let blocked = rng.below(3) == 0;
            let flag = PhishingFlag {
                kind: PhishingKind::Lookalike,
                blocked,
                brand: Some(brand.to_owned()),
            };
            if blocked {
                (name.to_owned(), QuerySource::Blocked, 0, 0, Vec::new(), None, None, Some(flag))
            } else {
                let addresses = sample_addresses(rng, record_type);
                let answers = addresses.len() as u16;
                (
                    name.to_owned(),
                    QuerySource::Upstream,
                    0,
                    answers,
                    addresses,
                    Some("cloudflare-dot".to_owned()),
                    None,
                    Some(flag),
                )
            }
        } else if roll < 98 {
            // Newly-registered domain: warned.
            let name = (*rng.pick(NRD)).to_owned();
            let flag = PhishingFlag {
                kind: PhishingKind::Nrd,
                blocked: false,
                brand: None,
            };
            let addresses = sample_addresses(rng, record_type);
            let answers = addresses.len() as u16;
            (
                name,
                QuerySource::Upstream,
                0,
                answers,
                addresses,
                Some("dns.google".to_owned()),
                None,
                Some(flag),
            )
        } else {
            // A miss: NXDOMAIN for a name that does not exist.
            let name = format!("typo-{}.example.", rng.below(9999));
            (name, QuerySource::Upstream, 3, 0, Vec::new(), Some("dns.google".to_owned()), None, None)
        };

    QueryEvent {
        at_ms: now_ms(),
        client,
        name,
        record_type,
        response_code,
        source,
        duration_ms,
        answers,
        addresses,
        resolver,
        dnssec,
        phishing,
        block_reason: None,
        reason: None,
        trace: None,
    }
}

/// A small, type-appropriate set of answer addresses for a resolved name.
fn sample_addresses(rng: &mut Rng, record_type: u16) -> Vec<IpAddr> {
    if record_type == 28 {
        vec![IpAddr::V6(Ipv6Addr::new(
            0x2606,
            0x4700,
            0,
            0,
            0,
            0,
            (rng.below(0xffff) + 1) as u16,
            (rng.below(0xffff) + 1) as u16,
        ))]
    } else {
        let count = 1 + rng.below(2);
        (0..count)
            .map(|_| {
                IpAddr::V4(Ipv4Addr::new(
                    104,
                    (rng.below(60) + 16) as u8,
                    (rng.below(254) + 1) as u8,
                    (rng.below(254) + 1) as u8,
                ))
            })
            .collect()
    }
}

/// Milliseconds since the Unix epoch, clamped into range on an absurd clock.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
