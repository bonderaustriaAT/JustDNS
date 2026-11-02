// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local DNS rewrites: hostnames the resolver answers itself from a fixed IP,
//! instead of forwarding them upstream.
//!
//! A rewrite maps one exact name to one or more IP addresses (for example
//! `nas.home` → `192.168.20.5`). The resolver answers a matching A query with
//! the IPv4 addresses and a matching AAAA query with the IPv6 ones; any other
//! name falls through to the normal cache and upstream path.
//!
//! This crate owns both the live lookup table the DNS pipeline reads per query
//! ([`Rewriter`], swapped atomically so an edit takes effect at once) and the
//! persistence of the user's entries ([`RewriteStore`], a JSON file on the data
//! volume). It mirrors the split between [`justdns_filter`] and `justdns_lists`.

mod store;

pub use store::{RewriteError, RewriteStore, RewritesView};

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

/// One rewrite: an exact name and the addresses it resolves to. Serialised in
/// camelCase to match the dashboard's JSON directly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rewrite {
    /// The name this rewrite answers, already normalised (lowercase, no trailing
    /// dot).
    pub name: String,
    /// The addresses the name resolves to. IPv4 addresses answer A queries,
    /// IPv6 addresses answer AAAA queries.
    pub ips: Vec<IpAddr>,
    /// Names of the reusable address objects (aliases) this rewrite points at.
    /// Each is resolved to the object's current addresses when the live table is
    /// built, so changing one object updates every rewrite that references it at
    /// once. Alongside `ips`: a rewrite answers with its own addresses plus every
    /// referenced object's. Defaults to empty so entries from an older build load.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Whether the rewrite is active. A disabled entry stays in the list and the
    /// file but is left out of the live table, so the name falls through to the
    /// normal upstream path until it is switched on again. Defaults to `true` so
    /// entries written by an older build load as enabled.
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

/// The default for a rewrite's `enabled` flag: a new or legacy entry is active.
fn enabled_default() -> bool {
    true
}

/// A reusable address object: a short name that stands for one or more IP
/// addresses. A rewrite can point at an object by name instead of repeating the
/// address, and changing the object's addresses updates every rewrite that
/// references it. Serialised in camelCase to match the dashboard's JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    /// The object's name, already normalised (lowercase), used by rewrites to
    /// reference it.
    pub name: String,
    /// The addresses the object stands for. A referencing rewrite answers A
    /// queries from the IPv4 ones and AAAA queries from the IPv6 ones.
    pub ips: Vec<IpAddr>,
}

/// The compiled lookup table: normalised name → resolved addresses. Built fresh
/// on every change (alias references already resolved to concrete IPs) and
/// swapped into the live [`Rewriter`] in one step.
#[derive(Debug, Default)]
struct RewriteTable {
    by_name: HashMap<String, Vec<IpAddr>>,
}

/// The live rewrite table the DNS pipeline reads on every query. Lock-free: an
/// edit builds a new table and swaps it in, so a lookup never waits on a writer.
#[derive(Debug, Default)]
pub struct Rewriter {
    table: ArcSwap<RewriteTable>,
}

impl Rewriter {
    /// An empty rewriter — nothing is answered locally until entries are added.
    pub fn new() -> Self {
        Self {
            table: ArcSwap::from_pointee(RewriteTable::default()),
        }
    }

    /// The addresses `name` resolves to locally, or `None` if it has no rewrite.
    /// The name is normalised before the lookup, so a trailing dot or odd case
    /// from the wire still matches a stored entry.
    pub fn lookup(&self, name: &str) -> Option<Vec<IpAddr>> {
        let normalised = normalise(name);
        self.table.load().by_name.get(&normalised).cloned()
    }

