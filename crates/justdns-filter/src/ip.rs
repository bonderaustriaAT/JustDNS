// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A set of blocked IPv4 addresses and CIDR ranges with longest-prefix matching.
//!
//! An [`IpSet`] answers "is this address blocked?" for the addresses carried in a
//! resolver's answer. Single addresses are kept in a hash set; CIDR ranges are
//! grouped by prefix length so a lookup masks the queried address once per
//! distinct prefix length and does a hash-set membership test, at most 32 probes
//! and no per-query allocation. Only IPv4 is handled today; the structure leaves
//! room for an IPv6 set to be added the same way later.

use std::collections::HashSet;
use std::net::Ipv4Addr;

use arc_swap::ArcSwap;
use std::sync::Arc;

/// A collection of blocked IPv4 addresses and ranges.
#[derive(Clone, Debug, Default)]
pub struct IpSet {
    /// Single addresses and `/32` ranges, as raw big-endian `u32`s.
    v4_exact: HashSet<u32>,
    /// CIDR ranges grouped by prefix length (1..=31). Each group holds the
    /// masked network addresses for that length, so a lookup masks the query to
    /// the group's length and tests membership.
    v4_cidr: Vec<(u8, HashSet<u32>)>,
}

impl IpSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a set from the text of an IP block list. See [`parse_ip_rule`] for
    /// the accepted line formats; lines that are blank, comments or unparseable
    /// are skipped.
    pub fn from_rules(text: &str) -> Self {
        let mut set = Self::new();
        set.add_rules(text);
        set
    }

    /// Add every rule in `text` to this set, keeping what is already there.
    pub fn add_rules(&mut self, text: &str) {
        for line in text.lines() {
            match parse_ip_rule(line) {
                Some(IpRule::Exact(addr)) => {
                    self.v4_exact.insert(u32::from(addr));
                }
                Some(IpRule::Cidr { network, prefix }) => {
                    // `/32` is just an exact address; `/0` matches everything, so
                    // keep it as a zero-length group with a single zero network.
                    if prefix == 32 {
                        self.v4_exact.insert(network);
                    } else {
                        self.insert_cidr(prefix, network);
                    }
                }
                None => {}
            }
        }
    }

    fn insert_cidr(&mut self, prefix: u8, network: u32) {
        if let Some((_, set)) = self.v4_cidr.iter_mut().find(|(len, _)| *len == prefix) {
            set.insert(network);
        } else {
            let mut set = HashSet::new();
            set.insert(network);
            self.v4_cidr.push((prefix, set));
        }
    }

    /// Number of distinct exact addresses and CIDR ranges in the set.
    pub fn len(&self) -> usize {
        self.v4_exact.len() + self.v4_cidr.iter().map(|(_, set)| set.len()).sum::<usize>()
    }

    /// Whether the set holds no addresses or ranges.
    pub fn is_empty(&self) -> bool {
        self.v4_exact.is_empty() && self.v4_cidr.iter().all(|(_, set)| set.is_empty())
    }

    /// Whether `addr` is blocked: it equals a stored address or falls inside one
    /// of the stored CIDR ranges.
    pub fn contains(&self, addr: Ipv4Addr) -> bool {
        let bits = u32::from(addr);
        if self.v4_exact.contains(&bits) {
            return true;
        }
        for (prefix, set) in &self.v4_cidr {
            if set.contains(&(bits & mask_v4(*prefix))) {
                return true;
            }
        }
        false
    }
}

/// The left-aligned IPv4 network mask for a prefix length (`0..=32`).
fn mask_v4(prefix: u8) -> u32 {
    match prefix {
        0 => 0,
        p if p >= 32 => u32::MAX,
        p => u32::MAX << (32 - p),
    }
}

/// One parsed IP block-list rule.
enum IpRule {
    /// A single address (a bare address or an explicit `/32`).
    Exact(Ipv4Addr),
    /// A CIDR range, already masked to its network address.
    Cidr { network: u32, prefix: u8 },
}

/// Parse one IP block-list line into a rule, or `None` if the line carries none.
///
/// Accepts a bare IPv4 address (`1.2.3.4`) or a CIDR range (`1.2.3.0/24`). The
/// address of a CIDR is masked to its network, so `1.2.3.5/24` is stored as
/// `1.2.3.0/24`. Blank lines and comments (`#`, `;` or `!`) yield `None`, as does
/// anything that is not a plain IPv4 address or range (IPv6 is not handled yet).
/// Only the first whitespace-separated field is read, so a trailing comment on a
/// rule line is ignored.
fn parse_ip_rule(line: &str) -> Option<IpRule> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') || line.starts_with('!') {
        return None;
    }

    let token = line.split_whitespace().next()?;

    if let Some((addr, prefix)) = token.split_once('/') {
        let addr: Ipv4Addr = addr.parse().ok()?;
        let prefix: u8 = prefix.parse().ok()?;
        if prefix > 32 {
            return None;
        }
        let network = u32::from(addr) & mask_v4(prefix);
        return Some(IpRule::Cidr { network, prefix });
    }

    let addr: Ipv4Addr = token.parse().ok()?;
    Some(IpRule::Exact(addr))
}

