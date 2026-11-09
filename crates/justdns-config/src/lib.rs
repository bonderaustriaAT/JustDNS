// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runtime configuration that the web UI can edit and that survives restarts.
//!
//! Startup still reads the environment (see the binary's `config` module), but
//! those values are only the *seed*: on first run they are written to a JSON
//! file, and from then on that file is the source of truth. The dashboard reads
//! and writes it through the API, so settings no longer live only in a single
//! browser's local storage.
//!
//! Most settings are consumed where their feature lives. IPv6 is wired through
//! live: the store publishes it as a shared flag the DNS pipeline reads per
//! query, so toggling it in the UI takes effect without a restart.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// A ready-made speed/privacy profile. A single switch the user understands
/// instead of a scatter of low-level knobs: each profile chooses which
/// resolvers are used and how eagerly answers are cached and refreshed. The
/// resolver set and cache windows a profile stands for are applied by the
/// binary (which owns the DNS engine); this crate only carries the choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedProfile {
    /// Privacy first: privacy-focused resolvers, nothing served past its TTL,
    /// minimal background traffic.
    Privacy,
    /// The balanced default: a mix of privacy and speed, with a short
    /// serve-stale window and background refresh of popular names.
    #[default]
    Balanced,
    /// Speed first: the fastest public resolvers and an aggressive cache that
    /// keeps popular names warm the longest.
    Speed,
}

/// What JustDNS answers when a name is blocked. `NullIp` hands back the
/// unroutable null address (`0.0.0.0` for A, `::` for AAAA), so a client's
/// connection to a blocked host fails at once and apps move on — this is the
/// default, and what AdGuard Home and Pi-hole do out of the box. `NxDomain`
/// says the name does not exist: tidier in a log, but some clients (notably
/// smart-TV and mobile video players) retry a failed lookup with backoff, which
/// can make playback stall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockResponse {
    /// Answer a blocked name with the null IP (`0.0.0.0` / `::`).
    #[default]
    NullIp,
    /// Answer that the blocked name does not exist (NXDOMAIN).
    NxDomain,
}

/// How a user-defined upstream is reached. Mirrors the DNS core's transport
/// kinds; kept here as its own type so the config crate stays a leaf.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomTransport {
    /// Plain DNS over UDP/TCP.
    #[default]
    Plain,
    /// DNS-over-TLS (needs a certificate name).
    Dot,
    /// DNS-over-HTTPS (needs a certificate name).
    Doh,
}

/// A single resolver the user entered by hand. Stored as typed, so a
/// half-finished entry survives editing; the binary validates each one when it
/// builds the live pool and simply skips any that does not parse.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomUpstream {
    /// The resolver's IP address, as typed.
    pub address: String,
    /// An explicit port, or `None` for the transport's default (53/853/443).
    pub port: Option<u16>,
    /// How to reach it.
    pub transport: CustomTransport,
    /// The TLS certificate name for DoT/DoH; ignored for plain.
    pub server_name: Option<String>,
}

/// How the resolver picks between the configured upstreams. Mirrors the DNS
/// core's `UpstreamMode`; kept here as its own type so the config crate stays a
/// leaf that the core and the web layer can both depend on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamMode {
    /// Ask every upstream at once and take the first answer.
    #[default]
    Racing,
    /// Try the upstreams in order, moving on only when one fails.
    Fallback,
    /// Spread queries across the upstreams in turn.
    Balanced,
    /// Pick the upstream by domain rules.
    Rules,
}

/// Where the certificate the clients should trust comes from, for the encrypted
/// client-DNS endpoints. Mirrors the DNS core's `tls::CertSource`; kept here as
/// its own type so the config crate stays a leaf.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CertSource {
    /// JustDNS mints and serves its own local CA + leaf certificate.
    #[default]
    SelfSigned,
    /// The operator uploaded their own certificate chain and key.
    Uploaded,
}