    /// Replace the whole table from already-resolved `(name, ips)` pairs. The
    /// store resolves each rewrite's own addresses and its alias references to
    /// concrete IPs before calling this, so the live table only ever holds plain
    /// addresses and a lookup stays a single map hit.
    pub fn set_resolved<I>(&self, entries: I)
    where
        I: IntoIterator<Item = (String, Vec<IpAddr>)>,
    {
        let by_name: HashMap<String, Vec<IpAddr>> = entries.into_iter().collect();
        self.table.store(Arc::new(RewriteTable { by_name }));
    }

    /// Replace the whole table from `rewrites`, skipping disabled ones. A
    /// convenience for callers with no address objects to resolve (tests and the
    /// pipeline); it ignores the `aliases` field.
    pub fn set(&self, rewrites: &[Rewrite]) {
        self.set_resolved(
            rewrites
                .iter()
                .filter(|rewrite| rewrite.enabled)
                .map(|rewrite| (rewrite.name.clone(), rewrite.ips.clone())),
        );
    }

    /// How many names currently have a rewrite.
    pub fn len(&self) -> usize {
        self.table.load().by_name.len()
    }

    /// Whether no name is rewritten.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Normalise a name for storage and lookup: trim surrounding space, drop a
/// single trailing dot (the root label as it arrives on the wire) and lowercase
/// it, so `NAS.Home.` and `nas.home` are the same entry.
pub fn normalise(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Whether `name` is a usable rewrite target: one or more dot-separated labels,
/// each 1–63 characters of ASCII letters, digits or hyphen, and no leading or
/// trailing hyphen on a label. A single label (for example `router`) is allowed,
/// so short LAN names work.
pub fn valid_host(name: &str) -> bool {
    let name = normalise(name);
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    name.split('.').all(valid_label)
}

fn valid_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 63 {
        return false;
    }
    if label.starts_with('-') || label.ends_with('-') {
        return false;
    }
    label
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Whether `name` is a usable address-object (alias) name: a single label of
/// 1–63 ASCII letters, digits or hyphen, with no leading or trailing hyphen and
/// no dots. Kept to one label (unlike a host name) so an object name such as
/// `reverse-proxy` never reads like a domain.
pub fn valid_alias_name(name: &str) -> bool {
    valid_label(&normalise(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn normalise_strips_dot_and_case() {
        assert_eq!(normalise("NAS.Home."), "nas.home");
        assert_eq!(normalise("  Router  "), "router");
    }

    #[test]
    fn valid_host_accepts_lan_names_and_rejects_junk() {
        assert!(valid_host("nas.home"));
        assert!(valid_host("router"));
        assert!(valid_host("git-server.lan."));
        assert!(!valid_host(""));
        assert!(!valid_host("bad_label.home")); // underscore not allowed
        assert!(!valid_host("-leading.home"));
        assert!(!valid_host("has space.home"));
    }

    #[test]
    fn lookup_matches_normalised_name() {
        let rewriter = Rewriter::new();
        rewriter.set(&[Rewrite {
            name: "nas.home".to_owned(),
            ips: vec![
                IpAddr::V4(Ipv4Addr::new(192, 168, 20, 5)),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ],
            aliases: Vec::new(),
            enabled: true,
        }]);

        // A wire-form name (trailing dot, mixed case) still hits the entry.
        let ips = rewriter.lookup("NAS.Home.").expect("name is rewritten");
        assert_eq!(ips.len(), 2);
        assert!(rewriter.lookup("other.home").is_none());
        assert_eq!(rewriter.len(), 1);
    }

    #[test]
    fn disabled_rewrite_is_not_answered() {
        let rewriter = Rewriter::new();
        rewriter.set(&[
            Rewrite {
                name: "on.home".to_owned(),
                ips: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))],
                aliases: Vec::new(),
                enabled: true,
            },
            Rewrite {
                name: "off.home".to_owned(),
                ips: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))],
                aliases: Vec::new(),
                enabled: false,
            },
        ]);

        assert!(rewriter.lookup("on.home").is_some());
        assert!(
            rewriter.lookup("off.home").is_none(),
            "disabled falls through"
        );
        assert_eq!(rewriter.len(), 1, "only enabled entries are live");
    }
}
