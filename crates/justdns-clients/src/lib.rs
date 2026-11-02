// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Named clients (devices): a label and a kind for a client address, so the log
//! and the statistics can speak in device names instead of bare IPs.
//!
//! A device points at either a single address (`192.168.20.10` → "Justin's
//! laptop") or a range in CIDR form (`192.168.30.0/24` → "Guest Wi-Fi"). When a
//! client address is looked up, an exact single-address entry wins over any
//! range, and among ranges the longest prefix wins, so a specific device inside
//! a named subnet still shows under its own name.
//!
//! This crate owns the data model and the matcher; [`ClientStore`] (in the
//! `store` module) persists the user's devices to a JSON file on the data volume.
//! Naming is a read-time, presentation concern: it is never on the DNS hot path,
//! so there is no live lookup table here — the web layer builds a [`ClientResolver`]
//! from a snapshot when it renders a view.

mod store;

pub use store::{ClientError, ClientStore};

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// The kind of device a name stands for, shown as an icon in the dashboard. A
/// plain closed set: enough to tell a phone from a TV at a glance without turning
/// into a catalogue to maintain. Serialised lowercase to match the UI's JSON.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Phone,
    Laptop,
    Desktop,
    Tablet,
    Tv,
    Console,
    /// A smart-home or other always-on gadget.
    Iot,
    Server,
    Printer,
    Router,
    /// Anything that does not fit, and the default for an entry written by an
    /// older build or left unset.
    #[default]
    Other,
}

/// One named client: the address or range it matches, a display name, an
/// optional note, and a device kind. Serialised in camelCase to match the
/// dashboard's JSON directly. `target` is kept in its normalised text form (a
/// canonical IP, or a masked CIDR such as `192.168.30.0/24`) so the file and the
/// UI round-trip exactly what the matcher uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// The client address or CIDR range this entry names, already normalised.
    pub target: String,
    /// The display name shown in the log and statistics in place of the address.
    pub name: String,
    /// A free-text note (for example "kids' room, tablet"). Absent when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The device kind, shown as an icon. Defaults to [`DeviceKind::Other`].
    #[serde(default)]
    pub kind: DeviceKind,
}

/// What a device matches: a single address, or a CIDR range. Parsed from a
/// device's `target` for matching and normalisation; never stored directly (the
/// stored form is the normalised string).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientMatch {
    /// Exactly one address.
    Ip(IpAddr),
    /// Every address in a CIDR range, held as a masked base plus a prefix length.
    Net { base: IpAddr, prefix: u8 },
}

impl ClientMatch {
    /// Whether `ip` falls under this matcher.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match *self {
            ClientMatch::Ip(addr) => addr == ip,
            ClientMatch::Net { base, prefix } => in_net(base, prefix, ip),
        }
    }

    /// How specific the matcher is, as a prefix length: a single address is a
    /// full-length prefix (32 for v4, 128 for v6), a range its CIDR prefix. Used
    /// to prefer the most specific match.
    pub fn prefix_len(&self) -> u8 {
        match *self {
            ClientMatch::Ip(IpAddr::V4(_)) => 32,
            ClientMatch::Ip(IpAddr::V6(_)) => 128,
            ClientMatch::Net { prefix, .. } => prefix,
        }
    }

    /// Whether this matcher is a range rather than a single address.
    pub fn is_range(&self) -> bool {
        matches!(self, ClientMatch::Net { .. })
    }
}

impl fmt::Display for ClientMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientMatch::Ip(addr) => write!(f, "{addr}"),
            ClientMatch::Net { base, prefix } => write!(f, "{base}/{prefix}"),
        }
    }
}