/// Settings for the encrypted client-DNS endpoints (DNS-over-TLS on 853 and
/// DNS-over-HTTPS on `/dns-query`). The certificate itself lives on the `/data`
/// volume, not here; this only carries whether each endpoint is on, the hostname
/// the certificate is minted for, and where the certificate comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EncryptedDns {
    /// Serve DNS-over-TLS (port 853) to the local network.
    pub dot_enabled: bool,
    /// Serve DNS-over-HTTPS (`/dns-query`) to the local network.
    pub doh_enabled: bool,
    /// The hostname the devices dial and that the certificate is minted for,
    /// e.g. `dns.fritz.box`. Used as the certificate's subject alternative name.
    pub hostname: String,
    /// Whether the certificate is self-signed (the default) or uploaded. Kept so
    /// a config from before the certificate store still loads; the selection is
    /// now carried by `dot_cert_id`/`doh_cert_id`.
    pub cert_source: CertSource,
    /// Which stored certificate DoT serves, by its id in the certificate store.
    /// Empty selects the store's default (migrated) certificate.
    #[serde(default)]
    pub dot_cert_id: Option<String>,
    /// Which stored certificate DoH serves. Empty selects the store default.
    #[serde(default)]
    pub doh_cert_id: Option<String>,
}

impl Default for EncryptedDns {
    fn default() -> Self {
        Self {
            dot_enabled: false,
            doh_enabled: false,
            hostname: String::new(),
            cert_source: CertSource::SelfSigned,
            dot_cert_id: None,
            doh_cert_id: None,
        }
    }
}

/// The custom block page: an optional web server JustDNS runs so a blocked name
/// resolves to a short explanation page instead of a dead connection. Off by
/// default. When on, a blocked `A`/`AAAA` query is answered with the server IP
/// below (instead of the null IP / NXDOMAIN), and the server answers on HTTP and
/// HTTPS — the HTTPS listener presents the certificate `cert_id` selects from the
/// store. Part of [`Settings`], so it syncs across an HA cluster as one unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BlockPage {
    /// Serve the block page (and point blocked names at it).
    pub enabled: bool,
    /// The IPv4 the block-page server is reached at on the LAN (what a blocked
    /// `A` query is answered with). Empty leaves `A` blocks at the null IP.
    pub server_ipv4: String,
    /// The IPv6 the block-page server is reached at. Empty leaves `AAAA` blocks
    /// at the null IP.
    pub server_ipv6: String,
    /// Which stored certificate the HTTPS block page presents, by store id.
    /// Empty selects the store default.
    pub cert_id: Option<String>,
    /// Run the block page behind a standalone reverse proxy (NGINX Proxy Manager,
    /// Traefik, …) that terminates TLS and forwards to JustDNS over plain HTTP.
    /// When on, JustDNS stops managing a certificate of its own and trusts the
    /// proxy's `X-Forwarded-Host` header to tell which hostname was requested, so
    /// the landing page is served without a redirect loop. Off by default — left
    /// off, the header is ignored so a client cannot spoof it.
    pub behind_reverse_proxy: bool,
    /// An optional redirect target. When non-empty, the block-page server answers
    /// every request with an HTTP redirect to this URL instead of rendering the
    /// page itself — so the explanation can be hosted at a real hostname with a
    /// valid certificate (via a DNS rewrite onto JustDNS, or behind a reverse
    /// proxy) and no certificate has to be managed here. Empty serves the built-in
    /// page as before.
    pub redirect_url: String,
    /// The heading shown on the block page.
    pub title: String,
    /// The explanatory line shown under the heading.
    pub message: String,
}

impl Default for BlockPage {
    fn default() -> Self {
        Self {
            enabled: false,
            server_ipv4: String::new(),
            server_ipv6: String::new(),
            cert_id: None,
            behind_reverse_proxy: false,
            redirect_url: String::new(),
            title: "Diese Seite ist gesperrt".to_owned(),
            message: "Der Zugriff auf diese Domain wurde durch JustDNS blockiert.".to_owned(),
        }
    }
}

/// Which shape a notification webhook payload takes, so the same URL field can
/// target the common self-hosted and chat services without per-service code in
/// the UI. `Generic` posts `{title, message}` JSON; `Ntfy` posts the message as
/// a plain-text body with the title in the `Title` header; `Slack` and `Discord`
/// post the one JSON field each expects; `Telegram` posts to the Bot API, taking
/// the bot token from the URL field and the chat id from `telegram_chat_id`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookFormat {
    /// `POST {"title": …, "message": …}` as JSON.
    #[default]
    Generic,
    /// ntfy.sh style: the message as a plain-text body, title in the `Title` header.
    Ntfy,
    /// Slack incoming webhook: `POST {"text": …}`.
    Slack,
    /// Discord webhook: `POST {"content": …}`.
    Discord,
    /// Telegram Bot API: `POST https://api.telegram.org/bot<token>/sendMessage`.
    Telegram,
}

