// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Named clients (devices): a label and a kind for a client address, so the log
//! and statistics can show device names instead of bare IPs.
//!
//! A device matches a single address (`192.168.20.10`) or a CIDR range
//! (`192.168.30.0/24`). On lookup, an exact single-address entry wins over any
//! range, and among ranges the longest prefix wins. Naming is presentation-only
//! and never on the DNS hot path: the web layer builds a [`ClientResolver`] from a
//! snapshot when it renders a view. [`ClientStore`] (in `store`) persists the
//! devices to a JSON file.

mod store;

pub use store::{ClientError, ClientStore};

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// The kind of device a name stands for, shown as an icon. Serialised lowercase.
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
    /// Anything that does not fit; the default when unset.
    #[default]
    Other,
}

/// One named client: the address or range it matches, a name, an optional note
/// and a kind. camelCase for the UI; `target` is the normalised text (a canonical
/// IP or a masked CIDR such as `192.168.30.0/24`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// The client address or CIDR range this entry names, already normalised.
    pub target: String,
    /// The display name shown in place of the address.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub kind: DeviceKind,
    /// The group this device belongs to, if any. A device inherits its group's
    /// policy delta on top of the global default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// This device's own filtering delta, on top of its group and the global default.
    #[serde(default, skip_serializing_if = "PolicyDelta::is_empty")]
    pub policy: PolicyDelta,
}

/// The filtering delta a scope — a group or a device — layers on what it inherits
/// (global → group → device). The policy coordinator folds these into one resolved
/// policy per device at rebuild time, so the DNS hot path never walks the
/// inheritance. List references are opaque list-object ids (feed subscriptions or
/// the built-in `ads`/`malware`), validated by the web layer, not here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PolicyDelta {
    /// List-object ids this scope switches on in addition to what it inherits.
    pub lists_added: Vec<String>,
    /// Inherited list-object ids this scope switches off for itself.
    pub lists_disabled: Vec<String>,
    /// Single domains this scope always allows; an allow wins over every block.
    pub allow: Vec<String>,
    /// Single domains this scope always blocks.
    pub block: Vec<String>,
}

impl PolicyDelta {
    /// Whether this delta changes nothing.
    pub fn is_empty(&self) -> bool {
        self.lists_added.is_empty()
            && self.lists_disabled.is_empty()
            && self.allow.is_empty()
            && self.block.is_empty()
    }
}

/// A named group of devices with its own filtering delta. Devices point at a
/// group by its stable `id`; the group's delta applies to every device in it,
/// between the global default and the device's own delta.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Group {
    /// Stable id, referenced by a device's `group` field and kept across renames.
    pub id: String,
    /// Display name shown in the dashboard.
    pub name: String,
    /// The group's filtering delta, layered on the global default.
    #[serde(default, skip_serializing_if = "PolicyDelta::is_empty")]
    pub policy: PolicyDelta,
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
        // Store the masked network address so matching is a plain compare.
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

/// Whether `name` is usable: non-empty after trimming and at most 64 characters.
pub fn valid_device_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && trimmed.chars().count() <= 64
}

/// Resolves a client address to the device that names it: exact single addresses
/// win over ranges, and the longest prefix wins among ranges. Built from a
/// snapshot of the stored devices when a view is rendered.
pub struct ClientResolver {
    devices: Vec<Device>,
    /// Index of every single-address device, address → position in `devices`.
    by_ip: HashMap<IpAddr, usize>,
    /// Every range device as `(matcher, position)`, sorted by descending prefix
    /// length so the first containing match is the most specific one.
    nets: Vec<(ClientMatch, usize)>,
}

impl ClientResolver {
    /// Build a resolver from the stored devices; entries whose target no longer
    /// parses are skipped.
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
                ..Default::default()
            },
            Device {
                target: "192.168.30.10".to_owned(),
                name: "Guest tablet".to_owned(),
                note: None,
                kind: DeviceKind::Tablet,
                ..Default::default()
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
                ..Default::default()
            },
            Device {
                target: "10.1.2.0/24".to_owned(),
                name: "Office".to_owned(),
                note: None,
                kind: DeviceKind::Other,
                ..Default::default()
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
