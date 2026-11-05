// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runtime configuration, read from the environment.
//!
//! The container is configured with environment variables, so startup reads
//! them here and nowhere else. Everything has a working default, so an empty
//! environment yields a usable forwarder on the EU resolvers.

use std::env::{self, VarError};
use std::net::{IpAddr, SocketAddr};
use std::num::ParseIntError;
use std::path::PathBuf;

use justdns_dns::upstream::{Upstream, UpstreamConfig, UpstreamMode};

const DEFAULT_LISTEN: &str = "0.0.0.0:53";
const DEFAULT_WEB_LISTEN: &str = "0.0.0.0:8080";
const DEFAULT_CACHE_CAPACITY: u64 = 10_000;
const DEFAULT_DB_PATH: &str = "queries.duckdb";
const DEFAULT_CONFIG_PATH: &str = "config.json";
const DEFAULT_AUTH_PATH: &str = "auth.json";
const DEFAULT_LISTS_PATH: &str = "lists.json";
const DEFAULT_LISTS_CACHE: &str = "lists-cache";
const DEFAULT_REWRITES_PATH: &str = "rewrites.json";
const DEFAULT_CLIENTS_PATH: &str = "clients.json";
const DEFAULT_DOT_LISTEN: &str = "0.0.0.0:853";
const DEFAULT_DOH_LISTEN: &str = "0.0.0.0:8443";
const DEFAULT_TLS_DIR: &str = "tls";

#[derive(Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub web_listen: SocketAddr,
    pub ipv6_enabled: bool,
    pub cache_capacity: u64,
    pub db_path: PathBuf,
    /// Where the editable runtime configuration is stored. Env values seed it
    /// on first run; afterwards this file is the source of truth.
    pub config_path: PathBuf,
    /// Where the account list (admin password hash) is stored.
    pub auth_path: PathBuf,
    /// Where the user's allow/block lists and subscriptions are stored.
    pub lists_path: PathBuf,
    /// Directory where fetched external feeds are cached.
    pub lists_cache_dir: PathBuf,
    /// Where the local DNS rewrites (name → IP) are stored.
    pub rewrites_path: PathBuf,
    /// Where the named clients (devices) are stored.
    pub clients_path: PathBuf,
    pub upstreams: UpstreamConfig,
    /// Whether `JUSTDNS_UPSTREAMS` was set, pinning the resolver pool to those
    /// addresses. When true the speed profile only tunes the cache and leaves
    /// the explicitly configured upstreams alone (used by local/test setups);
    /// when false the profile also chooses which resolvers are used.
    pub upstreams_overridden: bool,
    /// Where DNS-over-TLS (RFC 7858) listens when enabled in the settings.
    pub dot_listen: SocketAddr,
    /// Where DNS-over-HTTPS (RFC 8484) listens when enabled in the settings.
    pub doh_listen: SocketAddr,
    /// Directory holding the server TLS certificate and key for the encrypted
    /// client-DNS endpoints. Lives on `/data` so it survives image updates.
    pub tls_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{var} is not valid UTF-8")]
    NotUnicode { var: &'static str },
    #[error("{var}: {source}")]
    BadAddress {
        var: &'static str,
        source: std::net::AddrParseError,
    },
    #[error("{var}: {source}")]
    BadNumber {
        var: &'static str,
        source: ParseIntError,
    },
    #[error("JUSTDNS_UPSTREAMS: no usable upstream address in {0:?}")]
    NoUpstreams(String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let listen = parse_var("JUSTDNS_LISTEN", DEFAULT_LISTEN.parse().unwrap(), |value| {
            value.parse().map_err(|source| ConfigError::BadAddress {
                var: "JUSTDNS_LISTEN",
                source,
            })
        })?;

        let web_listen = parse_var(
            "JUSTDNS_WEB_LISTEN",
            DEFAULT_WEB_LISTEN.parse().unwrap(),
            |value| {
                value.parse().map_err(|source| ConfigError::BadAddress {
                    var: "JUSTDNS_WEB_LISTEN",
                    source,
                })
            },
        )?;

        let ipv6_enabled = parse_var("JUSTDNS_IPV6", false, |value| Ok(is_truthy(value)))?;

        let cache_capacity =
            parse_var("JUSTDNS_CACHE_CAPACITY", DEFAULT_CACHE_CAPACITY, |value| {
                value.parse().map_err(|source| ConfigError::BadNumber {
                    var: "JUSTDNS_CACHE_CAPACITY",
                    source,
                })
            })?;

        let db_path = PathBuf::from(
            read_var("JUSTDNS_DB_PATH")?.unwrap_or_else(|| DEFAULT_DB_PATH.to_owned()),
        );

        let config_path = PathBuf::from(
            read_var("JUSTDNS_CONFIG_PATH")?.unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_owned()),
        );

        let auth_path = PathBuf::from(
            read_var("JUSTDNS_AUTH_PATH")?.unwrap_or_else(|| DEFAULT_AUTH_PATH.to_owned()),
        );

        let lists_path = PathBuf::from(
            read_var("JUSTDNS_LISTS_PATH")?.unwrap_or_else(|| DEFAULT_LISTS_PATH.to_owned()),
        );

        let lists_cache_dir = PathBuf::from(
            read_var("JUSTDNS_LISTS_CACHE")?.unwrap_or_else(|| DEFAULT_LISTS_CACHE.to_owned()),
        );

        let rewrites_path = PathBuf::from(
            read_var("JUSTDNS_REWRITES_PATH")?.unwrap_or_else(|| DEFAULT_REWRITES_PATH.to_owned()),
        );

        let clients_path = PathBuf::from(
            read_var("JUSTDNS_CLIENTS_PATH")?.unwrap_or_else(|| DEFAULT_CLIENTS_PATH.to_owned()),
        );

        let (upstreams, upstreams_overridden) = match read_var("JUSTDNS_UPSTREAMS")? {
            Some(raw) => (parse_upstreams(&raw)?, true),
            None => (UpstreamConfig::eu_defaults(), false),
        };

        let dot_listen = parse_var(
            "JUSTDNS_DOT_LISTEN",
            DEFAULT_DOT_LISTEN.parse().unwrap(),
            |value| {
                value.parse().map_err(|source| ConfigError::BadAddress {
                    var: "JUSTDNS_DOT_LISTEN",
                    source,
                })
            },
        )?;

        let doh_listen = parse_var(
            "JUSTDNS_DOH_LISTEN",
            DEFAULT_DOH_LISTEN.parse().unwrap(),
            |value| {
                value.parse().map_err(|source| ConfigError::BadAddress {
                    var: "JUSTDNS_DOH_LISTEN",
                    source,
                })
            },
        )?;

        let tls_dir = PathBuf::from(
            read_var("JUSTDNS_TLS_DIR")?.unwrap_or_else(|| DEFAULT_TLS_DIR.to_owned()),
        );

        Ok(Self {
            listen,
            web_listen,
            ipv6_enabled,
            cache_capacity,
            db_path,
            config_path,
            auth_path,
            lists_path,
            lists_cache_dir,
            rewrites_path,
            clients_path,
            upstreams,
            upstreams_overridden,
            dot_listen,
            doh_listen,
            tls_dir,
        })
    }
}