/// Active notifications: when something noteworthy happens (a cluster node goes
/// unreachable, an update is available, a stored certificate is about to expire,
/// or a device suddenly makes an unusual number of blocked lookups), JustDNS
/// posts a short message to a webhook. Off by default. Part of [`Settings`], so
/// the webhook target syncs across an HA cluster as one unit — every node can
/// then send, and each message names the node it came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notifications {
    /// Send notifications at all.
    pub enabled: bool,
    /// The webhook target. For most formats this is the full URL to POST to; for
    /// `Telegram` it is the bot token (the chat id is set separately).
    pub webhook_url: String,
    /// Which payload shape to send, so the URL can target ntfy, Slack, Discord,
    /// Telegram or a generic receiver without extra configuration.
    pub webhook_format: WebhookFormat,
    /// The Telegram chat id to deliver to (only used when `webhook_format` is
    /// `Telegram`).
    pub telegram_chat_id: String,
    /// Alert when a cluster node becomes unreachable (and when it recovers).
    pub on_node_down: bool,
    /// Alert when a newer JustDNS release is available.
    pub on_update: bool,
    /// Alert when a stored certificate is within `cert_expiry_days` of expiring.
    pub on_cert_expiry: bool,
    /// Alert when a device makes an unusual number of blocked lookups in a short
    /// window (possible malware beaconing). A heuristic — off by default.
    pub on_device_spike: bool,
    /// How many days before a certificate's expiry to start warning.
    pub cert_expiry_days: u32,
    /// How many blocked lookups from one device within `spike_window_mins` count
    /// as a spike worth alerting on.
    pub spike_blocked_threshold: u32,
    /// The rolling window, in minutes, the spike threshold is measured over.
    pub spike_window_mins: u32,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: false,
            webhook_url: String::new(),
            webhook_format: WebhookFormat::Generic,
            telegram_chat_id: String::new(),
            on_node_down: true,
            on_update: true,
            on_cert_expiry: true,
            on_device_spike: false,
            cert_expiry_days: 14,
            spike_blocked_threshold: 100,
            spike_window_mins: 10,
        }
    }
}

/// When scheduled summary reports are generated. Both cadences are off by
/// default, so an existing install produces nothing until the admin turns one
/// on. The hour is the local hour-of-day (0–23) a report runs at, in the
/// instance's configured [`Settings::timezone`]; a report always covers the
/// previous complete period (yesterday, or the last seven days ending at the
/// most recent local midnight). Part of [`Settings`], so it syncs across an HA
/// cluster as one unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportSettings {
    /// Generate a daily summary.
    pub daily_enabled: bool,
    /// Local hour-of-day (0–23) the daily report runs at.
    pub daily_hour: u8,
    /// Generate a weekly summary.
    pub weekly_enabled: bool,
    /// Weekday the weekly report runs on, Monday = 1 … Sunday = 7 (ISO).
    pub weekly_weekday: u8,
    /// Local hour-of-day (0–23) the weekly report runs at.
    pub weekly_hour: u8,
}

impl Default for ReportSettings {
    fn default() -> Self {
        Self {
            daily_enabled: false,
            daily_hour: 6,
            weekly_enabled: false,
            weekly_weekday: 1,
            weekly_hour: 6,
        }
    }
}