/// A live-swappable IPv4 block set. The binary rebuilds the set when the user's
/// IP lists change and publishes it here; the resolver reads it on the answer
/// path with a wait-free load. Cheap to share: clone the [`Arc`] the binary wraps
/// it in.
#[derive(Debug, Default)]
pub struct IpBlocker {
    set: ArcSwap<IpSet>,
}

impl IpBlocker {
    /// An empty blocker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the block set. Takes effect on the next answer.
    pub fn publish(&self, set: IpSet) {
        self.set.store(Arc::new(set));
    }

    /// Whether the block set is empty, so the answer path can skip the check.
    pub fn is_empty(&self) -> bool {
        self.set.load().is_empty()
    }

    /// Whether `addr` is on the block set.
    pub fn contains(&self, addr: Ipv4Addr) -> bool {
        self.set.load().contains(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_addresses_match_only_themselves() {
        let set = IpSet::from_rules("1.2.3.4\n9.9.9.9\n");
        assert!(set.contains("1.2.3.4".parse().unwrap()));
        assert!(set.contains("9.9.9.9".parse().unwrap()));
        assert!(!set.contains("1.2.3.5".parse().unwrap()));
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
    }

    #[test]
    fn cidr_matches_every_address_in_range() {
        let set = IpSet::from_rules("10.0.0.0/8\n192.168.1.0/24\n");
        assert!(set.contains("10.255.255.254".parse().unwrap()));
        assert!(set.contains("192.168.1.42".parse().unwrap()));
        assert!(!set.contains("192.168.2.1".parse().unwrap()));
        assert!(!set.contains("11.0.0.1".parse().unwrap()));
    }

    #[test]
    fn cidr_host_bits_are_masked_to_the_network() {
        // 1.2.3.5/24 means the whole 1.2.3.0/24 range, not just 1.2.3.5.
        let set = IpSet::from_rules("1.2.3.5/24\n");
        assert!(set.contains("1.2.3.0".parse().unwrap()));
        assert!(set.contains("1.2.3.200".parse().unwrap()));
        assert!(!set.contains("1.2.4.0".parse().unwrap()));
    }

    #[test]
    fn overlapping_ranges_and_exact_all_match() {
        // A /32, a /24 and a /16 that overlap: any containing rule blocks.
        let set = IpSet::from_rules("5.5.5.5/32\n5.5.5.0/24\n5.5.0.0/16\n");
        assert!(set.contains("5.5.5.5".parse().unwrap())); // all three
        assert!(set.contains("5.5.5.99".parse().unwrap())); // /24 and /16
        assert!(set.contains("5.5.9.9".parse().unwrap())); // /16 only
        assert!(!set.contains("5.6.0.0".parse().unwrap()));
    }

    #[test]
    fn slash_zero_matches_everything() {
        let set = IpSet::from_rules("0.0.0.0/0\n");
        assert!(set.contains("8.8.8.8".parse().unwrap()));
        assert!(set.contains("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn comments_blanks_and_invalid_lines_are_skipped() {
        let set = IpSet::from_rules(
            "# a comment\n; another\n! and another\n\n  \nnot-an-ip\n1.2.3.4/33\n2001:db8::1\n7.7.7.7\n",
        );
        assert!(set.contains("7.7.7.7".parse().unwrap()));
        assert_eq!(set.len(), 1, "only the one valid address is kept");
    }

    #[test]
    fn trailing_comment_on_a_rule_line_is_ignored() {
        let set = IpSet::from_rules("3.3.3.3 # malware c2\n");
        assert!(set.contains("3.3.3.3".parse().unwrap()));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn empty_set_matches_nothing() {
        let set = IpSet::new();
        assert!(set.is_empty());
        assert!(!set.contains("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn blocker_publishes_and_reads_live() {
        let blocker = IpBlocker::new();
        assert!(blocker.is_empty());
        assert!(!blocker.contains("1.2.3.4".parse().unwrap()));

        blocker.publish(IpSet::from_rules("1.2.3.0/24\n"));
        assert!(!blocker.is_empty());
        assert!(blocker.contains("1.2.3.99".parse().unwrap()));
        assert!(!blocker.contains("1.2.4.0".parse().unwrap()));

        blocker.publish(IpSet::new());
        assert!(blocker.is_empty());
    }
}