/// Parse a device target in its text form — a plain IP (`192.168.20.10`) or a
/// CIDR range (`192.168.30.0/24`) — into a matcher, with the base masked to its
/// prefix so the stored form is canonical. Returns `None` for anything that is
/// not a usable address or range (a bad address, a prefix wider than the family
/// allows, or a v4 prefix on a v6 base).
pub fn parse_target(target: &str) -> Option<ClientMatch> {
    let target = target.trim();
    if let Some((addr, prefix)) = target.split_once('/') {
        let base: IpAddr = addr.trim().parse().ok()?;
        let prefix: u8 = prefix.trim().parse().ok()?;
        let max = match base {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > max {
            return None;
        }
        // Store the masked network address, so `192.168.30.42/24` and
        // `192.168.30.0/24` are the same entry and matching is a plain compare.
        let base = mask(base, prefix);
        Some(ClientMatch::Net { base, prefix })
    } else {
        Some(ClientMatch::Ip(target.parse().ok()?))
    }
}

/// Whether `ip` is inside the network `base/prefix`. A family mismatch (a v4
/// address against a v6 range or the reverse) never matches.
fn in_net(base: IpAddr, prefix: u8, ip: IpAddr) -> bool {
    match (base, ip) {
        (IpAddr::V4(base), IpAddr::V4(ip)) => {
            let mask = v4_mask(prefix);
            (u32::from(base) & mask) == (u32::from(ip) & mask)
        }
        (IpAddr::V6(base), IpAddr::V6(ip)) => {
            let mask = v6_mask(prefix);
            (u128::from(base) & mask) == (u128::from(ip) & mask)
        }
        _ => false,
    }
}

/// Mask `base` down to its network address for `prefix`.
fn mask(base: IpAddr, prefix: u8) -> IpAddr {
    match base {
        IpAddr::V4(addr) => IpAddr::V4((u32::from(addr) & v4_mask(prefix)).into()),
        IpAddr::V6(addr) => IpAddr::V6((u128::from(addr) & v6_mask(prefix)).into()),
    }
}

/// The 32-bit mask for an IPv4 prefix length (0–32). A prefix of 0 masks nothing.
fn v4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix.min(32) as u32)
    }
}

/// The 128-bit mask for an IPv6 prefix length (0–128).
fn v6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix.min(128) as u32)
    }
}

/// Whether `name` is a usable device name: non-empty after trimming and at most
/// 64 characters. Free text otherwise — a device name is a human label, not a
/// host name, so spaces, accents and the like are fine.
pub fn valid_device_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && trimmed.chars().count() <= 64
}

/// Resolves a client address to the device that names it, with exact single
/// addresses winning over ranges and the longest prefix winning among ranges.
/// Built from a snapshot of the stored devices when a view is rendered; cheap to
/// build and never touched by the DNS path.
pub struct ClientResolver {
    devices: Vec<Device>,
    /// Index of every single-address device, address → position in `devices`.
    by_ip: HashMap<IpAddr, usize>,
    /// Every range device as `(matcher, position)`, sorted by descending prefix
    /// length so the first containing match is the most specific one.
    nets: Vec<(ClientMatch, usize)>,
}

impl ClientResolver {
    /// Build a resolver from the stored devices. Entries whose target no longer
    /// parses are skipped rather than failing the whole view.
    pub fn new(devices: Vec<Device>) -> Self {
        let mut by_ip = HashMap::new();
        let mut nets: Vec<(ClientMatch, usize)> = Vec::new();
        for (index, device) in devices.iter().enumerate() {
            match parse_target(&device.target) {
                Some(ClientMatch::Ip(addr)) => {
                    by_ip.entry(addr).or_insert(index);
                }
                Some(matcher @ ClientMatch::Net { .. }) => nets.push((matcher, index)),
                None => {}
            }
        }
        // Longest prefix first, so `resolve` can take the first containing range.
        nets.sort_by_key(|entry| std::cmp::Reverse(entry.0.prefix_len()));
        Self {
            devices,
            by_ip,
            nets,
        }
    }

    /// The device that names `ip`, if any: an exact single-address entry first,
    /// otherwise the most specific range containing it.
    pub fn resolve(&self, ip: IpAddr) -> Option<&Device> {
        if let Some(&index) = self.by_ip.get(&ip) {
            return Some(&self.devices[index]);
        }
        self.nets
            .iter()
            .find(|(matcher, _)| matcher.contains(ip))
            .map(|&(_, index)| &self.devices[index])
    }

