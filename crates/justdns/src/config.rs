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
/// The at-rest encryption key. Kept outside the data directory on purpose (its
/// own volume in the container), so a copied data volume does not also hand over
/// the key. The image sets this to `/keys/master.key`.
const DEFAULT_SECRET_KEY_PATH: &str = "master.key";
const DEFAULT_LISTS_PATH: &str = "lists.json";
const DEFAULT_LISTS_CACHE: &str = "lists-cache";
const DEFAULT_REWRITES_PATH: &str = "rewrites.json";
const DEFAULT_CLIENTS_PATH: &str = "clients.json";
const DEFAULT_SCHEDULES_PATH: &str = "schedules.json";
const DEFAULT_AUDIT_PATH: &str = "audit.log";
const DEFAULT_REPORTS_PATH: &str = "reports.json";
const DEFAULT_ACCESS_PATH: &str = "access-requests.json";
const DEFAULT_DOT_LISTEN: &str = "0.0.0.0:853";
const DEFAULT_DOH_LISTEN: &str = "0.0.0.0:8443";
const DEFAULT_TLS_DIR: &str = "tls";
const DEFAULT_CERTS_DIR: &str = "certs";
const DEFAULT_BLOCKPAGE_HTTP_LISTEN: &str = "0.0.0.0:80";
const DEFAULT_BLOCKPAGE_HTTPS_LISTEN: &str = "0.0.0.0:443";

#[derive(Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub web_listen: SocketAddr,
    pub ipv6_enabled: bool,
    /// Whether to validate upstream answers with DNSSEC at startup. Off by
    /// default; signed answers that fail validation (Bogus) are refused with
    /// SERVFAIL. Seeded from `JUSTDNS_DNSSEC`.
    pub dnssec_enabled: bool,
    pub cache_capacity: u64,
    pub db_path: PathBuf,
    /// Where the editable runtime configuration is stored. Env values seed it
    /// on first run; afterwards this file is the source of truth.
    pub config_path: PathBuf,
    /// Where the account list (admin password hash) is stored.
    pub auth_path: PathBuf,
    /// Where the at-rest master key lives. A separate volume from the data, so a
    /// stolen data volume cannot also decrypt the secret files.
    pub secret_key_path: PathBuf,
    /// Where the user's allow/block lists and subscriptions are stored.
    pub lists_path: PathBuf,
    /// Directory where fetched external feeds are cached.
    pub lists_cache_dir: PathBuf,
    /// Where the local DNS rewrites (name → IP) are stored.
    pub rewrites_path: PathBuf,
    /// Where the named clients (devices) are stored.
    pub clients_path: PathBuf,
    /// Where the schedules (recurring time windows) are stored.
    pub schedules_path: PathBuf,
    /// Where the append-only security audit log is stored.
    pub audit_path: PathBuf,
    /// Where generated summary reports are stored (a single JSON file on `/data`).
    pub reports_path: PathBuf,
    /// Where the block page's pending "ask to unblock this" queue is stored.
    pub access_path: PathBuf,
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
    /// Directory holding the single-cert era's server TLS material. Kept for the
    /// one-time migration into the certificate store; lives on `/data`.
    pub tls_dir: PathBuf,
    /// Directory holding the certificate store (one sub-directory per stored
    /// certificate). Lives on `/data` so it survives image updates.
    pub certs_dir: PathBuf,
    /// Where the custom block page serves plain HTTP when enabled.
    pub blockpage_http_listen: SocketAddr,
    /// Where the custom block page serves HTTPS when enabled.
    pub blockpage_https_listen: SocketAddr,
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

        let dnssec_enabled = parse_var("JUSTDNS_DNSSEC", false, |value| Ok(is_truthy(value)))?;

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

        let secret_key_path = PathBuf::from(
            read_var("JUSTDNS_SECRET_KEY_PATH")?
                .unwrap_or_else(|| DEFAULT_SECRET_KEY_PATH.to_owned()),
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

        let schedules_path = PathBuf::from(
            read_var("JUSTDNS_SCHEDULES_PATH")?
                .unwrap_or_else(|| DEFAULT_SCHEDULES_PATH.to_owned()),
        );

        let audit_path = PathBuf::from(
            read_var("JUSTDNS_AUDIT_PATH")?.unwrap_or_else(|| DEFAULT_AUDIT_PATH.to_owned()),
        );

        let reports_path = PathBuf::from(
            read_var("JUSTDNS_REPORTS_PATH")?.unwrap_or_else(|| DEFAULT_REPORTS_PATH.to_owned()),
        );

        let access_path = PathBuf::from(
            read_var("JUSTDNS_ACCESS_PATH")?.unwrap_or_else(|| DEFAULT_ACCESS_PATH.to_owned()),
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

        let certs_dir = PathBuf::from(
            read_var("JUSTDNS_CERTS_DIR")?.unwrap_or_else(|| DEFAULT_CERTS_DIR.to_owned()),
        );

        let blockpage_http_listen = parse_var(
            "JUSTDNS_BLOCKPAGE_HTTP_LISTEN",
            DEFAULT_BLOCKPAGE_HTTP_LISTEN.parse().unwrap(),
            |value| {
                value.parse().map_err(|source| ConfigError::BadAddress {
                    var: "JUSTDNS_BLOCKPAGE_HTTP_LISTEN",
                    source,
                })
            },
        )?;

        let blockpage_https_listen = parse_var(
            "JUSTDNS_BLOCKPAGE_HTTPS_LISTEN",
            DEFAULT_BLOCKPAGE_HTTPS_LISTEN.parse().unwrap(),
            |value| {
                value.parse().map_err(|source| ConfigError::BadAddress {
                    var: "JUSTDNS_BLOCKPAGE_HTTPS_LISTEN",
                    source,
                })
            },
        )?;

        Ok(Self {
            listen,
            web_listen,
            ipv6_enabled,
            dnssec_enabled,
            cache_capacity,
            db_path,
            config_path,
            auth_path,
            secret_key_path,
            lists_path,
            lists_cache_dir,
            rewrites_path,
            clients_path,
            schedules_path,
            audit_path,
            reports_path,
            access_path,
            upstreams,
            upstreams_overridden,
            dot_listen,
            doh_listen,
            tls_dir,
            certs_dir,
            blockpage_http_listen,
            blockpage_https_listen,
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
