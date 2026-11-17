// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Active notifications: a small background evaluator that watches for a handful
//! of noteworthy conditions — a cluster node going unreachable (and recovering),
//! a newer release being available, a stored certificate nearing expiry, and a
//! device making an unusual number of blocked lookups — and posts a short message
//! to a user-configured webhook (generic JSON, ntfy, Slack, Discord or Telegram).
//!
//! Delivery uses the same blocking `ureq` client as the HA sync, so the evaluator
//! runs on its own OS thread (see `serve`), never the async runtime. Dedup state
//! is in-memory: a transition fires once, and a restart may re-announce a still-
//! true condition, which is acceptable for an at-least-once alert.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use justdns_clients::{ClientResolver, ClientStore, Group};
use justdns_config::{ConfigStore, Notifications, WebhookFormat};
use justdns_dns::cert_store::CertStore;
use justdns_geoip::{GeoDb, GeoInfo};
use justdns_store::{PhishingKind, QueryFilter, Store};
use tracing::warn;

use crate::geo_watch::{WatchSet, anything_watched, effective_watch};

use crate::ha;
use crate::update::UpdateChecker;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;
/// How many of the busiest blocked clients to look at per spike scan.
const SPIKE_SCAN_LIMIT: usize = 20;
/// How far back each watched-geo pass scans for answers resolving into a watched
/// country or network. Kept short because a pass runs every minute; the cooldown
/// below keeps a persistent contact from re-alerting.
const WATCHED_GEO_WINDOW_MS: i64 = 5 * 60 * 1_000;
/// How many recent events a watched-geo pass reads at most, so a very busy
/// instance never scans an unbounded slice.
const WATCHED_GEO_SCAN_LIMIT: usize = 20_000;
/// Per-(device, watched network) cooldown: a still-ongoing contact alerts at most
/// once an hour rather than every pass.
const WATCHED_GEO_COOLDOWN_MS: i64 = 60 * 60 * 1_000;

/// How far back a phishing-alert pass looks for freshly recorded hits. Short
/// because a pass runs every minute; the cooldown below keeps a repeated hit on
/// the same name from re-alerting.
const PHISHING_WINDOW_MS: i64 = 5 * 60 * 1_000;
/// How many recent events a phishing pass reads at most, bounding the scan on a
/// very busy instance.
const PHISHING_SCAN_LIMIT: usize = 20_000;
/// Per-(device, registrable domain, kind, mode) cooldown: one site a device keeps
/// looking up alerts at most once an hour rather than every pass, so an open page
/// does not spam the webhook.
const PHISHING_COOLDOWN_MS: i64 = 60 * 60 * 1_000;

/// Post `title`/`body` to the configured webhook, in the shape its format expects.
/// Returns a short error string on any failure (no URL, transport error, …).
pub fn send(cfg: &Notifications, title: &str, body: &str) -> Result<(), String> {
    let req = build_request(cfg, title, body)?;
    guard_target(&req.url)?;
    let mut call = ureq::post(&req.url)
        .timeout(HTTP_TIMEOUT)
        .set("Content-Type", req.content_type);
    if let Some((name, value)) = &req.extra_header {
        call = call.set(name, value);
    }
    call.send_string(&req.body)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Refuse a webhook target that resolves to a link-local or unspecified address.
/// This blocks the cloud metadata endpoint (`169.254.169.254` and its IPv6
/// equivalents) and other never-legitimate destinations, while deliberately still
/// allowing private LAN and loopback targets — a self-hosted ntfy/Gotify on the
/// operator's own network is the common case. A host that cannot be resolved is
/// let through so the real request fails with its own error.
fn guard_target(url: &str) -> Result<(), String> {
    let Some(host) = target_host(url) else {
        return Ok(());
    };
    let blocked = "webhook target is a link-local/metadata address and is not allowed";
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_disallowed_ip(ip) {
            Err(blocked.to_owned())
        } else {
            Ok(())
        };
    }
    // A hostname may resolve to a disallowed address (e.g. metadata.google.internal
    // → 169.254.169.254), so check every resolved address.
    match (host.as_str(), 0u16).to_socket_addrs() {
        Ok(addrs) => {
            for addr in addrs {
                if is_disallowed_ip(addr.ip()) {
                    return Err(blocked.to_owned());
                }
            }
            Ok(())
        }
        Err(_) => Ok(()),
    }
}