/// Parse a comma-separated list of plain-DNS upstream addresses, each
/// `ip` or `ip:port` (port defaults to 53). Used to point the forwarder at a
/// local or test resolver instead of the encrypted EU defaults.
fn parse_upstreams(raw: &str) -> Result<UpstreamConfig, ConfigError> {
    let mut upstreams = Vec::new();
    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let address = parse_plain_address(entry).ok_or_else(|| ConfigError::BadAddress {
            var: "JUSTDNS_UPSTREAMS",
            source: entry.parse::<IpAddr>().unwrap_err(),
        })?;
        upstreams.push(address);
    }

    if upstreams.is_empty() {
        return Err(ConfigError::NoUpstreams(raw.to_owned()));
    }

    Ok(UpstreamConfig {
        mode: UpstreamMode::Racing,
        upstreams,
        rules: Vec::new(),
    })
}

fn parse_plain_address(entry: &str) -> Option<Upstream> {
    if let Ok(socket) = entry.parse::<SocketAddr>() {
        return Some(Upstream {
            address: socket.ip(),
            port: socket.port(),
            transport: justdns_dns::upstream::Transport::Plain,
        });
    }
    entry.parse::<IpAddr>().ok().map(Upstream::plain)
}

fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn read_var(var: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(var) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(ConfigError::NotUnicode { var }),
    }
}

fn parse_var<T>(
    var: &'static str,
    default: T,
    parse: impl FnOnce(&str) -> Result<T, ConfigError>,
) -> Result<T, ConfigError> {
    match read_var(var)? {
        Some(value) => parse(&value),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_address_defaults_port_to_53() {
        let upstream = parse_plain_address("9.9.9.9").unwrap();
        assert_eq!(upstream.port, 53);
        assert_eq!(upstream.address, IpAddr::from([9, 9, 9, 9]));
    }

    #[test]
    fn plain_address_keeps_explicit_port() {
        let upstream = parse_plain_address("127.0.0.1:5300").unwrap();
        assert_eq!(upstream.port, 5300);
    }

    #[test]
    fn upstream_list_is_parsed_and_trimmed() {
        let config = parse_upstreams(" 9.9.9.9 , 1.1.1.1:53 ").unwrap();
        assert_eq!(config.upstreams.len(), 2);
    }

    #[test]
    fn empty_upstream_list_is_an_error() {
        assert!(matches!(
            parse_upstreams("  ,  "),
            Err(ConfigError::NoUpstreams(_))
        ));
    }

    #[test]
    fn truthy_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy("on"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy(""));
    }
}
