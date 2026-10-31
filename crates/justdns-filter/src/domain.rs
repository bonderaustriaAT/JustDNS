// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A set of blocked domains with sub-domain matching.
//!
//! A [`DomainSet`] stores each rule as a normalised domain. A query matches
//! when the queried name equals a stored domain or is a sub-domain of one, so
//! listing `doubleclick.net` also blocks `ad.doubleclick.net`. Matching walks
//! the name's parent labels, which is cheap and needs no per-query allocation
//! beyond lower-casing the name once.

use std::collections::HashSet;

/// A collection of blocked domains that answers "is this name blocked?".
#[derive(Clone, Debug, Default)]
pub struct DomainSet {
    domains: HashSet<String>,
}

impl DomainSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a set from the text of a block list. See [`parse_rule`] for the
    /// accepted line formats; lines that are blank, comments or unparseable are
    /// skipped.
    pub fn from_rules(text: &str) -> Self {
        let mut set = Self::new();
        set.add_rules(text);
        set
    }

    /// Add every rule in `text` to this set, keeping what is already there.
    pub fn add_rules(&mut self, text: &str) {
        for line in text.lines() {
            if let Some(domain) = parse_rule(line) {
                self.domains.insert(domain);
            }
        }
    }

    /// Number of distinct domains in the set.
    pub fn len(&self) -> usize {
        self.domains.len()
    }

    /// Whether the set holds no domains.
    pub fn is_empty(&self) -> bool {
        self.domains.is_empty()
    }

    /// Whether `name` is blocked: true when the normalised name equals a stored
    /// domain or is a sub-domain of one.
    pub fn blocks(&self, name: &str) -> bool {
        if self.domains.is_empty() {
            return false;
        }
        let name = normalise(name);
        if name.is_empty() {
            return false;
        }

        // Check the full name, then each parent suffix: for `a.b.example.com`
        // try `a.b.example.com`, `b.example.com`, `example.com`, `com`.
        let mut candidate = name.as_str();
        loop {
            if self.domains.contains(candidate) {
                return true;
            }
            match candidate.find('.') {
                Some(dot) => candidate = &candidate[dot + 1..],
                None => return false,
            }
        }
    }
}

/// Parse one block-list line into a normalised domain, or `None` if the line
/// carries no rule. Accepts three common formats:
///
/// - a bare domain: `doubleclick.net`
/// - a hosts-file line: `0.0.0.0 doubleclick.net` (the address is dropped)
/// - an Adblock-style host rule: `||doubleclick.net^`
///
/// Blank lines and comments (`#` or `!`) yield `None`, as do lines that do not
/// look like a plain host name (anything with a path, wildcard or option is
/// left for a richer rule engine in a later milestone).
pub fn parse_rule(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }

    // Adblock host rule: ||domain^ (no path, no options).
    if let Some(rest) = line.strip_prefix("||") {
        let domain = rest.strip_suffix('^').unwrap_or(rest);
        return accept_host(domain);
    }

    // Hosts-file line: an address followed by one or more names. Take the last
    // field so `0.0.0.0 ads.example.com` yields the name, and drop the
    // loopback aliases a hosts file usually carries.
    let mut fields = line.split_whitespace();
    let first = fields.next()?;
    let host = match fields.next_back() {
        Some(last) => {
            if is_ip_like(first) {
                last
            } else {
                // More than one field but no leading address: not a host line.
                return None;
            }
        }
        None => first,
    };

    if matches!(
        host,
        "localhost" | "localhost.localdomain" | "broadcasthost"
    ) {
        return None;
    }
    accept_host(host)
}

/// Accept `host` as a block rule if it is a plausible domain name, returning it
/// normalised. Rejects anything with a path, port, wildcard or illegal label
/// so a malformed line never becomes an over-broad rule.
fn accept_host(host: &str) -> Option<String> {
    let host = normalise(host);
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    let valid = host.split('.').all(|label| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    });
    valid.then_some(host)
}

/// Lower-case `name` and strip a trailing dot and a leading `*.` wildcard so
/// stored rules and queried names compare directly.
fn normalise(name: &str) -> String {
    let name = name.trim().trim_end_matches('.');
    let name = name.strip_prefix("*.").unwrap_or(name);
    name.to_ascii_lowercase()
}

/// Whether `field` looks like the address column of a hosts-file line.
fn is_ip_like(field: &str) -> bool {
    field.parse::<std::net::IpAddr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_domain_and_subdomains() {
        let set = DomainSet::from_rules("doubleclick.net\n");
        assert!(set.blocks("doubleclick.net"));
        assert!(set.blocks("ad.doubleclick.net"));
        assert!(set.blocks("a.b.doubleclick.net"));
        assert!(set.blocks("DOUBLECLICK.NET"));
        assert!(set.blocks("ad.doubleclick.net.")); // trailing dot
    }

    #[test]
    fn does_not_match_unrelated_or_partial_names() {
        let set = DomainSet::from_rules("doubleclick.net\n");
        assert!(!set.blocks("example.com"));
        assert!(!set.blocks("net"));
        // A name that merely ends with the text but not on a label boundary.
        assert!(!set.blocks("faveddoubleclick.net"));
        // A blocked name appearing as a left-hand label must not match.
        assert!(!set.blocks("doubleclick.net.evil.com"));
    }

    #[test]
    fn empty_set_blocks_nothing() {
        let set = DomainSet::new();
        assert!(set.is_empty());
        assert!(!set.blocks("doubleclick.net"));
    }

    #[test]
    fn parses_the_three_line_formats() {
        assert_eq!(
            parse_rule("doubleclick.net"),
            Some("doubleclick.net".into())
        );
        assert_eq!(
            parse_rule("0.0.0.0 ads.example.com"),
            Some("ads.example.com".into())
        );
        assert_eq!(
            parse_rule("127.0.0.1\ttracker.example.net"),
            Some("tracker.example.net".into())
        );
        assert_eq!(
            parse_rule("||analytics.example.org^"),
            Some("analytics.example.org".into())
        );
    }

    #[test]
    fn skips_comments_blanks_and_loopback() {
        assert_eq!(parse_rule(""), None);
        assert_eq!(parse_rule("   "), None);
        assert_eq!(parse_rule("# a comment"), None);
        assert_eq!(parse_rule("! adblock comment"), None);
        assert_eq!(parse_rule("127.0.0.1 localhost"), None);
        assert_eq!(parse_rule("::1 localhost"), None);
    }

    #[test]
    fn rejects_malformed_or_overbroad_hosts() {
        assert_eq!(parse_rule("com"), None); // no dot, would block a whole TLD
        assert_eq!(parse_rule("ads.example.com/path"), None);
        assert_eq!(parse_rule("ads example com bad"), None);
        assert_eq!(parse_rule("-bad.example.com"), None);
        assert_eq!(parse_rule("bad-.example.com"), None);
    }

    #[test]
    fn from_rules_counts_distinct_domains() {
        let set = DomainSet::from_rules("a.example\n# comment\n\nb.example\na.example\n");
        assert_eq!(set.len(), 2);
    }
}
