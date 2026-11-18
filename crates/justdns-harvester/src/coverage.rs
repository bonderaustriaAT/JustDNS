// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Domain-name helpers: the registrable-root derivation and the suffix-match
//! set that tells us a candidate is already covered by an existing list.
//!
//! The harvester must not re-propose a domain an enabled feed already blocks, so
//! it downloads the current DACH list and the active Hagezi feeds and folds every
//! line into a [`SuffixSet`]. Matching reuses the same parent-suffix walk the
//! JustDNS blocker uses: a lookup of `px.ads.doubleclick.net` tries the whole
//! name, then `ads.doubleclick.net`, then `doubleclick.net`.
//!
//! Registrable-root derivation uses a small, documented subset of the Public
//! Suffix List rather than pulling the whole PSL in: the DACH second-level
//! suffixes that actually occur (`co.at`, `or.at`, …) plus a handful of common
//! international ones. Anything else falls back to "last two labels", which is
//! correct for the flat TLDs that dominate here (`.de`, `.ch`, `.com`, `.net`).

use std::collections::HashSet;

/// Multi-label public suffixes we recognise, so a host under one derives its
/// registrable root from three labels, not two. A pragmatic subset of the PSL —
/// the DACH second-levels that occur in practice plus common international ones.
/// Documented as deliberately non-exhaustive; the fallback is "last two labels".
const MULTI_SUFFIXES: &[&str] = &[
    // Austria
    "co.at", "or.at", "ac.at", "gv.at", "priv.at",
    // United Kingdom (common on international ad-tech)
    "co.uk", "org.uk", "ac.uk", "gov.uk",
    // A few other frequently seen second-levels
    "com.au", "co.nz", "co.jp", "com.br",
];

/// Normalise a queried name for matching: lowercase and strip a trailing dot.
pub fn normalise(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// The registrable root of a host: the shortest suffix that is still a real,
/// registrable domain. `a.b.example.co.at` → `example.co.at`; `x.y.example.com`
/// → `example.com`. Input should already be [`normalise`]d; an empty or
/// single-label input is returned unchanged.
pub fn registrable_root(host: &str) -> String {
    let host = normalise(host);
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() <= 2 {
        return labels.join(".");
    }
    // If the last two labels form a known multi-label suffix, the root is the
    // last three; otherwise the last two.
    let last_two = format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1]);
    let take = if MULTI_SUFFIXES.contains(&last_two.as_str()) {
        3
    } else {
        2
    };
    let start = labels.len().saturating_sub(take);
    labels[start..].join(".")
}

/// A set of blocklist entries, matched by domain suffix like the JustDNS blocker.
#[derive(Debug, Default, Clone)]
pub struct SuffixSet {
    entries: HashSet<String>,
}

impl SuffixSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct entries the set holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Fold one blocklist body into the set. Each non-empty, non-comment line is
    /// read as a single domain (the `wildcard/…-onlydomains` and DACH formats);
    /// an optional `*.` prefix and a leading `||`/trailing `^` (Adblock-ish) are
    /// stripped so mixed sources still land as bare domains.
    pub fn add_list(&mut self, body: &str) {
        for raw in body.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let line = line
                .trim_start_matches("||")
                .trim_start_matches("*.")
                .trim_end_matches('^');
            // A hosts-file style "0.0.0.0 domain" line: take the last field.
            let domain = line.split_whitespace().last().unwrap_or(line);
            let domain = normalise(domain);
            if !domain.is_empty() && domain.contains('.') {
                self.entries.insert(domain);
            }
        }
    }

    /// Whether `host` is covered: the host itself or any parent suffix is in the
    /// set. `px.ads.doubleclick.net` is covered by an entry for `doubleclick.net`.
    pub fn covers(&self, host: &str) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let host = normalise(host);
        let mut rest: &str = &host;
        loop {
            if self.entries.contains(rest) {
                return true;
            }
            match rest.find('.') {
                Some(dot) => rest = &rest[dot + 1..],
                None => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrable_root_handles_flat_and_multi_suffixes() {
        assert_eq!(registrable_root("code.etracker.com"), "etracker.com");
        assert_eq!(registrable_root("met.vgwort.de"), "vgwort.de");
        assert_eq!(registrable_root("ioam.de"), "ioam.de");
        assert_eq!(registrable_root("a.b.example.co.at"), "example.co.at");
        assert_eq!(registrable_root("www.oewabox.at"), "oewabox.at");
        assert_eq!(registrable_root("x.y.z.webtrekk.net"), "webtrekk.net");
        // Trailing dot and case are normalised away.
        assert_eq!(registrable_root("Script.IOAM.de."), "ioam.de");
    }

    #[test]
    fn suffix_set_matches_parents_and_ignores_comments() {
        let mut set = SuffixSet::new();
        set.add_list(
            "# a comment\ndoubleclick.net\nioam.de\n\n0.0.0.0 ads.example.com\n||tracker.io^\n",
        );
        assert!(set.covers("doubleclick.net"));
        assert!(set.covers("px.ads.doubleclick.net"));
        assert!(set.covers("ioam.de"));
        assert!(set.covers("ads.example.com"));
        assert!(set.covers("sub.tracker.io"));
        assert!(!set.covers("example.com"));
        assert!(!set.covers("notdoubleclick.net"));
    }
}
