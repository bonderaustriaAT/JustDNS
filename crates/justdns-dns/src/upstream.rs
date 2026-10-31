// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Upstream configuration: which resolvers queries are forwarded to, over which
//! transport, and how one is chosen per query.
//!
//! This module is pure configuration and selection logic. The network client
//! that actually talks to an upstream lives separately, so the selection rules
//! can be tested without touching the network.

use std::net::IpAddr;

use hickory_proto::rr::Name;

/// How a single upstream is reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    /// Plain DNS over UDP/TCP.
    Plain,
    /// DNS-over-TLS. `server_name` is the certificate name presented over the
    /// TLS connection, which differs from the IP address a client dials.
    Dot { server_name: String },
    /// DNS-over-HTTPS. `url` is the full query endpoint.
    Doh { url: String },
}

/// A single resolver to forward to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    pub address: IpAddr,
    pub port: u16,
    pub transport: Transport,
}

impl Upstream {
    pub fn plain(address: IpAddr) -> Self {
        Self {
            address,
            port: 53,
            transport: Transport::Plain,
        }
    }

    pub fn dot(address: IpAddr, server_name: impl Into<String>) -> Self {
        Self {
            address,
            port: 853,
            transport: Transport::Dot {
                server_name: server_name.into(),
            },
        }
    }

    pub fn doh(address: IpAddr, url: impl Into<String>) -> Self {
        Self {
            address,
            port: 443,
            transport: Transport::Doh { url: url.into() },
        }
    }
}

/// How an upstream is picked from the configured pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpstreamMode {
    /// Query every upstream at once, take the first answer.
    #[default]
    Racing,
    /// Try upstreams in order, moving on only when one fails.
    Fallback,
    /// Spread queries across upstreams to share load.
    LoadBalanced,
    /// Route by domain rule, falling back to the pool when no rule matches.
    RuleBased,
}

/// Routes a matching domain (and its subdomains) to a specific upstream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainRule {
    pub suffix: Name,
    pub upstream: Upstream,
}

/// The full upstream configuration: the pool, the selection mode, and the
/// rules consulted in [`UpstreamMode::RuleBased`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamConfig {
    pub mode: UpstreamMode,
    pub upstreams: Vec<Upstream>,
    pub rules: Vec<DomainRule>,
}

impl UpstreamConfig {
    /// The shipped EU defaults: DNS4EU Protective and Quad9, both over DoT,
    /// raced against each other.
    pub fn eu_defaults() -> Self {
        Self {
            mode: UpstreamMode::Racing,
            upstreams: vec![
                Upstream::dot(IpAddr::from([86, 54, 11, 1]), "protective.joindns4.eu"),
                Upstream::dot(IpAddr::from([9, 9, 9, 9]), "dns.quad9.net"),
            ],
            rules: Vec::new(),
        }
    }

    /// The rule routing `name`, or `None` when none applies. The longest
    /// matching suffix wins, so `example.com` beats `com` for `a.example.com`.
    pub fn rule_for(&self, name: &Name) -> Option<&DomainRule> {
        self.rules
            .iter()
            .filter(|rule| rule.suffix.zone_of(name))
            .max_by_key(|rule| rule.suffix.num_labels())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(fqdn: &str) -> Name {
        Name::from_ascii(fqdn).unwrap()
    }

    #[test]
    fn mode_defaults_to_racing() {
        assert_eq!(UpstreamMode::default(), UpstreamMode::Racing);
    }

    #[test]
    fn eu_defaults_are_dns4eu_and_quad9_over_dot() {
        let config = UpstreamConfig::eu_defaults();

        assert_eq!(config.mode, UpstreamMode::Racing);
        assert_eq!(config.upstreams.len(), 2);

        assert_eq!(config.upstreams[0].address, IpAddr::from([86, 54, 11, 1]));
        assert_eq!(config.upstreams[0].port, 853);
        assert_eq!(
            config.upstreams[0].transport,
            Transport::Dot {
                server_name: "protective.joindns4.eu".to_owned()
            }
        );

        assert_eq!(config.upstreams[1].address, IpAddr::from([9, 9, 9, 9]));
        assert_eq!(
            config.upstreams[1].transport,
            Transport::Dot {
                server_name: "dns.quad9.net".to_owned()
            }
        );
    }

    #[test]
    fn default_ports_match_transport() {
        assert_eq!(Upstream::plain(IpAddr::from([1, 1, 1, 1])).port, 53);
        assert_eq!(
            Upstream::dot(IpAddr::from([1, 1, 1, 1]), "one.one").port,
            853
        );
        assert_eq!(
            Upstream::doh(IpAddr::from([1, 1, 1, 1]), "https://one.one/dns-query").port,
            443
        );
    }

    fn rule_config() -> UpstreamConfig {
        UpstreamConfig {
            mode: UpstreamMode::RuleBased,
            upstreams: vec![Upstream::plain(IpAddr::from([9, 9, 9, 9]))],
            rules: vec![
                DomainRule {
                    suffix: name("local."),
                    upstream: Upstream::plain(IpAddr::from([192, 168, 1, 1])),
                },
                DomainRule {
                    suffix: name("example.com."),
                    upstream: Upstream::plain(IpAddr::from([10, 0, 0, 1])),
                },
                DomainRule {
                    suffix: name("com."),
                    upstream: Upstream::plain(IpAddr::from([10, 0, 0, 2])),
                },
            ],
        }
    }

    #[test]
    fn rule_matches_domain_and_subdomains() {
        let config = rule_config();

        assert_eq!(
            config
                .rule_for(&name("nas.local."))
                .unwrap()
                .upstream
                .address,
            IpAddr::from([192, 168, 1, 1])
        );
        assert_eq!(
            config.rule_for(&name("local.")).unwrap().upstream.address,
            IpAddr::from([192, 168, 1, 1])
        );
    }

    #[test]
    fn longest_matching_suffix_wins() {
        let config = rule_config();

        assert_eq!(
            config
                .rule_for(&name("a.example.com."))
                .unwrap()
                .upstream
                .address,
            IpAddr::from([10, 0, 0, 1])
        );
    }

    #[test]
    fn unmatched_domain_has_no_rule() {
        let config = rule_config();

        assert!(config.rule_for(&name("example.org.")).is_none());
    }

    #[test]
    fn rule_matching_ignores_case() {
        let config = rule_config();

        assert_eq!(
            config
                .rule_for(&name("NAS.LOCAL."))
                .unwrap()
                .upstream
                .address,
            IpAddr::from([192, 168, 1, 1])
        );
    }
}