/// The settings the dashboard shows and edits. Field names are serialised in
/// camelCase to match the UI's JSON directly. `#[serde(default)]` lets a file
/// written by an older build load even when newer fields are missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Block known advertising and tracking domains.
    pub block_ads_trackers: bool,
    /// Redirect big search engines to their safe-search addresses.
    pub safe_search: bool,
    /// Block known phishing and malware domains.
    pub block_malware: bool,
    /// How the resolver chooses between upstreams.
    pub upstream_mode: UpstreamMode,
    /// Talk to the upstreams over DNS-over-TLS rather than plaintext.
    pub encrypt_upstream: bool,
    /// The speed/privacy profile: which resolvers are used and how eagerly
    /// answers are cached. Wired live — a change takes effect without a restart.
    pub speed_profile: SpeedProfile,
    /// Use the user's own resolver list instead of the profile's resolvers.
    /// When on (and at least one entry is valid), `custom_upstreams` replaces the
    /// profile servers; the profile then only tunes the cache. Wired live.
    pub custom_upstreams_enabled: bool,
    /// The user's own resolvers, used when `custom_upstreams_enabled` is on.
    pub custom_upstreams: Vec<CustomUpstream>,
    /// Answer AAAA queries instead of returning empty NODATA. Wired live.
    pub ipv6: bool,
    /// Record handled queries for the log view.
    pub query_log: bool,
    /// How many days log entries are kept.
    pub log_retention_days: u32,
    /// A display name for this instance, shown in the header. Free text the user
    /// sets in the setup wizard; defaults to "JustDNS".
    pub instance_name: String,
    /// What a blocked name is answered with: the null IP (the default) or
    /// NXDOMAIN. Wired live — a change takes effect on the next query.
    pub block_response: BlockResponse,
    /// How many seconds since a client's last query still counts it as "active"
    /// on the devices view. Read when that view is rendered, so a change takes
    /// effect at once; defaults to five minutes.
    pub active_window_secs: u32,
    /// The encrypted client-DNS endpoints (DoT/DoH). A change to the enable flags
    /// or hostname restarts the affected listener; see the binary's supervisor.
    pub encrypted_dns: EncryptedDns,
    /// The IANA time zone (e.g. `Europe/Vienna`) schedules are evaluated in, so a
    /// window like 22:00–07:00 means local time across DST. An empty or unknown
    /// name falls back to UTC. Defaults to UTC. Wired live — a change is picked up
    /// on the next schedule tick.
    pub timezone: String,
    /// Require every account to use multi-factor authentication. When on, a user
    /// without MFA set up must enrol before they can finish signing in. An
    /// administrator can still require MFA for individual users even when this is
    /// off (see the per-user flag in the account store). Defaults to off so an
    /// existing single-admin install is unaffected until the admin turns it on.
    pub require_mfa: bool,
    /// The blockable services switched on by default (globally), by their machine
    /// id (e.g. `tiktok`). Each one blocks the domains of a well-known product
    /// from the compiled-in catalogue (see `justdns_filter::services`). A group or
    /// device can still turn a service on or off for itself. Empty by default, so
    /// nothing is service-blocked until the admin chooses.
    pub blocked_services: Vec<String>,
    /// Let the instance check GitHub for a newer release and surface it in the UI.
    /// On by default; an admin can turn it off (e.g. an air-gapped install) so the
    /// app never reaches out. Only the check is governed here — an update is never
    /// applied without an explicit click.
    pub update_checks: bool,
    /// When scheduled summary reports run. Both cadences off by default.
    #[serde(default)]
    pub reports: ReportSettings,
    /// The custom block page. Off by default.
    #[serde(default)]
    pub block_page: BlockPage,
    /// Active notifications (webhook alerts). Off by default.
    #[serde(default)]
    pub notifications: Notifications,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            block_ads_trackers: true,
            safe_search: false,
            block_malware: true,
            upstream_mode: UpstreamMode::Racing,
            encrypt_upstream: true,
            speed_profile: SpeedProfile::Balanced,
            custom_upstreams_enabled: false,
            custom_upstreams: Vec::new(),
            ipv6: false,
            query_log: true,
            log_retention_days: 7,
            instance_name: "JustDNS".to_owned(),
            block_response: BlockResponse::NullIp,
            active_window_secs: 300,
            encrypted_dns: EncryptedDns::default(),
            timezone: "UTC".to_owned(),
            require_mfa: false,
            blocked_services: Vec::new(),
            update_checks: true,
            reports: ReportSettings::default(),
            block_page: BlockPage::default(),
            notifications: Notifications::default(),
        }
    }
}

/// A callback run after every settings change, holding the live engine handles
/// the config crate itself knows nothing about (the DNS router and cache). The
/// binary registers one so a dashboard change reconfigures the engine at once.
type OnChange = Arc<dyn Fn(&Settings) + Send + Sync>;

/// Errors from loading or saving the configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("config parse ({path}): {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

