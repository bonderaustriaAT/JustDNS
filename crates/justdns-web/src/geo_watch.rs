// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared "watched geo" resolution. A country or network is *watched* when it is
//! on the global list ([`justdns_config::GeoWatch`]) or on the watch delta of a
//! client's group or device. Answers that resolve into a watched network are
//! highlighted in the log and, when the alert is on, raise a webhook — but this is
//! purely a read-path marker: it is never folded into the blocking policy, so the
//! DNS hot path is untouched.
//!
//! Both the query views (per-row flag) and the alert evaluator resolve the same
//! effective set, so this lives in one place. [`WatchResolver`] caches the set per
//! client so a page of many rows from a few devices resolves each device once.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use justdns_clients::{ClientResolver, Device, Group};
use justdns_config::GeoWatch;
use justdns_geoip::GeoInfo;

/// The resolved set of watched countries (ISO 3166-1 alpha-2, lower-case) and
/// networks (ASN) for one client.
#[derive(Default)]
pub struct WatchSet {
    pub countries: HashSet<String>,
    pub asns: HashSet<u32>,
}

impl WatchSet {
    pub fn is_empty(&self) -> bool {
        self.countries.is_empty() && self.asns.is_empty()
    }

    /// Whether a resolved network is watched by this set.
    pub fn matches(&self, info: &GeoInfo) -> bool {
        info.country
            .as_deref()
            .is_some_and(|c| self.countries.contains(&c.to_ascii_lowercase()))
            || info.asn.is_some_and(|asn| self.asns.contains(&asn))
    }
}

fn extend(set: &mut WatchSet, countries: &[String], asns: &[u32]) {
    for country in countries {
        set.countries.insert(country.to_ascii_lowercase());
    }
    for asn in asns {
        set.asns.insert(*asn);
    }
}

/// The effective watch set for a client: the global list unioned with the client's
/// device delta and that device's group delta.
pub fn effective_watch(
    global: &GeoWatch,
    device: Option<&Device>,
    groups: &HashMap<String, Group>,
) -> WatchSet {
    let mut set = WatchSet {
        countries: global
            .countries
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect(),
        asns: global.asns.iter().copied().collect(),
    };
    if let Some(dev) = device {
        extend(
            &mut set,
            &dev.policy.watch_countries,
            &dev.policy.watch_asns,
        );
        if let Some(group) = dev.group.as_ref().and_then(|id| groups.get(id)) {
            extend(
                &mut set,
                &group.policy.watch_countries,
                &group.policy.watch_asns,
            );
        }
    }
    set
}

/// Whether any scope watches anything — a cheap gate before a scan.
pub fn anything_watched(global: &GeoWatch, devices: &[Device], groups: &[Group]) -> bool {
    let has = |countries: &[String], asns: &[u32]| !countries.is_empty() || !asns.is_empty();
    has(&global.countries, &global.asns)
        || devices
            .iter()
            .any(|d| has(&d.policy.watch_countries, &d.policy.watch_asns))
        || groups
            .iter()
            .any(|g| has(&g.policy.watch_countries, &g.policy.watch_asns))
}

/// Resolves, and caches per client, whether a row's networks are watched. Built
/// once per request from the device catalogue and global list; `active` is false
/// when no GeoIP database is loaded or nothing is watched, so `watched` is then a
/// constant `false` with no work.
pub struct WatchResolver<'a> {
    global: &'a GeoWatch,
    resolver: &'a ClientResolver,
    groups: &'a HashMap<String, Group>,
    active: bool,
    cache: HashMap<IpAddr, WatchSet>,
}

impl<'a> WatchResolver<'a> {
    pub fn new(
        global: &'a GeoWatch,
        resolver: &'a ClientResolver,
        groups: &'a HashMap<String, Group>,
        active: bool,
    ) -> Self {
        Self {
            global,
            resolver,
            groups,
            active,
            cache: HashMap::new(),
        }
    }

    /// Whether any of this client's resolved networks is watched.
    pub fn watched(&mut self, client: IpAddr, networks: &[GeoInfo]) -> bool {
        if !self.active || networks.is_empty() {
            return false;
        }
        let set = self.cache.entry(client).or_insert_with(|| {
            effective_watch(self.global, self.resolver.resolve(client), self.groups)
        });
        networks.iter().any(|info| set.matches(info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use justdns_clients::PolicyDelta;

    fn info(country: Option<&str>, asn: Option<u32>) -> GeoInfo {
        GeoInfo {
            country: country.map(str::to_owned),
            asn,
            org: None,
        }
    }

    fn device_with_watch(
        target: &str,
        group: Option<&str>,
        countries: &[&str],
        asns: &[u32],
    ) -> Device {
        Device {
            target: target.to_owned(),
            name: "d".to_owned(),
            note: None,
            kind: Default::default(),
            group: group.map(str::to_owned),
            policy: PolicyDelta {
                watch_countries: countries.iter().map(|c| c.to_string()).collect(),
                watch_asns: asns.to_vec(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn matches_country_case_insensitively_and_asn() {
        let global = GeoWatch {
            countries: vec!["RU".to_owned()],
            asns: vec![13335],
        };
        let set = effective_watch(&global, None, &HashMap::new());
        assert!(set.matches(&info(Some("ru"), None)), "country, lower-cased");
        assert!(set.matches(&info(Some("RU"), None)), "country, upper-cased");
        assert!(
            set.matches(&info(Some("de"), Some(13335))),
            "asn match alone"
        );
        assert!(
            !set.matches(&info(Some("de"), Some(15169))),
            "neither matches"
        );
    }

    #[test]
    fn effective_set_unions_global_group_and_device() {
        let global = GeoWatch {
            countries: vec!["ru".to_owned()],
            asns: vec![],
        };
        let group = Group {
            id: "g1".to_owned(),
            name: "G".to_owned(),
            policy: PolicyDelta {
                watch_asns: vec![64500],
                ..Default::default()
            },
        };
        let mut groups = HashMap::new();
        groups.insert("g1".to_owned(), group);
        let device = device_with_watch("10.0.0.1", Some("g1"), &["cn"], &[13335]);

        let set = effective_watch(&global, Some(&device), &groups);
        assert!(set.matches(&info(Some("ru"), None)), "from global");
        assert!(set.matches(&info(Some("cn"), None)), "from device");
        assert!(set.matches(&info(None, Some(13335))), "asn from device");
        assert!(set.matches(&info(None, Some(64500))), "asn from group");
        assert!(!set.matches(&info(Some("us"), Some(1))), "unwatched");
    }

    #[test]
    fn anything_watched_is_a_cheap_gate() {
        let empty = GeoWatch::default();
        assert!(!anything_watched(&empty, &[], &[]));
        let device = device_with_watch("10.0.0.2", None, &["cn"], &[]);
        assert!(anything_watched(&empty, std::slice::from_ref(&device), &[]));
    }
}
