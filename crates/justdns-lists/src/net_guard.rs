// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A small SSRF guard for server-side outbound HTTP (list feeds, webhooks).
//!
//! Any URL the operator can enter is fetched by the node itself, so a hostile or
//! mistyped URL could aim the request at the cloud metadata endpoint
//! (`169.254.169.254`) or another never-legitimate address. This refuses those
//! while deliberately still allowing loopback and private LAN — a self-hosted
//! blocklist mirror or a local ntfy/Gotify on the operator's own network is a
//! normal setup. One policy, shared by every outbound-fetch path, so the node
//! never guards one surface and leaves another open.

use std::net::{IpAddr, ToSocketAddrs};

/// Whether `url`'s host is a destination the node must never fetch: the
/// link-local range (which carries the `169.254.169.254` cloud metadata endpoint
/// and IPv6 `fe80::/10`), the unspecified address, or the IPv4 broadcast address.
/// A hostname is resolved and every address it maps to is checked, so a name that
/// points at the metadata endpoint is caught too. A host that cannot be resolved
/// is allowed through, so the real request fails with its own error rather than
/// being silently swallowed here. Loopback and private ranges are allowed on
/// purpose.
pub fn host_is_disallowed(url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_disallowed_ip(ip);
    }
    match (host.as_str(), 0u16).to_socket_addrs() {
        Ok(addrs) => addrs.map(|addr| addr.ip()).any(is_disallowed_ip),
        Err(_) => false,
    }
}

/// Whether an address is a never-legitimate outbound destination: link-local
/// (includes the cloud metadata endpoint and IPv6 `fe80::/10`), unspecified, or
/// the IPv4 broadcast address. Loopback and private ranges are intentionally
/// allowed.
fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast(),
        IpAddr::V6(v6) => {
            // A dual-stack resolver (or a crafted literal) can present the metadata
            // endpoint as an IPv4-mapped address — `::ffff:169.254.169.254`. Unwrap
            // it and apply the IPv4 rules, so the block cannot be slipped past by
            // dressing an IPv4 address up as IPv6.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_disallowed_ip(IpAddr::V4(v4));
            }
            v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Extract the host from a URL without a URL-parser dependency: drop the scheme,
/// stop at the first `/`, `?` or `#`, drop any `user@`, unwrap an `[IPv6]`
/// literal, and strip a trailing `:port`. Returns `None` when no host is present.
fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(rest) = authority.strip_prefix('[') {
        return rest
            .split(']')
            .next()
            .filter(|h| !h.is_empty())
            .map(str::to_owned);
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    (!host.is_empty()).then(|| host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_and_link_local_are_disallowed() {
        assert!(host_is_disallowed("http://169.254.169.254/latest/meta-data/"));
        assert!(host_is_disallowed("http://169.254.1.1"));
        assert!(host_is_disallowed("http://0.0.0.0/x"));
        assert!(host_is_disallowed("http://[::ffff:169.254.169.254]/x"));
        assert!(host_is_disallowed("http://[fe80::1]/x"));
    }

    #[test]
    fn loopback_and_private_and_public_literals_are_allowed() {
        // Loopback and private LAN are allowed on purpose (self-hosted mirrors).
        assert!(!host_is_disallowed("http://127.0.0.1:8080/list.txt"));
        assert!(!host_is_disallowed("http://192.168.1.10/list.txt"));
        assert!(!host_is_disallowed("http://10.0.0.5/list.txt"));
        assert!(!host_is_disallowed("https://203.0.113.7/list.txt"));
    }

    #[test]
    fn a_url_with_no_host_is_allowed_to_fail_later() {
        assert!(!host_is_disallowed("not-a-url"));
    }
}
