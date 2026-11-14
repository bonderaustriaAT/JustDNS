// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Offline GeoIP enrichment: map a resolved answer address to the country and
//! network (ASN) it belongs to, for the query log and the privacy dashboard.
//!
//! Lookups are **offline only** — a local MaxMind-format `.mmdb` database is read
//! from disk, never a network service — which is the whole point for a privacy
//! tool: we reveal where a device's traffic is heading without becoming another
//! party that watches it. DB-IP Lite and MaxMind GeoLite2 share the same schema,
//! so either (or a mix) works, and an operator can point at their own database.
//!
//! The enrichment lives entirely on the read path: the DNS engine and the query
//! log are untouched, and a missing database simply leaves the feature off.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Mutex;

use maxminddb::{Reader, geoip2};
use serde::Serialize;

/// The country and network a resolved address belongs to. Every field is
/// optional: a database may know the country but not the network, or the reverse,
/// and a private address carries neither.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct GeoInfo {
    /// ISO 3166-1 alpha-2 country code (for example `US`). The UI turns it into a
    /// flag and a localised name, so the database only has to provide the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// The autonomous-system number that announces the address (for example
    /// `13335`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<u32>,
    /// A human-readable name for that network's operator (for example
    /// `CLOUDFLARENET`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
}

impl GeoInfo {
    /// True when the database knew nothing about the address — nothing worth
    /// showing or caching as a hit.
    fn is_empty(&self) -> bool {
        self.country.is_none() && self.asn.is_none() && self.org.is_none()
    }
}

/// How many distinct addresses the lookup cache holds before it is cleared. The
/// same destinations (big CDNs, a handful of resolvers) recur constantly, so even
/// a coarse cap keeps almost every lookup off the database.
const CACHE_CAP: usize = 50_000;

/// A loaded pair of MaxMind-schema databases — a country database and an ASN
/// database — queried offline. Either may be absent. Cheap to share behind an
/// `Arc`; the in-memory lookup cache is internally synchronised.
pub struct GeoDb {
    country: Option<Reader<Vec<u8>>>,
    asn: Option<Reader<Vec<u8>>>,
    cache: Mutex<HashMap<IpAddr, GeoInfo>>,
}

impl GeoDb {
    /// Open whatever databases are present. Each path is tried on its own; a
    /// missing or unreadable file is logged and skipped. Returns `None` only when
    /// neither database could be opened, so the caller can leave the feature off
    /// without a special case.
    pub fn open(country_path: Option<&Path>, asn_path: Option<&Path>) -> Option<Self> {
        let country = country_path.and_then(|p| open_reader(p, "country"));
        let asn = asn_path.and_then(|p| open_reader(p, "asn"));
        if country.is_none() && asn.is_none() {
            return None;
        }
        Some(Self {
            country,
            asn,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Look up one address. Non-global addresses (private, loopback, link-local,
    /// and so on) never reach the databases and return `None` — the UI shows those
    /// as "local". A global address with no database entry also returns `None`.
    pub fn lookup(&self, ip: IpAddr) -> Option<GeoInfo> {
        if !is_global(ip) {
            return None;
        }
        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.get(&ip) {
                return non_empty(hit.clone());
            }
        }
        let info = self.query(ip);
        if let Ok(mut cache) = self.cache.lock() {
            // A blunt clear-at-cap keeps the map bounded without tracking usage;
            // the cost is only re-querying the database for still-hot addresses.
            if cache.len() >= CACHE_CAP {
                cache.clear();
            }
            cache.insert(ip, info.clone());
        }
        non_empty(info)
    }

    /// Read both databases for an address. Each decode is independent, so a hit in
    /// one and a miss in the other is fine.
    fn query(&self, ip: IpAddr) -> GeoInfo {
        let mut info = GeoInfo::default();
        if let Some(reader) = &self.country {
            if let Ok(result) = reader.lookup(ip) {
                if let Ok(Some(country)) = result.decode::<geoip2::Country>() {
                    info.country = country.country.iso_code.map(str::to_owned);
                }
            }
        }
        if let Some(reader) = &self.asn {
            if let Ok(result) = reader.lookup(ip) {
                if let Ok(Some(asn)) = result.decode::<geoip2::Asn>() {
                    info.asn = asn.autonomous_system_number;
                    info.org = asn.autonomous_system_organization.map(str::to_owned);
                }
            }
        }
        info
    }
}

/// Collapse an all-empty lookup to `None` so the caller never shows a blank row.
fn non_empty(info: GeoInfo) -> Option<GeoInfo> {
    if info.is_empty() { None } else { Some(info) }
}

/// Open one database, turning any error into `None` with a warning — a missing
/// database must never stop the dashboard from starting.
fn open_reader(path: &Path, which: &str) -> Option<Reader<Vec<u8>>> {
    match Reader::open_readfile(path) {
        Ok(reader) => Some(reader),
        Err(error) => {
            tracing::warn!(
                which,
                path = %path.display(),
                %error,
                "geoip database unavailable; skipping"
            );
            None
        }
    }
}

/// Whether an address is globally routable, so a geo lookup is meaningful.
/// Excludes the private ranges (RFC 1918 / carrier-grade NAT / IPv6 ULA),
/// loopback, link-local, the unspecified address and other reserved space. Kept
/// public so callers can label a non-global destination as "local" without a
/// database.
pub fn is_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || a == 0
                // Carrier-grade NAT, 100.64.0.0/10.
                || (a == 100 && (64..=127).contains(&b)))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                // Unique local addresses, fc00::/7.
                || (first & 0xfe00) == 0xfc00
                // Link-local unicast, fe80::/10.
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v6(s: &str) -> IpAddr {
        s.parse::<Ipv6Addr>().unwrap().into()
    }

    #[test]
    fn private_and_special_addresses_are_not_global() {
        assert!(!is_global(Ipv4Addr::new(192, 168, 1, 1).into()));
        assert!(!is_global(Ipv4Addr::new(10, 0, 0, 1).into()));
        assert!(!is_global(Ipv4Addr::new(172, 16, 5, 4).into()));
        assert!(!is_global(Ipv4Addr::LOCALHOST.into()));
        assert!(!is_global(Ipv4Addr::new(169, 254, 3, 2).into()));
        assert!(!is_global(Ipv4Addr::new(100, 64, 0, 1).into()));
        assert!(!is_global(Ipv4Addr::UNSPECIFIED.into()));
        assert!(!is_global(Ipv6Addr::LOCALHOST.into()));
        assert!(!is_global(v6("fd00::1")));
        assert!(!is_global(v6("fe80::1")));
    }

    #[test]
    fn public_addresses_are_global() {
        assert!(is_global(Ipv4Addr::new(1, 1, 1, 1).into()));
        assert!(is_global(Ipv4Addr::new(8, 8, 8, 8).into()));
        assert!(is_global(Ipv4Addr::new(100, 128, 0, 1).into()));
        assert!(is_global(v6("2606:4700:4700::1111")));
    }

    #[test]
    fn open_with_no_databases_returns_none() {
        assert!(GeoDb::open(None, None).is_none());
        assert!(
            GeoDb::open(
                Some(Path::new("/no/such/country.mmdb")),
                Some(Path::new("/no/such/asn.mmdb")),
            )
            .is_none()
        );
    }
}