struct Inner {
    path: PathBuf,
    settings: RwLock<Settings>,
    /// Live flags the DNS path reads per query, kept in step with the matching
    /// `settings` field on every load and save so a dashboard change takes
    /// effect without a restart.
    ipv6: Arc<AtomicBool>,
    block_ads_trackers: Arc<AtomicBool>,
    block_malware: Arc<AtomicBool>,
    safe_search: Arc<AtomicBool>,
    /// True when a blocked name is answered with the null IP, false for NXDOMAIN.
    /// The DNS pipeline reads it per query, so the mode flips without a restart.
    block_null_ip: Arc<AtomicBool>,
    /// True when answered queries are recorded to the query log, false to keep no
    /// record. The query observer reads it per query, so the switch takes effect
    /// without a restart.
    query_log: Arc<AtomicBool>,
    /// Run after each change to apply settings the engine owns (router, cache)
    /// that cannot be expressed as a simple atomic flag here.
    on_change: RwLock<Option<OnChange>>,
}

/// A cheap, cloneable handle to the persistent configuration.
#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<Inner>,
}

impl ConfigStore {
    /// Load the configuration from `path`. If the file is missing, `seed` (the
    /// values derived from the environment) is written as the starting point;
    /// a file that exists but cannot be parsed is reported as an error so a
    /// typo is not silently overwritten.
    pub fn load(path: impl Into<PathBuf>, seed: Settings) -> Result<Self, ConfigError> {
        let path = path.into();
        let settings = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| ConfigError::Parse {
                path: path.clone(),
                source,
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_atomic(&path, &seed)?;
                seed
            }
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };

        Ok(Self {
            inner: Arc::new(Inner {
                path,
                ipv6: Arc::new(AtomicBool::new(settings.ipv6)),
                block_ads_trackers: Arc::new(AtomicBool::new(settings.block_ads_trackers)),
                block_malware: Arc::new(AtomicBool::new(settings.block_malware)),
                safe_search: Arc::new(AtomicBool::new(settings.safe_search)),
                block_null_ip: Arc::new(AtomicBool::new(matches!(
                    settings.block_response,
                    BlockResponse::NullIp
                ))),
                query_log: Arc::new(AtomicBool::new(settings.query_log)),
                on_change: RwLock::new(None),
                settings: RwLock::new(settings),
            }),
        })
    }

    /// Register the callback run after every [`set`](Self::set), so the binary
    /// can reconfigure the live DNS engine (router, cache tuning) from the new
    /// settings. Replaces any previously registered callback.
    pub fn on_change(&self, callback: impl Fn(&Settings) + Send + Sync + 'static) {
        *self.inner.on_change.write().expect("config lock poisoned") = Some(Arc::new(callback));
    }

    /// A snapshot of the current settings.
    pub fn get(&self) -> Settings {
        self.inner
            .settings
            .read()
            .expect("config lock poisoned")
            .clone()
    }

    /// Replace the settings, publish the live IPv6 flag and persist to disk.
    /// The in-memory value is updated even if the write fails, so the running
    /// instance reflects the request; the error is returned so the caller can
    /// report that it was not saved durably.
    pub fn set(&self, settings: Settings) -> Result<(), ConfigError> {
        self.inner.ipv6.store(settings.ipv6, Ordering::Relaxed);
        self.inner
            .block_ads_trackers
            .store(settings.block_ads_trackers, Ordering::Relaxed);
        self.inner
            .block_malware
            .store(settings.block_malware, Ordering::Relaxed);
        self.inner
            .safe_search
            .store(settings.safe_search, Ordering::Relaxed);
        self.inner.block_null_ip.store(
            matches!(settings.block_response, BlockResponse::NullIp),
            Ordering::Relaxed,
        );
        self.inner
            .query_log
            .store(settings.query_log, Ordering::Relaxed);
        *self.inner.settings.write().expect("config lock poisoned") = settings.clone();
        // Apply engine-owned settings (router, cache tuning) live. Taken as a
        // clone so the lock is not held across the callback. Done even if the
        // disk write below fails, so the running instance reflects the request.
        let callback = self
            .inner
            .on_change
            .read()
            .expect("config lock poisoned")
            .clone();
        if let Some(callback) = callback {
            callback(&settings);
        }
        write_atomic(&self.inner.path, &settings)
    }

    /// The shared IPv6 flag for the DNS pipeline to read per query.
    pub fn ipv6_flag(&self) -> Arc<AtomicBool> {
        self.inner.ipv6.clone()
    }

    /// The shared flag gating the advertising/tracking blocker.
    pub fn block_ads_trackers_flag(&self) -> Arc<AtomicBool> {
        self.inner.block_ads_trackers.clone()
    }

    /// The shared flag gating the malware/phishing blocker.
    pub fn block_malware_flag(&self) -> Arc<AtomicBool> {
        self.inner.block_malware.clone()
    }

    /// The shared flag gating safe search (the DNS pipeline reads it per query).
    pub fn safe_search_flag(&self) -> Arc<AtomicBool> {
        self.inner.safe_search.clone()
    }

    /// The shared flag for the block response: true answers a blocked name with
    /// the null IP, false with NXDOMAIN. The DNS pipeline reads it per query.
    pub fn block_null_ip_flag(&self) -> Arc<AtomicBool> {
        self.inner.block_null_ip.clone()
    }

    /// The shared flag gating the query log: true records each answered query,
    /// false keeps no record. The query observer reads it per query.
    pub fn query_log_flag(&self) -> Arc<AtomicBool> {
        self.inner.query_log.clone()
    }
}