/// Whether an address is a never-legitimate webhook destination: link-local (which
/// includes the `169.254.169.254` cloud metadata endpoint and IPv6 `fe80::/10`) or
/// unspecified. Loopback and private ranges are intentionally allowed.
fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast(),
        IpAddr::V6(v6) => {
            // A dual-stack resolver (or a crafted literal) can present the metadata
            // endpoint as an IPv4-mapped address — `::ffff:169.254.169.254`. Unwrap
            // it and apply the IPv4 rules, so the link-local/metadata block cannot be
            // slipped past by dressing it up as IPv6.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_disallowed_ip(IpAddr::V4(v4));
            }
            v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Extract the host from a URL without a URL-parser dependency: drop the scheme,
/// stop at the first `/`, `?` or `#`, drop any `user@`, unwrap an `[IPv6]` literal,
/// and strip a trailing `:port`. Returns `None` when no host is present.
fn target_host(url: &str) -> Option<String> {
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

/// The concrete HTTP request a webhook format maps to. Split out from [`send`] so
/// the per-format shaping can be unit-tested without a network call.
struct WebhookRequest {
    url: String,
    body: String,
    content_type: &'static str,
    extra_header: Option<(&'static str, String)>,
}

fn build_request(cfg: &Notifications, title: &str, body: &str) -> Result<WebhookRequest, String> {
    let target = cfg.webhook_url.trim();
    if target.is_empty() {
        return Err("no webhook URL set".to_owned());
    }
    let json = "application/json";
    match cfg.webhook_format {
        WebhookFormat::Generic => Ok(WebhookRequest {
            url: target.to_owned(),
            body: serde_json::json!({ "title": title, "message": body }).to_string(),
            content_type: json,
            extra_header: None,
        }),
        WebhookFormat::Ntfy => Ok(WebhookRequest {
            url: target.to_owned(),
            body: body.to_owned(),
            content_type: "text/plain; charset=utf-8",
            // ntfy carries the title in a header; keep titles ASCII for it.
            extra_header: Some(("Title", title.to_owned())),
        }),
        WebhookFormat::Slack => Ok(WebhookRequest {
            url: target.to_owned(),
            body: serde_json::json!({ "text": format!("*{title}*\n{body}") }).to_string(),
            content_type: json,
            extra_header: None,
        }),
        WebhookFormat::Discord => Ok(WebhookRequest {
            url: target.to_owned(),
            body: serde_json::json!({ "content": format!("**{title}**\n{body}") }).to_string(),
            content_type: json,
            extra_header: None,
        }),
        WebhookFormat::Telegram => {
            let chat = cfg.telegram_chat_id.trim();
            if chat.is_empty() {
                return Err("no Telegram chat id set".to_owned());
            }
            // For Telegram the URL field holds the bot token.
            Ok(WebhookRequest {
                url: format!("https://api.telegram.org/bot{target}/sendMessage"),
                body: serde_json::json!({ "chat_id": chat, "text": format!("{title}\n{body}") })
                    .to_string(),
                content_type: json,
                extra_header: None,
            })
        }
    }
}

/// In-memory dedup so a still-true condition is not re-announced every cycle.
#[derive(Default)]
struct AlertState {
    /// Peer ids currently considered down (to fire once on each transition).
    down_peers: HashSet<String>,
    /// The latest version we have already announced as available.
    alerted_update: Option<String>,
    /// `"<cert id>@<not_after_ms>"` entries already announced as expiring.
    alerted_certs: HashSet<String>,
    /// Client address → last spike-alert time (ms), for a per-device cooldown.
    spike_last_ms: HashMap<String, i64>,
    /// `"<client>|<country?>|<asn?>"` → last watched-geo alert time (ms), for a
    /// per-device, per-network cooldown.
    watched_last_ms: HashMap<String, i64>,
    /// `"<root>|<kind>|<mode>|<client>"` → last phishing alert time (ms), for a
    /// per-device, per-registrable-domain cooldown.
    phishing_last_ms: HashMap<String, i64>,
}

/// Watches the alert conditions and posts to the webhook. Reads the live config
/// each pass, so turning notifications on/off or editing the webhook takes effect
/// without a restart.
pub struct AlertEvaluator {
    config: ConfigStore,
    ha: Arc<ha::Ha>,
    update: Arc<UpdateChecker>,
    certs: Option<Arc<CertStore>>,
    store: Store,
    clients: ClientStore,
    geo: Option<Arc<GeoDb>>,
    state: Mutex<AlertState>,
}

impl AlertEvaluator {
    pub fn new(
        config: ConfigStore,
        ha: Arc<ha::Ha>,
        update: Arc<UpdateChecker>,
        certs: Option<Arc<CertStore>>,
        store: Store,
        clients: ClientStore,
        geo: Option<Arc<GeoDb>>,
    ) -> Self {
        Self {
            config,
            ha,
            update,
            certs,
            store,
            clients,
            geo,
            state: Mutex::new(AlertState::default()),
        }
    }

    /// One evaluation pass. Cheap and side-effect-free when notifications are off.
    pub fn evaluate(&self) {
        let settings = self.config.get();
        let cfg = settings.notifications;
        if !cfg.enabled {
            return;
        }
        let label = settings.instance_name;
        // Each check computes what to send under the lock (updating dedup state),
        // then posts after the lock is released so network I/O never holds it.
        for (title, body) in self.collect(&cfg) {
            if let Err(error) = send(&cfg, &title, &format!("[{label}] {body}")) {
                warn!(%error, "notification webhook failed");
            }
        }
    }

    /// Gather the messages to send this pass, advancing dedup state as it goes.
    fn collect(&self, cfg: &Notifications) -> Vec<(String, String)> {
        let now = now_ms();
        let mut out = Vec::new();
        let mut state = self.state.lock().expect("alert state poisoned");
        if cfg.on_node_down {
            self.collect_nodes(&mut state, &mut out);
        }
        if cfg.on_update {
            self.collect_update(&mut state, &mut out);
        }
        if cfg.on_cert_expiry {
            self.collect_certs(cfg, now, &mut state, &mut out);
        }
        if cfg.on_device_spike {
            self.collect_spike(cfg, now, &mut state, &mut out);
        }
        if cfg.on_watched_geo {
            self.collect_watched_geo(now, &mut state, &mut out);
        }
        if cfg.on_phishing {
            self.collect_phishing(now, &mut state, &mut out);
        }
        out
    }

    fn collect_nodes(&self, state: &mut AlertState, out: &mut Vec<(String, String)>) {
        if !self.ha.enabled() {
            return;
        }
        let now = now_ms();
        // A peer counts as down only once a sync has actually failed AND it has
        // not succeeded within a few intervals — a peer that has simply never
        // synced yet (no error) is "pending", not down, so a boot never alarms.
        let threshold = (self.ha.interval().as_millis() as i64).saturating_mul(3);
        let mut down_now: HashSet<String> = HashSet::new();
        let mut label_of: HashMap<String, String> = HashMap::new();
        for peer in self.ha.status().peers {
            let id = peer
                .peer_node_id
                .clone()
                .unwrap_or_else(|| peer.url.clone());
            let stale = peer
                .last_sync_ms
                .is_none_or(|t| now - (t as i64) > threshold);
            if peer.last_error.is_some() && stale {
                down_now.insert(id.clone());
            }
            label_of.insert(id, ha::host_label(&peer.url));
        }
        let name = |id: &str| label_of.get(id).cloned().unwrap_or_else(|| id.to_owned());
        for id in down_now.difference(&state.down_peers) {
            out.push((
                "JustDNS: cluster node offline".to_owned(),
                format!("Node {} is no longer reachable from this node.", name(id)),
            ));
        }
        for id in state.down_peers.difference(&down_now) {
            out.push((
                "JustDNS: cluster node back".to_owned(),
                format!("Node {} is reachable again.", name(id)),
            ));
        }
        state.down_peers = down_now;
    }

    fn collect_update(&self, state: &mut AlertState, out: &mut Vec<(String, String)>) {
        let snap = self.update.snapshot();
        if !snap.update_available {
            return;
        }
        let Some(latest) = snap.latest else {
            return;
        };
        // In a cluster every node sees the same release; only the node with the
        // lowest id among the reachable ones sends, so the alert arrives once.
        if !self.is_primary_sender() {
            return;
        }
        if state.alerted_update.as_deref() == Some(latest.as_str()) {
            return;
        }
        state.alerted_update = Some(latest.clone());
        let mut body = format!("JustDNS {latest} is available to install.");
        if let Some(url) = snap.release_url {
            body.push(' ');
            body.push_str(&url);
        }
        out.push(("JustDNS: update available".to_owned(), body));
    }

    fn collect_certs(
        &self,
        cfg: &Notifications,
        now: i64,
        state: &mut AlertState,
        out: &mut Vec<(String, String)>,
    ) {
        let Some(certs) = &self.certs else {
            return;
        };
        let horizon = i64::from(cfg.cert_expiry_days).saturating_mul(MS_PER_DAY);
        for info in certs.list() {
            let remaining = info.not_after_ms - now;
            if remaining >= horizon {
                continue;
            }
            // Keyed on expiry too, so a renewal (new not_after) can alert afresh.
            let key = format!("{}@{}", info.id, info.not_after_ms);
            if !state.alerted_certs.insert(key) {
                continue;
            }
            let body = if remaining <= 0 {
                format!("Certificate \"{}\" has expired.", info.name)
            } else {
                let days = (remaining / MS_PER_DAY).max(0);
                format!("Certificate \"{}\" expires in {days} day(s).", info.name)
            };
            out.push(("JustDNS: certificate expiring".to_owned(), body));
        }
    }

    fn collect_spike(
        &self,
        cfg: &Notifications,
        now: i64,
        state: &mut AlertState,
        out: &mut Vec<(String, String)>,
    ) {
        let window = i64::from(cfg.spike_window_mins.max(1)).saturating_mul(60_000);
        let threshold = u64::from(cfg.spike_blocked_threshold);
        let counts = match self
            .store
            .blocked_counts_by_client(now - window, now, SPIKE_SCAN_LIMIT)
        {
            Ok(counts) => counts,
            Err(error) => {
                warn!(%error, "spike scan query failed");
                return;
            }
        };
        for (client, count) in counts {
            if count < threshold {
                break; // ordered busiest-first, so the rest are below too
            }
            // One alert per device per window, so a sustained spike is not spammy.
            if let Some(last) = state.spike_last_ms.get(&client) {
                if now - last < window {
                    continue;
                }
            }
            state.spike_last_ms.insert(client.clone(), now);
            out.push((
                "JustDNS: device activity spike".to_owned(),
                format!(
                    "Device {client} made {count} blocked lookups in the last {} min \
                     (possible malware beaconing).",
                    cfg.spike_window_mins.max(1)
                ),
            ));
        }
    }

    /// Alert when a recent answer resolved into a watched country or network. The
    /// watch set for each client is the union of the global list and the client's
    /// group and device deltas, so a device-specific watch alerts only for that
    /// device. Each node evaluates its own queries (a contact is local to the node
    /// that served it), so there is no primary-sender gating here. Nothing is
    /// scanned when no scope watches anything.
    fn collect_watched_geo(
        &self,
        now: i64,
        state: &mut AlertState,
        out: &mut Vec<(String, String)>,
    ) {
        let Some(db) = &self.geo else {
            return;
        };
        let global = self.config.get().geo_watch;
        let devices = self.clients.view();
        let groups_list = self.clients.groups();
        if !anything_watched(&global, &devices, &groups_list) {
            return;
        }

        // Drop cooldown entries that have aged out, so the map stays bounded by
        // the networks contacted within the last cooldown window rather than
        // growing for the life of the process (stale clients, un-watched nets).
        state
            .watched_last_ms
            .retain(|_, last| now - *last < WATCHED_GEO_COOLDOWN_MS);

        let groups: HashMap<String, Group> =
            groups_list.into_iter().map(|g| (g.id.clone(), g)).collect();
        let resolver = ClientResolver::new(devices);
        let filter = QueryFilter {
            before: None,
            start_ms: Some(now - WATCHED_GEO_WINDOW_MS),
            end_ms: Some(now),
            sources: Vec::new(),
            record_type: None,
            response_code: None,
            resolver: None,
            origin: None,
            clients: None,
            needle: None,
            phishing: None,
            limit: WATCHED_GEO_SCAN_LIMIT,
        };
        let rows = match self.store.history(&filter) {
            Ok((rows, _)) => rows,
            Err(error) => {
                warn!(%error, "watched-geo scan query failed");
                return;
            }
        };

        // Resolve each distinct client's effective watch set once; a 5-minute scan
        // of a busy node repeats the same handful of clients across many rows.
        let mut watch_cache: HashMap<IpAddr, WatchSet> = HashMap::new();
        for row in &rows {
            let client = row.event.client;
            let watch = watch_cache
                .entry(client)
                .or_insert_with(|| effective_watch(&global, resolver.resolve(client), &groups));
            if watch.is_empty() {
                continue;
            }
            // One query counts once per distinct network it resolved into.
            let mut seen: HashSet<(Option<String>, Option<u32>)> = HashSet::new();
            for ip in &row.event.addresses {
                let Some(info) = db.lookup(*ip) else {
                    continue;
                };
                if !seen.insert((info.country.clone(), info.asn)) {
                    continue;
                }
                if !watch.matches(&info) {
                    continue;
                }
                let dedup = format!("{client}|{:?}|{:?}", info.country, info.asn);
                if let Some(last) = state.watched_last_ms.get(&dedup) {
                    if now - last < WATCHED_GEO_COOLDOWN_MS {
                        continue;
                    }
                }
                state.watched_last_ms.insert(dedup, now);
                out.push((
                    "JustDNS: watched network contacted".to_owned(),
                    format!(
                        "Device {client} resolved {} into {} (watched).",
                        row.event.name,
                        describe_geo(&info)
                    ),
                ));
            }
        }
    }

    /// Scan the recent query log for phishing hits the detector recorded and post
    /// one alert per newly seen name, deduped per (name, kind, mode) on a cooldown.
    /// Node-local only: it reads this node's own history, matching where the hits
    /// were produced.
    fn collect_phishing(&self, now: i64, state: &mut AlertState, out: &mut Vec<(String, String)>) {
        // Drop cooldown entries that have aged out so the map stays bounded by the
        // names hit within the last cooldown window.
        state
            .phishing_last_ms
            .retain(|_, last| now - *last < PHISHING_COOLDOWN_MS);

        let filter = QueryFilter {
            before: None,
            start_ms: Some(now - PHISHING_WINDOW_MS),
            end_ms: Some(now),
            sources: Vec::new(),
            record_type: None,
            response_code: None,
            resolver: None,
            origin: Some(String::new()),
            clients: None,
            needle: None,
            phishing: None,
            limit: PHISHING_SCAN_LIMIT,
        };
        let rows = match self.store.history(&filter) {
            Ok((rows, _)) => rows,
            Err(error) => {
                warn!(%error, "phishing scan query failed");
                return;
            }
        };

        // Resolve device names once so an alert can name the device, not only its
        // address, matching how the query log labels it.
        let resolver = ClientResolver::new(self.clients.view());

        for row in &rows {
            let Some(flag) = &row.event.phishing else {
                continue;
            };
            let (kind_key, kind_text) = match flag.kind {
                PhishingKind::Lookalike => ("lookalike", describe_lookalike(&flag.brand)),
                PhishingKind::Punycode => ("punycode", describe_punycode(&flag.brand)),
                PhishingKind::Nrd => ("nrd", "neu registriert".to_owned()),
            };
            let mode_key = if flag.blocked { "block" } else { "warn" };
            // De-dupe per (device, registrable domain, kind, mode): one open page
            // re-resolving www./apex/cdn sub-hosts of the same site raises a single
            // alert per device, not one per distinct sub-host.
            let root = registrable_root(&row.event.name);
            let dedup = format!("{root}|{kind_key}|{mode_key}|{}", row.event.client);
            if let Some(last) = state.phishing_last_ms.get(&dedup) {
                if now - last < PHISHING_COOLDOWN_MS {
                    continue;
                }
            }
            state.phishing_last_ms.insert(dedup, now);
            let device = resolver
                .name_for(row.event.client)
                .map(|name| format!("{name} ({})", row.event.client))
                .unwrap_or_else(|| row.event.client.to_string());
            let (title, tail) = if flag.blocked {
                ("JustDNS: Phishing blockiert", "Antwort blockiert.")
            } else {
                (
                    "JustDNS: Phishing-Warnung",
                    "Nur markiert, nicht blockiert.",
                )
            };
            out.push((
                title.to_owned(),
                format!(
                    "Verdächtige Domain {} ({kind_text}). Angefragt von {device}. {tail}",
                    row.event.name
                ),
            ));
        }
    }

    /// Whether this node is the one that should send cluster-wide alerts: true on
    /// a standalone node or HA-off, and otherwise only for the lowest node id
    /// among this node and its currently-reachable peers.
    fn is_primary_sender(&self) -> bool {
        if !self.ha.enabled() {
            return true;
        }
        let me = self.ha.node_id();
        let mut lowest = me.clone();
        for peer in self.ha.status().peers {
            if peer.last_error.is_none() {
                if let Some(id) = peer.peer_node_id {
                    if id < lowest {
                        lowest = id;
                    }
                }
            }
        }
        lowest == me
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Describe a lookalike hit for an alert body: the imitated brand when known,
/// otherwise a plain label.
fn describe_lookalike(brand: &Option<String>) -> String {
    match brand {
        Some(brand) => format!("sieht aus wie {brand}"),
        None => "Nachahmer-Domain".to_owned(),
    }
}

/// Describe a Punycode hit for an alert body: the imitated brand when known,
/// otherwise a plain label.
fn describe_punycode(brand: &Option<String>) -> String {
    match brand {
        Some(brand) => format!("Punycode, sieht aus wie {brand}"),
        None => "Punycode-Nachahmung".to_owned(),
    }
}

/// Widespread two-label public suffixes, so a host under one keeps its own
/// registrable label (e.g. `shop.co.uk` stays `shop.co.uk`, not `co.uk`). Not the
/// full public-suffix list — just the common ones — which is enough to collapse
/// the sub-hosts of one site for alert de-duplication.
const MULTI_LABEL_SUFFIXES: &[&str] = &[
    "co.uk", "org.uk", "me.uk", "ac.uk", "gov.uk", "com.au", "net.au", "org.au", "co.nz", "org.nz",
    "co.za", "co.jp", "or.jp", "ne.jp", "com.br", "com.tr", "co.in", "co.kr", "com.cn", "com.mx",
    "com.ar", "com.sg", "com.hk",
];

/// The registrable domain of `host`: enough labels to identify one site, so that
/// `www.0-100.ch`, `0-100.ch` and `cdn.0-100.ch` share one value and a page that
/// re-resolves many sub-hosts raises a single alert. Uses a short list of common
/// two-label public suffixes; otherwise the last two labels, which is correct for
/// the single-label TLDs (`.ch`, `.de`, `.at`, …) this sees in practice.
fn registrable_root(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').filter(|label| !label.is_empty()).collect();
    if labels.len() <= 2 {
        return host;
    }
    let last_two = format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1]);
    let take = if MULTI_LABEL_SUFFIXES.contains(&last_two.as_str()) {
        3
    } else {
        2
    };
    let start = labels.len().saturating_sub(take);
    labels[start..].join(".")
}

/// A short English label for a resolved network, for the alert body — e.g.
/// "US AS13335 Cloudflare, Inc.".
fn describe_geo(info: &GeoInfo) -> String {
    let mut parts = Vec::new();
    if let Some(country) = &info.country {
        parts.push(country.to_uppercase());
    }
    if let Some(asn) = info.asn {
        parts.push(format!("AS{asn}"));
    }
    if let Some(org) = &info.org {
        parts.push(org.clone());
    }
    if parts.is_empty() {
        "an unknown network".to_owned()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(format: WebhookFormat, url: &str) -> Notifications {
        Notifications {
            webhook_url: url.to_owned(),
            webhook_format: format,
            ..Notifications::default()
        }
    }

    #[test]
    fn registrable_root_collapses_sub_hosts_to_one_site() {
        assert_eq!(registrable_root("www.0-100.ch"), "0-100.ch");
        assert_eq!(registrable_root("0-100.ch"), "0-100.ch");
        assert_eq!(registrable_root("cdn.0-100.ch"), "0-100.ch");
        assert_eq!(registrable_root("0-100.ch."), "0-100.ch");
        assert_eq!(registrable_root("WWW.0-100.CH"), "0-100.ch");
        // A two-label public suffix keeps the site's own label.
        assert_eq!(registrable_root("shop.example.co.uk"), "example.co.uk");
        assert_eq!(registrable_root("example.co.uk"), "example.co.uk");
    }

    #[test]
    fn target_host_parses_scheme_userinfo_port_and_ipv6() {
        assert_eq!(
            target_host("https://ntfy.sh/justdns").as_deref(),
            Some("ntfy.sh")
        );
        assert_eq!(
            target_host("http://user:pw@10.0.0.5:8080/push").as_deref(),
            Some("10.0.0.5")
        );
        assert_eq!(
            target_host("http://[fe80::1]:80/").as_deref(),
            Some("fe80::1")
        );
        assert_eq!(
            target_host("https://169.254.169.254/latest").as_deref(),
            Some("169.254.169.254")
        );
    }

    #[test]
    fn guard_blocks_metadata_and_linklocal_but_allows_lan_and_loopback() {
        // Cloud metadata / link-local / unspecified are refused.
        assert!(guard_target("http://169.254.169.254/latest/meta-data").is_err());
        assert!(guard_target("http://[fe80::1]/x").is_err());
        assert!(guard_target("http://0.0.0.0/x").is_err());
        // The metadata endpoint dressed up as an IPv4-mapped IPv6 literal is still
        // refused (would otherwise slip past the IPv6 branch).
        assert!(guard_target("http://[::ffff:169.254.169.254]/latest").is_err());
        // A self-hosted ntfy/Gotify on the LAN or loopback is allowed.
        assert!(guard_target("http://192.168.20.9:8080/justdns").is_ok());
        assert!(guard_target("http://10.1.2.3/hook").is_ok());
        assert!(guard_target("http://127.0.0.1:8080/hook").is_ok());
    }

    #[test]
    fn generic_posts_title_and_message_json() {
        let req = build_request(
            &cfg(WebhookFormat::Generic, "https://hook.example/x"),
            "T",
            "B",
        )
        .unwrap();
        assert_eq!(req.url, "https://hook.example/x");
        assert_eq!(req.content_type, "application/json");
        assert!(req.extra_header.is_none());
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(v["title"], "T");
        assert_eq!(v["message"], "B");
    }

    #[test]
    fn ntfy_posts_plain_body_with_title_header() {
        let req = build_request(
            &cfg(WebhookFormat::Ntfy, "https://ntfy.sh/justdns"),
            "T",
            "B",
        )
        .unwrap();
        assert_eq!(req.body, "B");
        assert!(req.content_type.starts_with("text/plain"));
        assert_eq!(req.extra_header, Some(("Title", "T".to_owned())));
    }

    #[test]
    fn slack_and_discord_use_their_json_field() {
        let slack = build_request(
            &cfg(WebhookFormat::Slack, "https://hooks.slack.com/x"),
            "T",
            "B",
        )
        .unwrap();
        let sv: serde_json::Value = serde_json::from_str(&slack.body).unwrap();
        assert!(
            sv["text"].as_str().unwrap().contains("T")
                && sv["text"].as_str().unwrap().contains("B")
        );

        let discord = build_request(
            &cfg(WebhookFormat::Discord, "https://discord.com/api/webhooks/x"),
            "T",
            "B",
        )
        .unwrap();
        let dv: serde_json::Value = serde_json::from_str(&discord.body).unwrap();
        assert!(dv["content"].as_str().unwrap().contains("B"));
    }

    #[test]
    fn telegram_builds_bot_url_and_needs_a_chat_id() {
        let mut c = cfg(WebhookFormat::Telegram, "123:ABC");
        // No chat id yet → rejected.
        assert!(build_request(&c, "T", "B").is_err());
        c.telegram_chat_id = "999".to_owned();
        let req = build_request(&c, "T", "B").unwrap();
        assert_eq!(req.url, "https://api.telegram.org/bot123:ABC/sendMessage");
        let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(v["chat_id"], "999");
        assert!(v["text"].as_str().unwrap().contains("B"));
    }

    #[test]
    fn empty_url_is_rejected() {
        assert!(build_request(&cfg(WebhookFormat::Generic, "   "), "T", "B").is_err());
    }
}