    /// The display name for `ip`, if a device matches it.
    pub fn name_for(&self, ip: IpAddr) -> Option<&str> {
        self.resolve(ip).map(|device| device.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_masks_a_cidr_target() {
        let matcher = parse_target("192.168.30.42/24").unwrap();
        assert_eq!(matcher.to_string(), "192.168.30.0/24", "base is masked");
        assert!(matcher.contains("192.168.30.200".parse().unwrap()));
        assert!(!matcher.contains("192.168.31.1".parse().unwrap()));
        assert!(matcher.is_range());
    }

    #[test]
    fn parses_a_single_address() {
        let matcher = parse_target("192.168.20.10").unwrap();
        assert_eq!(matcher.prefix_len(), 32);
        assert!(matcher.contains("192.168.20.10".parse().unwrap()));
        assert!(!matcher.contains("192.168.20.11".parse().unwrap()));
        assert!(!matcher.is_range());
    }

    #[test]
    fn rejects_junk_and_too_wide_prefixes() {
        assert!(parse_target("not-an-ip").is_none());
        assert!(parse_target("192.168.0.0/33").is_none(), "v4 max is /32");
        assert!(parse_target("::1/129").is_none(), "v6 max is /128");
    }

    #[test]
    fn ipv6_ranges_match_within_family_only() {
        let matcher = parse_target("2001:db8::/32").unwrap();
        assert!(matcher.contains("2001:db8::1".parse().unwrap()));
        assert!(!matcher.contains("2001:dead::1".parse().unwrap()));
        // A v4 address never falls inside a v6 range.
        assert!(!matcher.contains("192.168.0.1".parse().unwrap()));
    }

    #[test]
    fn exact_address_wins_over_a_range() {
        let devices = vec![
            Device {
                target: "192.168.30.0/24".to_owned(),
                name: "Guest Wi-Fi".to_owned(),
                note: None,
                kind: DeviceKind::Other,
            },
            Device {
                target: "192.168.30.10".to_owned(),
                name: "Guest tablet".to_owned(),
                note: None,
                kind: DeviceKind::Tablet,
            },
        ];
        let resolver = ClientResolver::new(devices);
        assert_eq!(
            resolver.name_for("192.168.30.10".parse().unwrap()),
            Some("Guest tablet"),
            "the single address wins over the enclosing range"
        );
        assert_eq!(
            resolver.name_for("192.168.30.20".parse().unwrap()),
            Some("Guest Wi-Fi"),
            "other addresses fall to the range"
        );
        assert_eq!(resolver.name_for("10.0.0.1".parse().unwrap()), None);
    }

    #[test]
    fn longest_prefix_wins_among_ranges() {
        let devices = vec![
            Device {
                target: "10.0.0.0/8".to_owned(),
                name: "Whole LAN".to_owned(),
                note: None,
                kind: DeviceKind::Other,
            },
            Device {
                target: "10.1.2.0/24".to_owned(),
                name: "Office".to_owned(),
                note: None,
                kind: DeviceKind::Other,
            },
        ];
        let resolver = ClientResolver::new(devices);
        assert_eq!(
            resolver.name_for("10.1.2.5".parse().unwrap()),
            Some("Office"),
            "the more specific /24 wins over the /8"
        );
        assert_eq!(
            resolver.name_for("10.9.9.9".parse().unwrap()),
            Some("Whole LAN")
        );
    }

    #[test]
    fn device_name_validation() {
        assert!(valid_device_name("Living room TV"));
        assert!(!valid_device_name("   "));
        assert!(!valid_device_name(""));
        assert!(!valid_device_name(&"x".repeat(65)));
        assert!(valid_device_name(&"x".repeat(64)));
    }

    #[test]
    fn device_kind_serialises_lowercase_and_defaults_to_other() {
        assert_eq!(DeviceKind::default(), DeviceKind::Other);
        assert_eq!(serde_json::to_string(&DeviceKind::Tv).unwrap(), "\"tv\"");
        assert_eq!(
            serde_json::from_str::<DeviceKind>("\"phone\"").unwrap(),
            DeviceKind::Phone
        );
    }
}