/// Write `settings` to `path` as pretty JSON, atomically: serialise to a
/// sibling temp file first, then rename over the target so a crash mid-write
/// never leaves a half-written config.
fn write_atomic(path: &Path, settings: &Settings) -> Result<(), ConfigError> {
    let io_err = |source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
    }

    let json = serde_json::to_vec_pretty(settings).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|source| ConfigError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(io_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        (dir, path)
    }

    #[test]
    fn missing_file_is_seeded_and_written() {
        let (_dir, path) = temp_path();
        let seed = Settings {
            ipv6: true,
            log_retention_days: 30,
            ..Settings::default()
        };

        let store = ConfigStore::load(&path, seed.clone()).unwrap();
        assert_eq!(store.get(), seed);
        assert!(path.exists(), "seed should be written on first run");

        // A reload reads the file, not the (now different) seed.
        let reopened = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(reopened.get(), seed);
    }

    #[test]
    fn set_persists_and_publishes_ipv6() {
        let (_dir, path) = temp_path();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let flag = store.ipv6_flag();
        assert!(!flag.load(Ordering::Relaxed));

        let mut next = store.get();
        next.ipv6 = true;
        next.upstream_mode = UpstreamMode::Fallback;
        store.set(next.clone()).unwrap();

        assert!(
            flag.load(Ordering::Relaxed),
            "live flag follows the setting"
        );
        let reopened = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(reopened.get(), next);
    }

    #[test]
    fn set_publishes_block_flags_live() {
        let (_dir, path) = temp_path();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let ads = store.block_ads_trackers_flag();
        let malware = store.block_malware_flag();
        let safe_search = store.safe_search_flag();
        // Both block categories default to on; safe search defaults off.
        assert!(ads.load(Ordering::Relaxed));
        assert!(malware.load(Ordering::Relaxed));
        assert!(!safe_search.load(Ordering::Relaxed));

        let mut next = store.get();
        next.block_ads_trackers = false;
        next.block_malware = false;
        next.safe_search = true;
        store.set(next).unwrap();

        assert!(!ads.load(Ordering::Relaxed), "ads flag follows the setting");
        assert!(
            !malware.load(Ordering::Relaxed),
            "malware flag follows the setting"
        );
        assert!(
            safe_search.load(Ordering::Relaxed),
            "safe search flag follows the setting"
        );
    }

    #[test]
    fn partial_file_fills_missing_fields_from_defaults() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();

        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let settings = store.get();
        assert!(settings.ipv6);
        // A field absent from the file falls back to the default.
        assert_eq!(settings.log_retention_days, 7);
        assert!(settings.block_malware);
    }

    #[test]
    fn unparseable_file_is_an_error() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            ConfigStore::load(&path, Settings::default()),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn upstream_mode_serialises_lowercase() {
        let json = serde_json::to_string(&UpstreamMode::Balanced).unwrap();
        assert_eq!(json, "\"balanced\"");
    }

    #[test]
    fn speed_profile_serialises_lowercase_and_defaults_to_balanced() {
        assert_eq!(SpeedProfile::default(), SpeedProfile::Balanced);
        assert_eq!(
            serde_json::to_string(&SpeedProfile::Speed).unwrap(),
            "\"speed\""
        );
        assert_eq!(
            serde_json::from_str::<SpeedProfile>("\"privacy\"").unwrap(),
            SpeedProfile::Privacy
        );
    }

    #[test]
    fn custom_upstreams_round_trip_through_json() {
        let settings = Settings {
            custom_upstreams_enabled: true,
            custom_upstreams: vec![
                CustomUpstream {
                    address: "9.9.9.9".to_owned(),
                    port: None,
                    transport: CustomTransport::Dot,
                    server_name: Some("dns.quad9.net".to_owned()),
                },
                CustomUpstream {
                    address: "192.168.1.1".to_owned(),
                    port: Some(5353),
                    transport: CustomTransport::Plain,
                    server_name: None,
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        // camelCase field name and lowercase transport on the wire.
        assert!(json.contains("\"customUpstreamsEnabled\":true"));
        assert!(json.contains("\"transport\":\"dot\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn older_config_without_custom_upstreams_still_loads() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true,"speedProfile":"speed"}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let settings = store.get();
        assert!(!settings.custom_upstreams_enabled, "defaults off");
        assert!(settings.custom_upstreams.is_empty());
        assert_eq!(settings.speed_profile, SpeedProfile::Speed);
        // Fields added later also fall back to their defaults.
        assert_eq!(settings.instance_name, "JustDNS");
    }

    #[test]
    fn a_config_with_the_retired_block_level_field_still_loads() {
        // Older files carried a `blockLevel`; it is gone now, and serde ignores
        // the unknown field rather than failing the load.
        let (_dir, path) = temp_path();
        std::fs::write(
            &path,
            r#"{"blockLevel":"extreme","instanceName":"Heimnetz"}"#,
        )
        .unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(store.get().instance_name, "Heimnetz");
    }

    #[test]
    fn instance_name_round_trips() {
        let settings = Settings {
            instance_name: "Heimnetz".to_owned(),
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"instanceName\":\"Heimnetz\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn block_response_round_trips_and_defaults_to_null_ip() {
        assert_eq!(Settings::default().block_response, BlockResponse::NullIp);
        let settings = Settings {
            block_response: BlockResponse::NxDomain,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"blockResponse\":\"nx-domain\""), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
        // The default serialises as the kebab-case null-ip.
        assert!(
            serde_json::to_string(&Settings::default())
                .unwrap()
                .contains("\"blockResponse\":\"null-ip\"")
        );
    }

    #[test]
    fn older_config_without_block_response_defaults_to_null_ip() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(store.get().block_response, BlockResponse::NullIp);
    }

    #[test]
    fn active_window_defaults_and_round_trips() {
        assert_eq!(Settings::default().active_window_secs, 300);
        // A file written before this field existed loads with the default.
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        assert_eq!(store.get().active_window_secs, 300);

        let settings = Settings {
            active_window_secs: 600,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"activeWindowSecs\":600"), "{json}");
        assert_eq!(
            serde_json::from_str::<Settings>(&json).unwrap(),
            settings,
            "round-trips through JSON"
        );
    }

    #[test]
    fn encrypted_dns_round_trips_and_defaults_off() {
        // Defaults: both endpoints off, empty hostname, self-signed.
        let defaults = Settings::default().encrypted_dns;
        assert!(!defaults.dot_enabled && !defaults.doh_enabled);
        assert!(defaults.hostname.is_empty());
        assert_eq!(defaults.cert_source, CertSource::SelfSigned);

        let settings = Settings {
            encrypted_dns: EncryptedDns {
                dot_enabled: true,
                doh_enabled: true,
                hostname: "dns.fritz.box".to_owned(),
                cert_source: CertSource::Uploaded,
                dot_cert_id: Some("cert-abc".to_owned()),
                doh_cert_id: None,
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"encryptedDns\":{"), "{json}");
        assert!(json.contains("\"dotEnabled\":true"));
        assert!(json.contains("\"certSource\":\"uploaded\""), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn block_page_defaults_off_and_round_trips() {
        let defaults = Settings::default().block_page;
        assert!(!defaults.enabled);
        assert!(defaults.server_ipv4.is_empty());
        assert!(defaults.cert_id.is_none());
        assert!(!defaults.behind_reverse_proxy);
        assert!(!defaults.title.is_empty() && !defaults.message.is_empty());

        assert!(defaults.redirect_url.is_empty());

        let settings = Settings {
            block_page: BlockPage {
                enabled: true,
                server_ipv4: "192.168.20.2".to_owned(),
                server_ipv6: String::new(),
                cert_id: Some("cert-xyz".to_owned()),
                behind_reverse_proxy: true,
                redirect_url: "https://justdns.example.com".to_owned(),
                title: "Gesperrt".to_owned(),
                message: "Nicht erlaubt.".to_owned(),
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"blockPage\":{"), "{json}");
        assert!(json.contains("\"serverIpv4\":\"192.168.20.2\""), "{json}");
        assert!(json.contains("\"behindReverseProxy\":true"), "{json}");
        assert!(
            json.contains("\"redirectUrl\":\"https://justdns.example.com\""),
            "{json}"
        );
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn older_config_without_block_page_loads_with_defaults() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        assert!(!store.get().block_page.enabled);
    }

    #[test]
    fn notifications_default_off_and_round_trip() {
        let defaults = Settings::default().notifications;
        assert!(!defaults.enabled);
        assert!(defaults.webhook_url.is_empty());
        assert_eq!(defaults.webhook_format, WebhookFormat::Generic);
        // Sensible event defaults: deterministic alerts on, the heuristic off.
        assert!(defaults.on_node_down && defaults.on_update && defaults.on_cert_expiry);
        assert!(!defaults.on_device_spike);
        assert_eq!(defaults.cert_expiry_days, 14);

        let settings = Settings {
            notifications: Notifications {
                enabled: true,
                webhook_url: "https://ntfy.sh/justdns".to_owned(),
                webhook_format: WebhookFormat::Telegram,
                telegram_chat_id: "12345".to_owned(),
                on_node_down: true,
                on_update: false,
                on_cert_expiry: true,
                on_device_spike: true,
                cert_expiry_days: 7,
                spike_blocked_threshold: 250,
                spike_window_mins: 5,
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"notifications\":{"), "{json}");
        assert!(json.contains("\"webhookFormat\":\"telegram\""), "{json}");
        assert!(json.contains("\"telegramChatId\":\"12345\""), "{json}");
        assert!(json.contains("\"spikeBlockedThreshold\":250"), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn older_config_without_notifications_loads_with_defaults() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        assert!(!store.get().notifications.enabled);
    }

    #[test]
    fn older_config_without_encrypted_dns_loads_with_defaults() {
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let encrypted = store.get().encrypted_dns;
        assert!(!encrypted.dot_enabled && !encrypted.doh_enabled);
        assert_eq!(encrypted.cert_source, CertSource::SelfSigned);
    }

    #[test]
    fn require_mfa_defaults_off_and_round_trips() {
        assert!(!Settings::default().require_mfa);
        // A file written before this field existed loads with the default.
        let (_dir, path) = temp_path();
        std::fs::write(&path, r#"{"ipv6":true}"#).unwrap();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        assert!(!store.get().require_mfa);

        let settings = Settings {
            require_mfa: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"requireMfa\":true"), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn set_publishes_the_block_null_ip_flag_live() {
        let (_dir, path) = temp_path();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let flag = store.block_null_ip_flag();
        // Default is null-ip, so the flag starts true.
        assert!(flag.load(Ordering::Relaxed));

        let mut next = store.get();
        next.block_response = BlockResponse::NxDomain;
        store.set(next).unwrap();
        assert!(!flag.load(Ordering::Relaxed), "flag follows the setting");
    }

    #[test]
    fn set_publishes_the_query_log_flag_live() {
        let (_dir, path) = temp_path();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();
        let flag = store.query_log_flag();
        // Default records queries, so the flag starts true.
        assert!(flag.load(Ordering::Relaxed));

        let mut next = store.get();
        next.query_log = false;
        store.set(next).unwrap();
        assert!(!flag.load(Ordering::Relaxed), "flag follows the setting");
    }

    #[test]
    fn set_runs_the_on_change_callback_with_the_new_settings() {
        use std::sync::atomic::AtomicU8;

        let (_dir, path) = temp_path();
        let store = ConfigStore::load(&path, Settings::default()).unwrap();

        // The callback records the profile it was handed, so the test can see it
        // ran with the just-set settings rather than the old ones.
        let seen = Arc::new(AtomicU8::new(0));
        let sink = seen.clone();
        store.on_change(move |settings| {
            let code = match settings.speed_profile {
                SpeedProfile::Privacy => 1,
                SpeedProfile::Balanced => 2,
                SpeedProfile::Speed => 3,
            };
            sink.store(code, Ordering::Relaxed);
        });

        let mut next = store.get();
        next.speed_profile = SpeedProfile::Speed;
        store.set(next).unwrap();

        assert_eq!(
            seen.load(Ordering::Relaxed),
            3,
            "callback saw the new profile"
        );
    }
}
