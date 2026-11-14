// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! HTTP API, WebSocket streams, authentication and the embedded web UI.
//!
//! The JSON API under `/api` reads the query log's hot view so the dashboard
//! can show live figures. Everything else falls through to the embedded
//! single-page UI: its assets are embedded into the binary in release builds;
//! in debug builds `rust-embed` reads them from `ui/dist` at runtime, so a
//! debug build compiles even before the UI is built.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{FromRef, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post, put};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine as _;
use justdns_access::{AccessRequest, AccessStore};
use justdns_audit::{AuditEntry, AuditStore};
use justdns_auth::{AuthError, Role, SessionState, SessionStore, User, UserStore};
use justdns_clients::{
    ClientError, ClientResolver, ClientStore, Device, DeviceKind, Group, PolicyDelta, parse_target,
};
use justdns_config::{BlockPage, ConfigStore, Settings};
use justdns_dns::cache::DnsCache;
use justdns_dns::cert_store::{CertInfo, CertRecord, CertStore};
use justdns_filter::Blocker;
use justdns_lists::{
    ListEntry, ListError, ListKind, ListStore, ListsView, RefreshOutcome, Subscription, UserLists,
};
use justdns_rewrite::{Alias, Rewrite, RewriteError, RewriteStore, RewritesView};
use justdns_schedule::{
    Schedule, ScheduleDraft, ScheduleError, ScheduleStore, ScheduleTarget, TargetScope, parse_tz,
};
use justdns_store::{
    ActivityBucket, Cursor, DnssecStatus, DomainCount, QueryEvent, QueryFilter, QuerySource,
    ResolverStat, Store,
};
use reports::{
    REPORT_TOP_N, RenderLang, Report, ReportPeriod, ReportStore, ReportSummary, adhoc_window,
    due_daily, due_weekly, to_csv, to_html,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{info, warn};

mod ha;
mod ha_crypto;
mod metrics;
mod notify;
mod reports;
mod syslog;
mod update;
use syslog::LogRecord;
pub use syslog::{LogBuffer, LogLevelControl, RingLayer, RuntimeStatus};
use update::{UpdateChecker, UpdateProgress};

/// Name of the cookie that carries the session token. HttpOnly and
/// SameSite=Strict; not marked Secure because the dashboard is served over
/// plain HTTP on the LAN, where a Secure cookie would never be sent back.
const SESSION_COOKIE: &str = "justdns_session";

/// Default number of recent queries returned by `/api/queries`.
const DEFAULT_QUERY_LIMIT: usize = 50;
/// Upper bound on how many recent queries a caller may request at once.
const MAX_QUERY_LIMIT: usize = 500;
/// How many distinct client addresses the explorer scans when turning a device-
/// or group-filter into a concrete `client IN (…)` set. Far above any real home
/// or small-office client count, so the set is complete in practice.
const MAX_FILTER_CLIENTS: usize = 50_000;
/// How many of a peer's query-log events to pull per HA sync cycle. Bounds the
/// per-round transfer; a large backlog is caught up over several cycles.
const HA_QUERY_PULL_LIMIT: usize = 5_000;
/// Milliseconds in a day, for turning the retention setting into a cutoff.
const MS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;
/// How often the query log is pruned to the retention window.
const RETENTION_PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often the scheduler checks whether a report window has come due. A few
/// times an hour is ample — windows are day-grained — and cheap when nothing is
/// due (a read of the config and the stored report list).
const REPORT_CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// How often the notification evaluator checks its alert conditions.
const ALERT_CHECK_INTERVAL: Duration = Duration::from_secs(60);
/// Default number of diagnostic log events returned by `/api/logs/system`.
const DEFAULT_LOG_LIMIT: usize = 200;
/// Upper bound on diagnostic log events a caller may request at once.
const MAX_LOG_LIMIT: usize = 2000;
/// How many recent queries to scan when counting the active clients.
const ACTIVE_CLIENT_WINDOW: usize = 1000;

/// How many failed login/second-factor attempts for one username within the
/// window before further attempts are refused.
const MAX_LOGIN_ATTEMPTS: u32 = 10;
/// The rolling window over which failed attempts are counted.
const LOGIN_WINDOW: Duration = Duration::from_secs(300);

/// Smallest sync key the wizard accepts. The key alone authorises the peer
/// snapshot, which from HA Stage B carries every password hash and TOTP secret,
/// so it must not be guessable. A pinned env key is the operator's own call and
/// is not checked here.
const MIN_SYNC_KEY_LEN: usize = 16;

/// A soft, in-memory rate limiter for the login and second-factor endpoints,
/// keyed by username. It slows an online guess of a password, TOTP or recovery
/// code; it is not persisted, so a restart clears it (acceptable for a LAN box).
#[derive(Clone)]
struct LoginThrottle {
    inner: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}

impl LoginThrottle {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Whether `key` has used up its attempts for the current window. A window
    /// that has elapsed is reset (and the key forgotten) on the way through.
    fn is_locked(&self, key: &str) -> bool {
        let mut map = self.inner.lock().expect("throttle lock poisoned");
        match map.get(key) {
            Some((count, started)) => {
                if started.elapsed() > LOGIN_WINDOW {
                    map.remove(key);
                    false
                } else {
                    *count >= MAX_LOGIN_ATTEMPTS
                }
            }
            None => false,
        }
    }

    /// Note one failed attempt for `key`, starting a fresh window if the last
    /// one has elapsed.
    fn note_failure(&self, key: &str) {
        let mut map = self.inner.lock().expect("throttle lock poisoned");
        let entry = map.entry(key.to_owned()).or_insert((0, Instant::now()));
        if entry.1.elapsed() > LOGIN_WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    /// Forget a key's attempts after a successful sign-in.
    fn clear(&self, key: &str) {
        self.inner
            .lock()
            .expect("throttle lock poisoned")
            .remove(key);
    }
}

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

/// A closure the binary supplies to rebuild the live DoT/DoH and block-page TLS
/// configs after a certificate is renewed or regenerated, so the change is
/// served without a restart.
type CertReload = Arc<dyn Fn() + Send + Sync>;

/// What the API handlers share: the query log, the editable configuration, the
/// blocking engine (to explain why a name was blocked), the account list and
/// the live session table. Each handler extracts just the piece it needs via
/// [`FromRef`].
#[derive(Clone)]
struct AppState {
    store: Store,
    config: ConfigStore,
    blocker: Arc<Blocker>,
    lists: ListStore,
    rewrites: RewriteStore,
    clients: ClientStore,
    schedules: ScheduleStore,
    /// The pending "ask to unblock this page" queue, filled by the block page and
    /// worked off by the admin dashboard.
    access: AccessStore,
    cache: DnsCache,
    users: UserStore,
    sessions: SessionStore,
    /// The append-only security audit log (logins, account and settings changes).
    audit: AuditStore,
    /// Soft rate limiter for the login and second-factor endpoints, so a stolen
    /// password cannot be paired with a brute-forced TOTP or recovery code.
    throttle: LoginThrottle,
    /// The shared certificate store backing DoT/DoH and the block page. `None`
    /// when TLS could not be prepared at startup, in which case the `/api/certs`
    /// and `/api/tls` endpoints report it as unavailable rather than failing.
    certs: Option<Arc<CertStore>>,
    /// Rebuilds the live DoT/DoH and block-page TLS configs from the current
    /// selections, so a renewed or regenerated certificate is served at once.
    /// Supplied by the binary; `None` in tests and when TLS is unavailable.
    cert_reload: Option<CertReload>,
    /// The ports the DoT and DoH listeners bind, so the UI can show copy-ready
    /// endpoint addresses. Taken from the binary's configuration.
    dot_port: u16,
    doh_port: u16,
    /// The bounded ring of recent diagnostic log events, shown on the System page.
    logs: LogBuffer,
    /// Live runtime state (uptime, listener and feed status) for the health and
    /// diagnostics endpoints.
    status: Arc<RuntimeStatus>,
    /// Runtime control over the global log verbosity, so an admin can raise it to
    /// debug/trace temporarily from the System page. Not persisted — a restart
    /// returns to the shipping default.
    log_level: Arc<LogLevelControl>,
    /// The bearer token that guards `GET /metrics`. `None` disables the endpoint
    /// entirely (it answers 404); when set, a scraper must present it.
    metrics_token: Option<Arc<str>>,
    /// Checks GitHub for newer releases and mediates the on-disk update protocol
    /// the updater sidecar watches. Shared so the background checker and the API
    /// handlers see the same cache.
    update: Arc<UpdateChecker>,
    /// High-availability sync: this node's identity, pairing state and ledger.
    /// Shared so the background sync loop, the peer endpoints and the management
    /// API all see the same state. Off (and inert) until the user enables it.
    ha: Arc<ha::Ha>,
    /// Generated summary reports, persisted on the data volume. The scheduler
    /// thread writes into it; the `/api/reports` handlers read from it.
    reports: ReportStore,
}

impl FromRef<AppState> for Store {
    fn from_ref(state: &AppState) -> Self {
        state.store.clone()
    }
}

impl FromRef<AppState> for ConfigStore {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

impl FromRef<AppState> for Arc<Blocker> {
    fn from_ref(state: &AppState) -> Self {
        state.blocker.clone()
    }
}

impl FromRef<AppState> for ListStore {
    fn from_ref(state: &AppState) -> Self {
        state.lists.clone()
    }
}

impl FromRef<AppState> for RewriteStore {
    fn from_ref(state: &AppState) -> Self {
        state.rewrites.clone()
    }
}

impl FromRef<AppState> for ClientStore {
    fn from_ref(state: &AppState) -> Self {
        state.clients.clone()
    }
}

impl FromRef<AppState> for ScheduleStore {
    fn from_ref(state: &AppState) -> Self {
        state.schedules.clone()
    }
}

impl FromRef<AppState> for DnsCache {
    fn from_ref(state: &AppState) -> Self {
        state.cache.clone()
    }
}

impl FromRef<AppState> for UserStore {
    fn from_ref(state: &AppState) -> Self {
        state.users.clone()
    }
}

impl FromRef<AppState> for SessionStore {
    fn from_ref(state: &AppState) -> Self {
        state.sessions.clone()
    }
}

impl FromRef<AppState> for AuditStore {
    fn from_ref(state: &AppState) -> Self {
        state.audit.clone()
    }
}

impl FromRef<AppState> for LogBuffer {
    fn from_ref(state: &AppState) -> Self {
        state.logs.clone()
    }
}

impl FromRef<AppState> for Arc<RuntimeStatus> {
    fn from_ref(state: &AppState) -> Self {
        state.status.clone()
    }
}

impl FromRef<AppState> for Arc<LogLevelControl> {
    fn from_ref(state: &AppState) -> Self {
        state.log_level.clone()
    }
}

impl FromRef<AppState> for Arc<UpdateChecker> {
    fn from_ref(state: &AppState) -> Self {
        state.update.clone()
    }
}

impl FromRef<AppState> for ReportStore {
    fn from_ref(state: &AppState) -> Self {
        state.reports.clone()
    }
}

/// Bind `addr` and serve the API and embedded UI until the listener fails.
///
/// This is the web layer's composition root: it takes each shared store the
/// handlers need as its own argument, so the list is long by nature.
#[allow(clippy::too_many_arguments)]
pub async fn serve(
    addr: SocketAddr,
    store: Store,
    config: ConfigStore,
    blocker: Arc<Blocker>,
    lists: ListStore,
    rewrites: RewriteStore,
    clients: ClientStore,
    schedules: ScheduleStore,
    access: AccessStore,
    cache: DnsCache,
    users: UserStore,
    sessions: SessionStore,
    audit: AuditStore,
    reports_path: std::path::PathBuf,
    certs: Option<Arc<CertStore>>,
    cert_reload: Option<CertReload>,
    dot_port: u16,
    doh_port: u16,
    logs: LogBuffer,
    status: Arc<RuntimeStatus>,
    log_level: Arc<LogLevelControl>,
    metrics_token: Option<Arc<str>>,
) -> std::io::Result<()> {
    // The update checker reads GitHub (public tags) and mediates the on-disk
    // protocol the updater sidecar watches. The periodic check runs on its own
    // thread because the HTTP client is blocking; it is gated on the live setting,
    // so turning update checks off stops the outbound calls without a restart.
    let update = Arc::new(UpdateChecker::from_env(status.version()));
    {
        let update = update.clone();
        let config = config.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(15));
            loop {
                if config.get().update_checks {
                    update.refresh();
                }
                std::thread::sleep(Duration::from_secs(6 * 60 * 60));
            }
        });
    }

    // High-availability sync. The manager loads its node-local state from
    // `<data>/ha.json`; it is inert until the user enables HA and pairs a peer.
    let ha = Arc::new(ha::Ha::from_env(status.version()));

    // Generated summary reports, loaded from the data volume (empty on first run).
    let reports = ReportStore::load(&reports_path);

    let app_state = AppState {
        store,
        config,
        blocker,
        lists,
        rewrites,
        clients,
        schedules,
        access,
        cache,
        users,
        sessions,
        audit,
        throttle: LoginThrottle::new(),
        certs,
        cert_reload,
        dot_port,
        doh_port,
        logs,
        status,
        log_level,
        metrics_token,
        update,
        ha,
        reports,
    };

    // Fill the block-page server IP from this node's own cluster address when the
    // admin left it blank, so a blocked name points at the right node without
    // anyone typing the IP by hand. Node-local and idempotent (see the function).
    autofill_block_page_ips(&app_state);

    // The HA sync loop pulls each peer's snapshot, merges it and applies the
    // result — on its own blocking thread, like the update checker. It is gated on
    // HA being enabled with at least one peer, so turning HA off stops all peer
    // traffic without a restart. Each peer is synced independently, so one node
    // being down never blocks the sync among the rest (the mesh stays converged).
    {
        let state = app_state.clone();
        std::thread::spawn(move || {
            let interval = state.ha.interval();
            // A short warm-up so the stores and listeners are up before first sync.
            std::thread::sleep(Duration::from_secs(10));
            loop {
                if state.ha.enabled() {
                    sync_mesh(&state);
                }
                std::thread::sleep(interval);
            }
        });
    }

    // Query-log retention. Independent of HA: the durable DuckDB log is pruned to
    // the configured window (`log_retention_days`) so it does not grow without
    // bound. The prune is queued to the store's writer, serialised with the writes
    // so it never races the appender. Runs on its own thread, like the checkers.
    {
        let state = app_state.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(45));
            loop {
                let days = i64::from(state.config.get().log_retention_days.max(1));
                state.store.prune(now_ms() as i64 - days * MS_PER_DAY);
                std::thread::sleep(RETENTION_PRUNE_INTERVAL);
            }
        });
    }

    // Scheduled summary reports. Checks a few times an hour whether a daily or
    // weekly window has come due (in the instance's timezone) and, if so and it
    // is not already stored, generates it. Both cadences are off by default, so
    // this does nothing until the admin turns one on.
    {
        let state = app_state.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            loop {
                run_due_reports(&state);
                std::thread::sleep(REPORT_CHECK_INTERVAL);
            }
        });
    }

    // Active notifications. A small evaluator checks a few alert conditions
    // (a node going unreachable, an update available, a certificate nearing
    // expiry, a device's blocked-lookup spike) and posts to the configured
    // webhook. It reads the live config each pass, so it does nothing until the
    // admin turns notifications on — and uses the blocking `ureq` client, so it
    // runs on its own thread like the checkers above.
    {
        let state = app_state.clone();
        std::thread::spawn(move || {
            let evaluator = notify::AlertEvaluator::new(
                state.config.clone(),
                state.ha.clone(),
                state.update.clone(),
                state.certs.clone(),
                state.store.clone(),
            );
            std::thread::sleep(Duration::from_secs(60));
            loop {
                evaluator.evaluate();
                std::thread::sleep(ALERT_CHECK_INTERVAL);
            }
        });
    }

    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "serving web UI and API");
    axum::serve(listener, router(app_state)).await
}

fn router(state: AppState) -> Router {
    // The data API sits behind a login guard. The auth endpoints and the static
    // UI (so the login screen itself can load) stay public.
    let protected = Router::new()
        .route("/api/stats", get(stats))
        .route("/api/stats/day", get(stats_day))
        .route("/api/stats/top-domains", get(top_domains))
        .route("/api/stats/resolvers", get(resolvers))
        .route("/api/activity", get(activity))
        .route("/api/queries", get(queries))
        .route("/api/queries/history", get(queries_history))
        .route("/api/privacy/trackers", get(privacy_trackers))
        .route("/api/privacy/block", post(privacy_block))
        .route("/api/nodes", get(nodes))
        .route("/api/dnssec", get(dnssec))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/classify", get(classify))
        .route("/api/lists", get(get_lists))
        .route(
            "/api/lists/allow",
            post(add_allow).delete(remove_allow).put(set_allow_note),
        )
        .route(
            "/api/lists/block",
            post(add_block).delete(remove_block).put(set_block_note),
        )
        .route("/api/lists/subscriptions", post(add_subscription))
        .route(
            "/api/lists/subscriptions/{id}",
            put(set_subscription_enabled).delete(remove_subscription),
        )
        .route("/api/lists/presets/{key}", put(set_preset))
        .route("/api/lists/refresh", post(refresh_lists))
        .route("/api/services", get(get_services))
        .route("/api/services/{id}", put(set_service))
        .route(
            "/api/rewrites",
            get(get_rewrites)
                .post(set_rewrite)
                .put(edit_rewrite)
                .delete(remove_rewrite),
        )
        .route("/api/rewrites/enabled", put(set_rewrite_enabled))
        .route(
            "/api/aliases",
            post(set_alias).put(edit_alias).delete(remove_alias),
        )
        .route(
            "/api/clients",
            get(get_clients)
                .post(set_client)
                .put(edit_client)
                .delete(remove_client),
        )
        .route(
            "/api/groups",
            get(get_groups)
                .post(add_group)
                .put(edit_group)
                .delete(remove_group),
        )
        .route(
            "/api/schedules",
            get(get_schedules)
                .post(add_schedule)
                .put(edit_schedule)
                .delete(remove_schedule),
        )
        .route("/api/access-requests", get(list_access_requests))
        .route(
            "/api/access-requests/{id}/release",
            post(release_access_request),
        )
        .route(
            "/api/access-requests/{id}/reject",
            post(reject_access_request),
        )
        .route("/api/notifications/test", post(notifications_test))
        .route("/api/tls", get(get_tls).put(put_tls))
        .route("/api/certs", get(list_certs).post(create_cert))
        .route("/api/certs/{id}", delete(delete_cert))
        .route("/api/certs/{id}/renew", post(renew_cert))
        .route("/api/certs/{id}/ca", get(download_cert_ca))
        .route("/api/cache/clear", post(clear_cache))
        .route("/api/backup", post(export_backup))
        .route("/api/backup/restore", post(restore_backup))
        // Self-service: a signed-in user manages their own password and MFA.
        .route("/api/account", get(get_account))
        .route("/api/account/password", put(change_own_password))
        .route("/api/account/mfa/begin", post(account_mfa_begin))
        .route("/api/account/mfa/confirm", post(account_mfa_confirm))
        .route("/api/account/mfa/disable", post(account_mfa_disable))
        .route(
            "/api/account/mfa/recovery",
            post(account_mfa_regenerate_recovery),
        )
        // Administration: the account list and the audit trail (Admin only).
        .route("/api/users", get(get_users).post(add_user))
        .route("/api/users/{username}", delete(delete_user))
        .route("/api/users/{username}/role", put(set_user_role))
        .route("/api/users/{username}/password", put(reset_user_password))
        .route(
            "/api/users/{username}/mfa-required",
            put(set_user_mfa_required),
        )
        .route("/api/users/{username}/mfa", delete(reset_user_mfa))
        .route("/api/audit", get(get_audit))
        // System diagnostics: the operational log ring and the runtime snapshot.
        .route("/api/logs/system", get(system_logs))
        // Raise or lower the live log verbosity (temporary; not persisted).
        .route("/api/logs/level", get(get_log_level).put(set_log_level))
        .route("/api/diag", get(diag))
        // In-app update: check GitHub, trigger a one-click update via the sidecar,
        // and poll progress. The sidecar prunes old images on its own (it keeps
        // the new version and its predecessor), so there is no cleanup endpoint.
        .route("/api/update", get(get_update).post(start_update))
        .route("/api/update/check", post(check_update))
        .route("/api/update/status", get(update_status))
        // High-availability management (admin). The peer-to-peer endpoints
        // (`/api/ha/hello`, `/api/ha/snapshot`) carry their own key auth and live
        // in the public router instead.
        .route("/api/ha/status", get(ha_status))
        .route("/api/ha/enable", post(ha_enable))
        .route("/api/ha/key", put(ha_set_key))
        .route("/api/ha/key/reveal", get(ha_reveal_key))
        .route("/api/ha/address", put(ha_set_address))
        .route("/api/ha/peers", post(ha_connect).delete(ha_disconnect))
        .route("/api/ha/sync", post(ha_sync_now))
        // Scheduled summary reports (admin): list, view, generate now, download.
        .route("/api/reports", get(reports_list))
        .route("/api/reports/run", post(reports_run))
        .route("/api/reports/{id}", get(reports_get))
        .route("/api/reports/{id}/download", get(reports_download))
        .layer(from_fn_with_state(state.clone(), require_auth));

    let public = Router::new()
        // A lightweight, unauthenticated liveness probe for Docker / monitoring.
        .route("/api/health", get(health))
        // Prometheus scrape endpoint. It carries its own bearer-token check (a
        // scraper cannot hold a login session), so it stays outside the auth
        // guard; with no token configured it answers 404.
        .route("/metrics", get(metrics))
        // Machine-to-machine HA endpoints. They authenticate with the shared sync
        // key (a peer holds no login session), so they sit outside the auth guard;
        // they answer 404 until HA is enabled and paired.
        .route("/api/ha/hello", get(ha_hello))
        .route("/api/ha/snapshot", get(ha_snapshot))
        .route("/api/ha/depart", post(ha_depart))
        .route("/api/ha/queries", get(ha_queries))
        .route("/api/auth/state", get(auth_state))
        .route("/api/auth/setup", post(auth_setup))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        // Second step of a login that needs a factor beyond the password.
        .route("/api/auth/mfa", post(auth_mfa))
        .route("/api/auth/enroll/begin", post(auth_enroll_begin))
        .route("/api/auth/enroll/confirm", post(auth_enroll_confirm))
        .fallback(static_handler);

    protected.merge(public).with_state(state)
}

/// Who the current request is made by, injected by [`require_auth`] into the
/// request so handlers (and the audit log) can name the actor and check finer
/// permissions. Only ever present on a request that passed the guard.
#[derive(Clone)]
struct Identity {
    username: String,
    role: Role,
}

impl<S: Send + Sync> FromRequestParts<S> for Identity {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Identity>()
            .cloned()
            // The guard always inserts this, so its absence is a wiring bug, not
            // an unauthenticated request.
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

/// The minimum role an endpoint requires, from its method and path. Reads are
/// open to any signed-in user; content changes need Editor; settings, TLS,
/// backups, user management and the audit trail need Admin. Self-service account
/// endpoints are open to the holder (Viewer) and authorise the target inside the
/// handler.
fn required_role(method: &Method, path: &str) -> Role {
    // A user's own account: any signed-in user may reach it; it only ever acts
    // on the caller themselves.
    if path == "/api/account" || path.starts_with("/api/account/") {
        return Role::Viewer;
    }
    // Administration surfaces.
    if path == "/api/users"
        || path.starts_with("/api/users/")
        || path == "/api/audit"
        || path == "/api/backup"
        || path == "/api/backup/restore"
        || path.starts_with("/api/logs/")
        || path == "/api/diag"
        || path.starts_with("/api/update")
        || path.starts_with("/api/ha")
        || path.starts_with("/api/reports")
        || path.starts_with("/api/certs")
        || path.starts_with("/api/access-requests")
        || path.starts_with("/api/notifications")
    {
        return Role::Admin;
    }
    // Server-wide configuration and TLS: reading is fine for anyone, changing is
    // an administrator's job.
    let is_write = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    if (path == "/api/settings"
        || path.starts_with("/api/tls")
        || path.starts_with("/api/services"))
        && is_write
    {
        return Role::Admin;
    }
    // Any other change (lists, rewrites, devices, groups, schedules, cache) is an
    // editor's job; anything read-only is open to viewers.
    if is_write { Role::Editor } else { Role::Viewer }
}

/// Guard the data API. A request passes only when its session cookie names a
/// live, fully authenticated session **and** the user's role meets the
/// endpoint's [`required_role`]. A missing or pending session answers 401 (the
/// UI redirects to login); an authenticated user without the role answers 403.
/// The resolved [`Identity`] is attached to the request for the handlers.
async fn require_auth(
    State(state): State<AppState>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(username) = jar
        .get(SESSION_COOKIE)
        .and_then(|cookie| state.sessions.username_for(cookie.value()))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    // The account must still exist (it may have been deleted mid-session).
    let Some(role) = state.users.role_of(&username) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let needed = required_role(request.method(), request.uri().path());
    if !role.at_least(needed) {
        return StatusCode::FORBIDDEN.into_response();
    }

    request.extensions_mut().insert(Identity { username, role });
    next.run(request).await
}

/// Build the session cookie carrying `token`. Short-lived cookies are session
/// cookies in the browser; the server-side expiry is what really bounds a login.
fn session_cookie(token: String) -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE, token);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Strict);
    cookie.set_path("/");
    cookie
}

/// Whether the box has an admin yet, and who (if anyone) the caller is logged in
/// as. The UI reads this on load to choose between first-run setup, the login
/// screen and the dashboard.
#[derive(Serialize)]
struct AuthState {
    configured: bool,
    authenticated: bool,
    username: Option<String>,
    /// The signed-in user's role, so the UI can hide what they cannot use.
    /// `None` when not signed in.
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<Role>,
}

async fn auth_state(
    State(users): State<UserStore>,
    State(sessions): State<SessionStore>,
    jar: CookieJar,
) -> Json<AuthState> {
    let username = jar
        .get(SESSION_COOKIE)
        .and_then(|cookie| sessions.username_for(cookie.value()));
    let role = username.as_deref().and_then(|name| users.role_of(name));
    Json(AuthState {
        configured: users.is_configured(),
        authenticated: username.is_some(),
        username,
        role,
    })
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

/// Where a login stands after a step. `authenticated` means done; `mfa` means
/// the user must post a code to `/api/auth/mfa`; `enroll` means MFA is required
/// for them and they must set it up via `/api/auth/enroll/*` before access.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum LoginStatus {
    Authenticated,
    Mfa,
    Enroll,
}

/// What a login step returns: who, and whether another step is needed.
#[derive(Serialize)]
struct LoginResponse {
    username: String,
    status: LoginStatus,
}

/// A body carrying a single code (a TOTP or a recovery code).
#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

/// The one-time secret material shown to the user while enrolling in MFA. The
/// recovery codes and the secret are shown only here, once; only hashes and the
/// secret-for-verification are kept server-side.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MfaSetupPayload {
    secret: String,
    otpauth_uri: String,
    qr_svg: String,
    recovery_codes: Vec<String>,
}

/// Record an audit entry, ignoring a disk-write failure (the running instance
/// stays consistent; a lost line is not worth failing the request over).
fn record(state: &AppState, entry: AuditEntry) {
    let _ = state.audit.record(entry);
}

/// The session token from the cookie jar, if present.
fn cookie_token(jar: &CookieJar) -> Option<String> {
    jar.get(SESSION_COOKIE)
        .map(|cookie| cookie.value().to_owned())
}

/// The user and state of the session the cookie names, pending sessions
/// included — for the second-step login endpoints.
fn current_pending(state: &AppState, jar: &CookieJar) -> Option<(String, SessionState)> {
    state.sessions.lookup(&cookie_token(jar)?)
}

/// Whether MFA is required for `username` right now: the server-wide switch, or
/// the user's own flag.
fn effective_require_mfa(state: &AppState, username: &str) -> bool {
    state.config.get().require_mfa || state.users.mfa_required_flag(username)
}

/// The issuer label authenticator apps show for this box: the instance name, or
/// `JustDNS` if it is blank.
fn mfa_issuer(state: &AppState) -> String {
    let name = state.config.get().instance_name;
    if name.trim().is_empty() {
        "JustDNS".to_owned()
    } else {
        name
    }
}

/// Mint a fresh secret and recovery codes for `username`, store them unconfirmed,
/// and return the material to show the user once.
fn begin_enrollment(
    state: &AppState,
    username: &str,
) -> Result<MfaSetupPayload, (StatusCode, String)> {
    let secret = justdns_auth::mfa::generate_secret();
    let otpauth_uri =
        justdns_auth::mfa::otpauth_uri(&secret, &mfa_issuer(state), username).map_err(internal)?;
    let qr_svg = justdns_auth::mfa::qr_svg(&otpauth_uri).map_err(internal)?;
    let recovery_codes = justdns_auth::mfa::generate_recovery_codes();
    state
        .users
        .begin_mfa(username, &secret, &recovery_codes)
        .map_err(auth_error)?;
    Ok(MfaSetupPayload {
        secret,
        otpauth_uri,
        qr_svg,
        recovery_codes,
    })
}

/// Map an [`AuthError`] to an HTTP status and message.
fn auth_error(error: AuthError) -> (StatusCode, String) {
    let status = match &error {
        AuthError::AlreadyConfigured | AuthError::Duplicate(_) | AuthError::LastAdmin => {
            StatusCode::CONFLICT
        }
        AuthError::Invalid(_) => StatusCode::BAD_REQUEST,
        AuthError::Unknown(_) => StatusCode::NOT_FOUND,
        AuthError::Hash(_)
        | AuthError::Io { .. }
        | AuthError::Parse { .. }
        | AuthError::Decrypt { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string())
}

/// Wrap an internal error message as a 500.
fn internal(message: String) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

/// Create the first administrator on an unconfigured box and log them straight
/// in. Refused once an admin exists, so this cannot be used to take over.
async fn auth_setup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<LoginResponse>), (StatusCode, String)> {
    match state.users.create_admin(&creds.username, &creds.password) {
        Ok(()) => {
            let token = state.sessions.create(&creds.username);
            record(
                &state,
                AuditEntry::success(&creds.username, "setup.admin").target(&creds.username),
            );
            Ok((
                jar.add(session_cookie(token)),
                Json(LoginResponse {
                    username: creds.username,
                    status: LoginStatus::Authenticated,
                }),
            ))
        }
        Err(error) => Err(auth_error(error)),
    }
}

/// Step one of signing in: check the username and password. On success the
/// response says whether the login is finished or a second factor is still
/// needed (a code, or first setting MFA up). Repeated failures are throttled.
async fn auth_login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<LoginResponse>), (StatusCode, String)> {
    if state.throttle.is_locked(&creds.username) {
        record(
            &state,
            AuditEntry::failure(&creds.username, "login.throttled").target(&creds.username),
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts, try again in a few minutes".into(),
        ));
    }

    let Some(username) = state.users.verify(&creds.username, &creds.password) else {
        state.throttle.note_failure(&creds.username);
        record(
            &state,
            AuditEntry::failure(&creds.username, "login.failure")
                .target(&creds.username)
                .detail("password"),
        );
        return Err((
            StatusCode::UNAUTHORIZED,
            "invalid username or password".into(),
        ));
    };

    // The password was right. Does this user still need a second factor?
    if state.users.mfa_enabled(&username) {
        let token = state
            .sessions
            .create_pending(&username, SessionState::PendingMfa);
        return Ok((
            jar.add(session_cookie(token)),
            Json(LoginResponse {
                username,
                status: LoginStatus::Mfa,
            }),
        ));
    }
    if effective_require_mfa(&state, &username) {
        let token = state
            .sessions
            .create_pending(&username, SessionState::PendingEnroll);
        return Ok((
            jar.add(session_cookie(token)),
            Json(LoginResponse {
                username,
                status: LoginStatus::Enroll,
            }),
        ));
    }

    // No second factor needed — finish here.
    state.throttle.clear(&username);
    let token = state.sessions.create(&username);
    record(
        &state,
        AuditEntry::success(&username, "login.success").target(&username),
    );
    Ok((
        jar.add(session_cookie(token)),
        Json(LoginResponse {
            username,
            status: LoginStatus::Authenticated,
        }),
    ))
}

/// Step two of a login for a user with MFA enabled: accept a TOTP code or a
/// one-time recovery code, and on success promote the pending session.
async fn auth_mfa(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<CodeBody>,
) -> Result<(CookieJar, Json<LoginResponse>), (StatusCode, String)> {
    let Some((username, SessionState::PendingMfa)) = current_pending(&state, &jar) else {
        return Err((StatusCode::UNAUTHORIZED, "no pending login".into()));
    };
    if state.throttle.is_locked(&username) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts, try again in a few minutes".into(),
        ));
    }

    let code = body.code.trim();
    let method = if state.users.verify_totp(&username, code) {
        Some("mfa")
    } else if state.users.consume_recovery(&username, code) {
        Some("recovery")
    } else {
        None
    };
    let Some(method) = method else {
        state.throttle.note_failure(&username);
        record(
            &state,
            AuditEntry::failure(&username, "login.failure")
                .target(&username)
                .detail("mfa"),
        );
        return Err((StatusCode::UNAUTHORIZED, "invalid code".into()));
    };

    let token = cookie_token(&jar).expect("pending session had a cookie");
    state.sessions.promote(&token);
    state.throttle.clear(&username);
    record(
        &state,
        AuditEntry::success(&username, "login.success")
            .target(&username)
            .detail(method),
    );
    Ok((
        jar,
        Json(LoginResponse {
            username,
            status: LoginStatus::Authenticated,
        }),
    ))
}

/// Begin enrolling the pending user in MFA (when MFA is required but not yet set
/// up). Returns the secret, QR and recovery codes to show once.
async fn auth_enroll_begin(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<MfaSetupPayload>, (StatusCode, String)> {
    let Some((username, SessionState::PendingEnroll)) = current_pending(&state, &jar) else {
        return Err((StatusCode::UNAUTHORIZED, "no pending enrolment".into()));
    };
    Ok(Json(begin_enrollment(&state, &username)?))
}

/// Finish the pending user's enrolment: verify a code against the fresh secret,
/// confirm MFA and promote the session.
async fn auth_enroll_confirm(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<CodeBody>,
) -> Result<(CookieJar, Json<LoginResponse>), (StatusCode, String)> {
    let Some((username, SessionState::PendingEnroll)) = current_pending(&state, &jar) else {
        return Err((StatusCode::UNAUTHORIZED, "no pending enrolment".into()));
    };
    if !state.users.verify_totp(&username, body.code.trim()) {
        record(
            &state,
            AuditEntry::failure(&username, "mfa.enable").target(&username),
        );
        return Err((StatusCode::UNAUTHORIZED, "invalid code".into()));
    }
    state.users.confirm_mfa(&username).map_err(auth_error)?;
    let token = cookie_token(&jar).expect("pending session had a cookie");
    state.sessions.promote(&token);
    state.throttle.clear(&username);
    record(
        &state,
        AuditEntry::success(&username, "mfa.enable").target(&username),
    );
    record(
        &state,
        AuditEntry::success(&username, "login.success")
            .target(&username)
            .detail("enroll"),
    );
    Ok((
        jar,
        Json(LoginResponse {
            username,
            status: LoginStatus::Authenticated,
        }),
    ))
}

/// End the current session and clear the cookie. Always succeeds, even without a
/// valid session, so a stale cookie can always be cleared.
async fn auth_logout(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, StatusCode) {
    if let Some(token) = cookie_token(&jar) {
        if let Some(username) = state.sessions.username_for(&token) {
            record(
                &state,
                AuditEntry::success(&username, "logout").target(&username),
            );
        }
        state.sessions.remove(&token);
    }
    (
        jar.remove(Cookie::from(SESSION_COOKIE)),
        StatusCode::NO_CONTENT,
    )
}

// ---- Account self-service -------------------------------------------------

/// The signed-in user's own account summary.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountInfo {
    username: String,
    role: Role,
    mfa_enabled: bool,
    /// Whether MFA is required for this user (server-wide or individually), so
    /// the UI can tell them they may not turn it off.
    mfa_required: bool,
}

/// A freshly minted set of recovery codes, shown once.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryCodes {
    recovery_codes: Vec<String>,
}

async fn get_account(State(state): State<AppState>, identity: Identity) -> Json<AccountInfo> {
    Json(AccountInfo {
        mfa_enabled: state.users.mfa_enabled(&identity.username),
        mfa_required: effective_require_mfa(&state, &identity.username),
        username: identity.username,
        role: identity.role,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

/// Change one's own password after proving the current one.
async fn change_own_password(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<ChangePasswordBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !state
        .users
        .verify_password(&identity.username, &body.current_password)
    {
        record(
            &state,
            AuditEntry::failure(&identity.username, "account.password").target(&identity.username),
        );
        return Err((
            StatusCode::UNAUTHORIZED,
            "current password is incorrect".into(),
        ));
    }
    state
        .users
        .set_password(&identity.username, &body.new_password)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "account.password").target(&identity.username),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Begin enrolling oneself in MFA voluntarily. Returns the secret, QR and
/// recovery codes to show once; MFA is not active until confirmed.
async fn account_mfa_begin(
    State(state): State<AppState>,
    identity: Identity,
) -> Result<Json<MfaSetupPayload>, (StatusCode, String)> {
    Ok(Json(begin_enrollment(&state, &identity.username)?))
}

/// Confirm one's own enrolment with a code, turning MFA on.
async fn account_mfa_confirm(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<CodeBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !state
        .users
        .verify_totp(&identity.username, body.code.trim())
    {
        return Err((StatusCode::UNAUTHORIZED, "invalid code".into()));
    }
    state
        .users
        .confirm_mfa(&identity.username)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "mfa.enable").target(&identity.username),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// A body carrying just a password, to re-authorise a sensitive self-service
/// action.
#[derive(Deserialize)]
struct PasswordBody {
    password: String,
}

/// Turn one's own MFA off, unless it is required for the account.
async fn account_mfa_disable(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<PasswordBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    if effective_require_mfa(&state, &identity.username) {
        return Err((
            StatusCode::CONFLICT,
            "MFA is required for your account and cannot be turned off".into(),
        ));
    }
    if !state
        .users
        .verify_password(&identity.username, &body.password)
    {
        return Err((StatusCode::UNAUTHORIZED, "password is incorrect".into()));
    }
    state
        .users
        .clear_mfa(&identity.username)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "mfa.disable").target(&identity.username),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Replace one's own recovery codes, after proving the password.
async fn account_mfa_regenerate_recovery(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<PasswordBody>,
) -> Result<Json<RecoveryCodes>, (StatusCode, String)> {
    if !state.users.mfa_enabled(&identity.username) {
        return Err((StatusCode::CONFLICT, "MFA is not set up".into()));
    }
    if !state
        .users
        .verify_password(&identity.username, &body.password)
    {
        return Err((StatusCode::UNAUTHORIZED, "password is incorrect".into()));
    }
    let recovery_codes = justdns_auth::mfa::generate_recovery_codes();
    state
        .users
        .replace_recovery(&identity.username, &recovery_codes)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "mfa.recovery").target(&identity.username),
    );
    Ok(Json(RecoveryCodes { recovery_codes }))
}

// ---- Administration: users -------------------------------------------------

/// The account list for the admin's user-management view (no secrets).
async fn get_users(State(state): State<AppState>) -> Json<Vec<justdns_auth::UserInfo>> {
    Json(state.users.list())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewUserBody {
    username: String,
    password: String,
    role: Role,
    #[serde(default)]
    mfa_required: bool,
}

/// Create a new account with a role and an initial password.
async fn add_user(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<NewUserBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .users
        .add_user(&body.username, &body.password, body.role, body.mfa_required)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "user.create")
            .target(&body.username)
            .detail(format!("role={}", body.role.as_str())),
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RoleBody {
    role: Role,
}

/// Change a user's role (the store refuses to demote the last admin).
async fn set_user_role(
    State(state): State<AppState>,
    identity: Identity,
    Path(username): Path<String>,
    Json(body): Json<RoleBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .users
        .set_role(&username, body.role)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "user.role")
            .target(&username)
            .detail(format!("role={}", body.role.as_str())),
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewPasswordBody {
    new_password: String,
}

/// Reset a user's password (an admin action; the user is not asked for the old
/// one).
async fn reset_user_password(
    State(state): State<AppState>,
    identity: Identity,
    Path(username): Path<String>,
    Json(body): Json<NewPasswordBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .users
        .set_password(&username, &body.new_password)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "user.password").target(&username),
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct MfaRequiredBody {
    required: bool,
}

/// Require (or stop requiring) MFA for a single user.
async fn set_user_mfa_required(
    State(state): State<AppState>,
    identity: Identity,
    Path(username): Path<String>,
    Json(body): Json<MfaRequiredBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .users
        .set_mfa_required(&username, body.required)
        .map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "user.mfaRequired")
            .target(&username)
            .detail(if body.required { "on" } else { "off" }),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Clear a user's MFA (an admin reset after a lost device). If MFA is still
/// required for them, they must enrol again at next login.
async fn reset_user_mfa(
    State(state): State<AppState>,
    identity: Identity,
    Path(username): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.users.clear_mfa(&username).map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "user.mfaReset").target(&username),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Delete an account. You cannot delete your own, and the store refuses to
/// remove the last administrator.
async fn delete_user(
    State(state): State<AppState>,
    identity: Identity,
    Path(username): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if username == identity.username {
        return Err((
            StatusCode::CONFLICT,
            "you cannot delete your own account".into(),
        ));
    }
    state.users.remove_user(&username).map_err(auth_error)?;
    record(
        &state,
        AuditEntry::success(&identity.username, "user.delete").target(&username),
    );
    Ok(StatusCode::NO_CONTENT)
}

// ---- Administration: audit log ---------------------------------------------

#[derive(Deserialize)]
struct AuditParams {
    #[serde(default)]
    limit: Option<usize>,
}

/// The most recent audit entries, newest first.
async fn get_audit(
    State(state): State<AppState>,
    Query(params): Query<AuditParams>,
) -> Json<Vec<AuditEntry>> {
    let limit = params.limit.unwrap_or(200).min(justdns_audit::MAX_ENTRIES);
    Json(state.audit.recent(limit))
}

/// Explain why a name is on a block list: the broad category
/// (`"ads"`/`"malware"`/`"custom"`), the rule that matched, and — for a custom
/// block — the display name of the exact list that caught it (the user's own
/// list or a subscribed feed). Returns `null` when the name is on no list. Used
/// by the log's detail view to show why an entry was blocked.
#[derive(Serialize)]
struct ClassifyResponse {
    list: &'static str,
    rule: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Deserialize)]
struct ClassifyParams {
    name: String,
}

async fn classify(
    State(blocker): State<Arc<Blocker>>,
    Query(params): Query<ClassifyParams>,
) -> Json<Option<ClassifyResponse>> {
    Json(blocker.explain(&params.name).map(|hit| ClassifyResponse {
        list: hit.category.as_str(),
        rule: hit.rule,
        name: hit.list,
    }))
}

/// Translate a list-store error into an HTTP status and message.
fn list_error(error: ListError) -> (StatusCode, String) {
    let status = match &error {
        ListError::InvalidDomain(_) | ListError::InvalidUrl(_) => StatusCode::BAD_REQUEST,
        ListError::UnknownSubscription(_) | ListError::UnknownPreset(_) => StatusCode::NOT_FOUND,
        ListError::Io { .. } | ListError::Parse { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string())
}

/// Everything the dashboard's "Listen" view needs: the user's own allow and
/// block entries, their feed subscriptions, and the curated catalogue.
async fn get_lists(State(lists): State<ListStore>) -> Json<ListsView> {
    Json(lists.view())
}

/// A single domain, as the allow/block editors and the log buttons send it. The
/// optional note rides along on an add (POST) or a note edit (PUT); the log
/// buttons and the delete path simply omit it.
#[derive(Deserialize)]
struct DomainBody {
    domain: String,
    #[serde(default)]
    note: Option<String>,
}

/// Mutations return the fresh view so the UI can replace its state in one step.
async fn add_allow(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists
        .add_allow(&body.domain, body.note.as_deref())
        .map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn remove_allow(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.remove_allow(&body.domain).map_err(list_error)?;
    Ok(Json(lists.view()))
}

/// Set or clear the note on an existing allow entry.
async fn set_allow_note(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists
        .set_allow_note(&body.domain, body.note.as_deref())
        .map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn add_block(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists
        .add_block(&body.domain, body.note.as_deref())
        .map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn remove_block(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.remove_block(&body.domain).map_err(list_error)?;
    Ok(Json(lists.view()))
}

/// Set or clear the note on an existing block entry.
async fn set_block_note(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists
        .set_block_note(&body.domain, body.note.as_deref())
        .map_err(list_error)?;
    Ok(Json(lists.view()))
}

/// Subscribe to an external feed by URL.
#[derive(Deserialize)]
struct SubscriptionBody {
    name: String,
    url: String,
    kind: ListKind,
}

async fn add_subscription(
    State(lists): State<ListStore>,
    Json(body): Json<SubscriptionBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists
        .add_subscription(&body.name, &body.url, body.kind)
        .map_err(list_error)?;
    Ok(Json(lists.view()))
}

/// Pause or resume a subscription (a preset or a hand-added feed).
#[derive(Deserialize)]
struct EnabledBody {
    enabled: bool,
}

async fn set_subscription_enabled(
    State(lists): State<ListStore>,
    Path(id): Path<String>,
    Json(body): Json<EnabledBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists
        .set_subscription_enabled(&id, body.enabled)
        .map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn remove_subscription(
    State(lists): State<ListStore>,
    Path(id): Path<String>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.remove_subscription(&id).map_err(list_error)?;
    Ok(Json(lists.view()))
}

/// Switch a curated preset on or off.
#[derive(Deserialize)]
struct PresetBody {
    on: bool,
}

async fn set_preset(
    State(lists): State<ListStore>,
    Path(key): Path<String>,
    Json(body): Json<PresetBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.set_preset(&key, body.on).map_err(list_error)?;
    Ok(Json(lists.view()))
}

/// What a manual refresh returns: the per-feed outcomes and the fresh view.
#[derive(Serialize)]
struct RefreshResponse {
    outcomes: Vec<RefreshOutcome>,
    lists: ListsView,
}

/// Re-download every enabled feed now. The fetch is blocking, so it runs on the
/// blocking pool to keep the async runtime free.
async fn refresh_lists(
    State(lists): State<ListStore>,
) -> Result<Json<RefreshResponse>, (StatusCode, String)> {
    let worker = lists.clone();
    let outcomes = tokio::task::spawn_blocking(move || worker.refresh_all())
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(RefreshResponse {
        outcomes,
        lists: lists.view(),
    }))
}

/// Translate a rewrite-store error into an HTTP status and message.
fn rewrite_error(error: RewriteError) -> (StatusCode, String) {
    let status = match &error {
        RewriteError::InvalidName(_)
        | RewriteError::InvalidIp(_)
        | RewriteError::NoIps
        | RewriteError::InvalidAliasName(_)
        | RewriteError::UnknownAlias(_) => StatusCode::BAD_REQUEST,
        RewriteError::Unknown(_) => StatusCode::NOT_FOUND,
        // The object is still referenced, so the delete conflicts with live state;
        // the message names every rewrite that still points at it.
        RewriteError::AliasInUse { .. } => StatusCode::CONFLICT,
        RewriteError::Io { .. } | RewriteError::Parse { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string())
}

/// Every local DNS rewrite the dashboard shows.
async fn get_rewrites(State(rewrites): State<RewriteStore>) -> Json<RewritesView> {
    Json(rewrites.view())
}

/// Add one or more rewrites that share the same addresses. A single name is just
/// a list of one, so the add form and the "same IP for several names" case use
/// one endpoint.
#[derive(Deserialize)]
struct RewriteBody {
    names: Vec<String>,
    #[serde(default)]
    ips: Vec<String>,
    /// Names of the address objects the rewrite points at. Optional, so an
    /// address-only rewrite posts just `ips`.
    #[serde(default)]
    aliases: Vec<String>,
}

async fn set_rewrite(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<RewriteBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .set_many(&body.names, &body.ips, &body.aliases)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Rename a rewrite and replace its addresses and object references, keeping its
/// enabled state.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RewriteEditBody {
    old_name: String,
    name: String,
    #[serde(default)]
    ips: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
}

async fn edit_rewrite(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<RewriteEditBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .edit(&body.old_name, &body.name, &body.ips, &body.aliases)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Add an address object, or replace the addresses of one that already exists.
#[derive(Deserialize)]
struct AliasBody {
    name: String,
    ips: Vec<String>,
}

async fn set_alias(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<AliasBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .set_alias(&body.name, &body.ips)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Rename an address object and replace its addresses; references on every
/// rewrite that points at it follow to the new name.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AliasEditBody {
    old_name: String,
    name: String,
    ips: Vec<String>,
}

async fn edit_alias(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<AliasEditBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .edit_alias(&body.old_name, &body.name, &body.ips)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Remove an address object by name. Refused with 409 while a rewrite still
/// points at it.
#[derive(Deserialize)]
struct AliasNameBody {
    name: String,
}

async fn remove_alias(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<AliasNameBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites.remove_alias(&body.name).map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Translate a client-store error into an HTTP status and message.
fn client_error(error: ClientError) -> (StatusCode, String) {
    let status = match &error {
        ClientError::InvalidTarget(_)
        | ClientError::InvalidName
        | ClientError::InvalidGroupName
        | ClientError::UnknownGroup(_) => StatusCode::BAD_REQUEST,
        ClientError::Unknown(_) => StatusCode::NOT_FOUND,
        ClientError::Io { .. } | ClientError::Parse { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string())
}

/// One row of the devices view: a named device or a seen-but-unnamed client,
/// with its recent footprint. For a named device, `target` is its address or
/// range and `name`/`note`/`kind` describe it; for an unnamed client, `target`
/// is the bare address it was seen from and `name` is absent. `known` tells the
/// two apart so the UI offers "edit" for one and "name this" for the other.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientRow {
    target: String,
    is_range: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    kind: DeviceKind,
    known: bool,
    count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen_ms: Option<i64>,
    active: bool,
    /// The id of the group this device belongs to, if any. Absent for unnamed
    /// clients and ungrouped devices.
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<String>,
    /// This device's own filtering delta. Always present for a named device (empty
    /// when it has none), so the editor can read and write it uniformly.
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<PolicyDelta>,
}

/// One entry in the list catalogue the policy editor offers: a switchable block
/// object, with whether it is on in the global default. Covers the two built-in
/// objects (`ads`, `malware`) and every block feed subscription.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListObjectRow {
    id: String,
    label: String,
    /// Whether this object is on in the global default (so a scope that neither
    /// adds nor disables it inherits this state).
    global_on: bool,
}

/// One blockable service in the catalogue the dashboard offers, with whether it
/// is on in the global default. The `id` is the list-object id (`svc:<id>`), so
/// a group or device toggles it through the same policy delta as any list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRow {
    /// Bare machine id (`tiktok`); the path param of `PUT /api/services/{id}`.
    id: String,
    /// List-object id (`svc:tiktok`) to write into a `PolicyDelta` for per-scope
    /// (group / device / schedule) toggles.
    list_id: String,
    name: String,
    category: String,
    global_on: bool,
}

/// The whole blockable-services catalogue, each entry marked with its global
/// default state (from `settings.blocked_services`).
fn service_catalog(config: &ConfigStore) -> Vec<ServiceRow> {
    let on = config.get().blocked_services;
    justdns_filter::services()
        .iter()
        .map(|service| ServiceRow {
            id: service.id.to_owned(),
            list_id: service.object_id(),
            name: service.name.to_owned(),
            category: service.category.to_owned(),
            global_on: on.iter().any(|bare| bare == service.id),
        })
        .collect()
}

/// The blockable-services catalogue with each service's global-default state.
async fn get_services(State(config): State<ConfigStore>) -> Json<Vec<ServiceRow>> {
    Json(service_catalog(&config))
}

/// Turn a service on or off in the global default.
#[derive(Deserialize)]
struct ServiceToggle {
    on: bool,
}

/// Switch a blockable service on or off in the global default (Admin). The path
/// id is the bare catalogue id (`tiktok`); a group or device can still override
/// it for itself. Returns the fresh catalogue.
async fn set_service(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Json(body): Json<ServiceToggle>,
) -> Result<Json<Vec<ServiceRow>>, (StatusCode, String)> {
    if justdns_filter::service(&id).is_none() {
        return Err((StatusCode::NOT_FOUND, format!("unbekannter Dienst {id}")));
    }
    let mut settings = state.config.get();
    let present = settings.blocked_services.iter().any(|bare| bare == &id);
    if body.on && !present {
        settings.blocked_services.push(id.clone());
    } else if !body.on {
        settings.blocked_services.retain(|bare| bare != &id);
    }
    state
        .config
        .set(settings)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    record(
        &state,
        AuditEntry::success(&identity.username, "services.update").target(&id),
    );
    Ok(Json(service_catalog(&state.config)))
}

/// Everything the devices view needs: the rows (named devices plus recently seen
/// unnamed clients), the group definitions, the list catalogue the policy editor
/// draws from, and the active-window the UI shows and the rows were aged by.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientsView {
    clients: Vec<ClientRow>,
    groups: Vec<Group>,
    lists: Vec<ListObjectRow>,
    /// The blockable-services catalogue, so the per-device/group editor can show
    /// a service grid toggled through the same policy delta as the lists.
    services: Vec<ServiceRow>,
    active_window_secs: u32,
}

/// Build the devices view: start a row for every stored device, fold each
/// recently seen client's count and last-seen into the device that names it (or
/// into its own unnamed row), then mark as active anything seen within the
/// window. Rows are ordered most-recently-seen first, with never-seen named
/// devices after, so the list reads like "what's on the network now".
fn clients_view(
    store: &Store,
    clients: &ClientStore,
    config: &ConfigStore,
    lists: &ListStore,
) -> ClientsView {
    let devices = clients.view();
    let resolver = ClientResolver::new(devices.clone());
    let settings = config.get();
    let active_window_secs = settings.active_window_secs;
    let cutoff = now_ms() as i64 - i64::from(active_window_secs) * 1000;

    // The list catalogue the policy editor draws from: the two built-in objects
    // followed by every block feed, each with its global-default state so the UI
    // can show what an untouched scope inherits.
    let resolved = lists.resolved();
    let mut list_catalog = vec![
        ListObjectRow {
            id: "ads".to_owned(),
            label: "Werbung & Tracker".to_owned(),
            global_on: settings.block_ads_trackers,
        },
        ListObjectRow {
            id: "malware".to_owned(),
            label: "Schadsoftware".to_owned(),
            global_on: settings.block_malware,
        },
    ];
    list_catalog.extend(resolved.block_feeds.iter().map(|feed| ListObjectRow {
        id: feed.id.clone(),
        label: feed.label.clone(),
        global_on: feed.enabled,
    }));

    let mut rows: Vec<ClientRow> = Vec::new();
    // A stored device's row, by target, and a seen unnamed client's row, by the
    // address text — so repeated sightings fold into one row each.
    let mut device_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut unnamed_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for device in &devices {
        let is_range = parse_target(&device.target).is_some_and(|m| m.is_range());
        device_index.insert(device.target.clone(), rows.len());
        rows.push(ClientRow {
            target: device.target.clone(),
            is_range,
            name: Some(device.name.clone()),
            note: device.note.clone(),
            kind: device.kind,
            known: true,
            count: 0,
            last_seen_ms: None,
            active: false,
            group: device.group.clone(),
            policy: Some(device.policy.clone()),
        });
    }

    for activity in store.client_activity() {
        match resolver.resolve(activity.client) {
            Some(device) => {
                let row = &mut rows[device_index[&device.target]];
                row.count += activity.count;
                row.last_seen_ms = Some(row.last_seen_ms.map_or(activity.last_seen_ms, |seen| {
                    seen.max(activity.last_seen_ms)
                }));
            }
            None => {
                let key = activity.client.to_string();
                match unnamed_index.get(&key) {
                    Some(&pos) => {
                        let row = &mut rows[pos];
                        row.count += activity.count;
                        row.last_seen_ms =
                            Some(row.last_seen_ms.map_or(activity.last_seen_ms, |seen| {
                                seen.max(activity.last_seen_ms)
                            }));
                    }
                    None => {
                        unnamed_index.insert(key.clone(), rows.len());
                        rows.push(ClientRow {
                            target: key,
                            is_range: false,
                            name: None,
                            note: None,
                            kind: DeviceKind::Other,
                            known: false,
                            count: activity.count,
                            last_seen_ms: Some(activity.last_seen_ms),
                            active: false,
                            group: None,
                            policy: None,
                        });
                    }
                }
            }
        }
    }

    for row in &mut rows {
        row.active = row.last_seen_ms.is_some_and(|seen| seen >= cutoff);
    }
    // Most recently seen first (never-seen named devices, last_seen None, sort to
    // the end); ties broken by target for a stable order.
    rows.sort_by(|a, b| {
        b.last_seen_ms
            .unwrap_or(i64::MIN)
            .cmp(&a.last_seen_ms.unwrap_or(i64::MIN))
            .then_with(|| a.target.cmp(&b.target))
    });

    ClientsView {
        clients: rows,
        groups: clients.groups(),
        lists: list_catalog,
        services: service_catalog(config),
        active_window_secs,
    }
}

/// The devices view: named devices and recently seen unnamed clients.
async fn get_clients(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
) -> Json<ClientsView> {
    Json(clients_view(&store, &clients, &config, &lists))
}

/// The policy part of a device write: its group and its own delta. Optional on
/// the body — when omitted, a device's existing group and delta are left
/// untouched (so an older UI that only renames a device keeps its policy).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevicePolicyInput {
    /// The group to put the device in, or `null`/absent for ungrouped.
    #[serde(default)]
    group: Option<String>,
    /// The device's own delta, layered on its group and the global default.
    #[serde(flatten)]
    delta: PolicyDelta,
}

/// Add a device, or replace the name/note/kind of one that already matches the
/// same target. When `policy` is present, its group and delta are applied too.
#[derive(Deserialize)]
struct ClientBody {
    target: String,
    name: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    kind: DeviceKind,
    #[serde(default)]
    policy: Option<DevicePolicyInput>,
}

async fn set_client(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    Json(body): Json<ClientBody>,
) -> Result<Json<ClientsView>, (StatusCode, String)> {
    clients
        .set(&body.target, &body.name, body.note.as_deref(), body.kind)
        .map_err(client_error)?;
    if let Some(policy) = body.policy {
        clients
            .set_device_policy(&body.target, policy.group.as_deref(), policy.delta)
            .map_err(client_error)?;
    }
    Ok(Json(clients_view(&store, &clients, &config, &lists)))
}

/// Re-point a device at a new target and replace its name/note/kind, keyed by
/// its old target. When `policy` is present, its group and delta are applied to
/// the device at its new target.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientEditBody {
    old_target: String,
    target: String,
    name: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    kind: DeviceKind,
    #[serde(default)]
    policy: Option<DevicePolicyInput>,
}

async fn edit_client(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    Json(body): Json<ClientEditBody>,
) -> Result<Json<ClientsView>, (StatusCode, String)> {
    clients
        .edit(
            &body.old_target,
            &body.target,
            &body.name,
            body.note.as_deref(),
            body.kind,
        )
        .map_err(client_error)?;
    if let Some(policy) = body.policy {
        clients
            .set_device_policy(&body.target, policy.group.as_deref(), policy.delta)
            .map_err(client_error)?;
    }
    Ok(Json(clients_view(&store, &clients, &config, &lists)))
}

/// Remove a device by its target.
#[derive(Deserialize)]
struct ClientTargetBody {
    target: String,
}

async fn remove_client(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    Json(body): Json<ClientTargetBody>,
) -> Result<Json<ClientsView>, (StatusCode, String)> {
    clients.remove(&body.target).map_err(client_error)?;
    Ok(Json(clients_view(&store, &clients, &config, &lists)))
}

/// The devices/groups view, groups alone — returned by the group endpoints so the
/// UI refreshes the whole picture after a group change.
async fn get_groups(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
) -> Json<ClientsView> {
    Json(clients_view(&store, &clients, &config, &lists))
}

/// Create a group with the given name and delta.
#[derive(Deserialize)]
struct GroupBody {
    name: String,
    #[serde(flatten)]
    policy: PolicyDelta,
}

async fn add_group(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    Json(body): Json<GroupBody>,
) -> Result<Json<ClientsView>, (StatusCode, String)> {
    clients
        .add_group(&body.name, body.policy)
        .map_err(client_error)?;
    Ok(Json(clients_view(&store, &clients, &config, &lists)))
}

/// Rename a group and replace its delta, keyed by its id.
#[derive(Deserialize)]
struct GroupEditBody {
    id: String,
    name: String,
    #[serde(flatten)]
    policy: PolicyDelta,
}

async fn edit_group(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    Json(body): Json<GroupEditBody>,
) -> Result<Json<ClientsView>, (StatusCode, String)> {
    clients
        .edit_group(&body.id, &body.name, body.policy)
        .map_err(client_error)?;
    Ok(Json(clients_view(&store, &clients, &config, &lists)))
}

/// Delete a group by id; devices that referenced it become ungrouped.
#[derive(Deserialize)]
struct GroupIdBody {
    id: String,
}

async fn remove_group(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    Json(body): Json<GroupIdBody>,
) -> Result<Json<ClientsView>, (StatusCode, String)> {
    clients.remove_group(&body.id).map_err(client_error)?;
    Ok(Json(clients_view(&store, &clients, &config, &lists)))
}

/// Everything the schedules view shows: just the list of schedules. The target
/// group/device catalogue the editor needs is the devices view (`/api/clients`).
#[derive(Serialize)]
struct SchedulesView {
    schedules: Vec<Schedule>,
}

/// List every schedule.
async fn get_schedules(State(schedules): State<ScheduleStore>) -> Json<SchedulesView> {
    Json(SchedulesView {
        schedules: schedules.view(),
    })
}

/// Create a schedule from a draft (no id — the store assigns one).
async fn add_schedule(
    State(schedules): State<ScheduleStore>,
    State(clients): State<ClientStore>,
    Json(draft): Json<ScheduleDraft>,
) -> Result<Json<SchedulesView>, (StatusCode, String)> {
    validate_schedule_target(&draft.target, &clients)?;
    schedules.add(draft).map_err(schedule_error)?;
    Ok(Json(SchedulesView {
        schedules: schedules.view(),
    }))
}

/// Replace a schedule, keyed by its id.
async fn edit_schedule(
    State(schedules): State<ScheduleStore>,
    State(clients): State<ClientStore>,
    Json(body): Json<Schedule>,
) -> Result<Json<SchedulesView>, (StatusCode, String)> {
    validate_schedule_target(&body.target, &clients)?;
    let id = body.id.clone();
    schedules
        .edit(&id, ScheduleDraft::from(body))
        .map_err(schedule_error)?;
    Ok(Json(SchedulesView {
        schedules: schedules.view(),
    }))
}

/// Delete a schedule by id.
#[derive(Deserialize)]
struct ScheduleIdBody {
    id: String,
}

async fn remove_schedule(
    State(schedules): State<ScheduleStore>,
    Json(body): Json<ScheduleIdBody>,
) -> Result<Json<SchedulesView>, (StatusCode, String)> {
    schedules.remove(&body.id).map_err(schedule_error)?;
    Ok(Json(SchedulesView {
        schedules: schedules.view(),
    }))
}

/// A group-scoped schedule must name a group that exists; a device target's form
/// is validated by the store. Keeps a schedule from pointing at a missing group.
fn validate_schedule_target(
    target: &ScheduleTarget,
    clients: &ClientStore,
) -> Result<(), (StatusCode, String)> {
    if target.scope == TargetScope::Group {
        if let Some(id) = &target.id {
            if !clients.groups().iter().any(|group| &group.id == id) {
                return Err((StatusCode::BAD_REQUEST, format!("no group with id '{id}'")));
            }
        }
    }
    Ok(())
}

/// Map a schedule-store error to an HTTP status: not-found for an unknown id,
/// server error for IO/parse trouble, bad-request for a rejected draft.
fn schedule_error(error: ScheduleError) -> (StatusCode, String) {
    match error {
        ScheduleError::Unknown(_) => (StatusCode::NOT_FOUND, error.to_string()),
        ScheduleError::Io { .. } | ScheduleError::Parse { .. } => {
            (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
        _ => (StatusCode::BAD_REQUEST, error.to_string()),
    }
}

/// Switch a rewrite on or off by name.
#[derive(Deserialize)]
struct RewriteEnabledBody {
    name: String,
    enabled: bool,
}

async fn set_rewrite_enabled(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<RewriteEnabledBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .set_enabled(&body.name, body.enabled)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Remove a rewrite by name.
#[derive(Deserialize)]
struct RewriteNameBody {
    name: String,
}

async fn remove_rewrite(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<RewriteNameBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites.remove(&body.name).map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Drop every entry in the DNS response cache. Returns 204 on success.
async fn clear_cache(State(cache): State<DnsCache>) -> StatusCode {
    cache.clear();
    StatusCode::NO_CONTENT
}

/// The state of the encrypted client-DNS endpoints, for the settings view.
/// `available` is false when TLS could not be prepared, which disables the
/// section in the UI. The certificates themselves live in the certificate store
/// (`/api/certs`); here DoT and DoH only record which one each selects.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TlsStatus {
    available: bool,
    dot_enabled: bool,
    doh_enabled: bool,
    hostname: String,
    /// The certificate id DoT serves (empty string = the store default).
    dot_cert_id: String,
    /// The certificate id DoH serves (empty string = the store default).
    doh_cert_id: String,
    dot_port: u16,
    doh_port: u16,
}

/// Build the encrypted-DNS status from the live settings.
fn tls_status(state: &AppState) -> TlsStatus {
    let settings = state.config.get().encrypted_dns;
    TlsStatus {
        available: state.certs.is_some(),
        dot_enabled: settings.dot_enabled,
        doh_enabled: settings.doh_enabled,
        hostname: settings.hostname,
        dot_cert_id: settings.dot_cert_id.unwrap_or_default(),
        doh_cert_id: settings.doh_cert_id.unwrap_or_default(),
        dot_port: state.dot_port,
        doh_port: state.doh_port,
    }
}

/// The current encrypted-DNS status for the settings view.
async fn get_tls(State(state): State<AppState>) -> Json<TlsStatus> {
    Json(tls_status(&state))
}

/// Set which endpoints are on, the hostname and which stored certificate each
/// serves. Persisted and applied live by the binary's supervisor (start/stop the
/// listeners, swap the served certificate).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TlsSettingsBody {
    dot_enabled: bool,
    doh_enabled: bool,
    hostname: String,
    #[serde(default)]
    dot_cert_id: Option<String>,
    #[serde(default)]
    doh_cert_id: Option<String>,
}

async fn put_tls(
    State(state): State<AppState>,
    Json(body): Json<TlsSettingsBody>,
) -> Result<Json<TlsStatus>, (StatusCode, String)> {
    if state.certs.is_none() {
        return Err(certs_unavailable());
    }
    let mut settings = state.config.get();
    settings.encrypted_dns.dot_enabled = body.dot_enabled;
    settings.encrypted_dns.doh_enabled = body.doh_enabled;
    settings.encrypted_dns.hostname = body.hostname.trim().to_owned();
    settings.encrypted_dns.dot_cert_id = blank_to_none(body.dot_cert_id);
    settings.encrypted_dns.doh_cert_id = blank_to_none(body.doh_cert_id);
    state
        .config
        .set(settings)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(tls_status(&state)))
}

/// A stored certificate as the dashboard sees it: the store's derived facts plus
/// which features currently select it (so the UI can mark it "in use" and warn
/// before a delete).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CertListItem {
    #[serde(flatten)]
    info: CertInfo,
    /// Feature ids that select this certificate: `dot`, `doh`, `blockPage`.
    used_by: Vec<String>,
}

/// A PEM chain + key body, for an upload or a renewal.
#[derive(Deserialize)]
struct CertKeyBody {
    cert: String,
    key: String,
}

/// Create a certificate: upload a chain + key (both PEM) when `cert` and `key`
/// are given, otherwise mint a self-signed one for `hostname` (falling back to
/// the encrypted-DNS hostname). The certificate is named `name`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCertBody {
    name: String,
    #[serde(default)]
    cert: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    hostname: Option<String>,
}

/// The error returned when TLS could not be prepared on this instance.
fn certs_unavailable() -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "encrypted DNS is unavailable on this instance".to_owned(),
    )
}

/// Treat an empty selection as "no selection" (the store default).
fn blank_to_none(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// Which features select the certificate `id`, accounting for an empty selection
/// meaning the store default.
fn cert_usage(settings: &Settings, id: &str, default_id: Option<&str>) -> Vec<String> {
    let selects = |choice: &Option<String>| match choice.as_deref() {
        Some(chosen) if !chosen.is_empty() => chosen == id,
        _ => default_id == Some(id),
    };
    let mut used = Vec::new();
    if selects(&settings.encrypted_dns.dot_cert_id) {
        used.push("dot".to_owned());
    }
    if selects(&settings.encrypted_dns.doh_cert_id) {
        used.push("doh".to_owned());
    }
    if settings.block_page.enabled && selects(&settings.block_page.cert_id) {
        used.push("blockPage".to_owned());
    }
    used
}

/// The full catalogue with usage, for the certificates page and the selectors.
fn cert_list(state: &AppState) -> Result<Vec<CertListItem>, (StatusCode, String)> {
    let store = state.certs.clone().ok_or_else(certs_unavailable)?;
    let settings = state.config.get();
    let default_id = store.default_id();
    Ok(store
        .list()
        .into_iter()
        .map(|info| {
            let used_by = cert_usage(&settings, &info.id, default_id.as_deref());
            CertListItem { info, used_by }
        })
        .collect())
}

/// List the stored certificates.
async fn list_certs(
    State(state): State<AppState>,
) -> Result<Json<Vec<CertListItem>>, (StatusCode, String)> {
    Ok(Json(cert_list(&state)?))
}

/// Create a certificate (upload or mint) and return the refreshed catalogue.
async fn create_cert(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<CreateCertBody>,
) -> Result<Json<Vec<CertListItem>>, (StatusCode, String)> {
    let store = state.certs.clone().ok_or_else(certs_unavailable)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "a name is required".to_owned()));
    }
    let (id, action) = match (body.cert, body.key) {
        (Some(cert), Some(key)) => {
            let id = store
                .upload(name, &cert, &key)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            (id, "certs.upload")
        }
        _ => {
            let hostname = body.hostname.unwrap_or_default();
            let hostname = if hostname.trim().is_empty() {
                state.config.get().encrypted_dns.hostname
            } else {
                hostname
            };
            let id = store
                .generate(name, hostname.trim(), &[])
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            (id, "certs.generate")
        }
    };
    record(
        &state,
        AuditEntry::success(&identity.username, action).target(&id),
    );
    Ok(Json(cert_list(&state)?))
}

/// Replace a certificate in place with an uploaded, currently-valid chain + key.
async fn renew_cert(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Json(body): Json<CertKeyBody>,
) -> Result<Json<Vec<CertListItem>>, (StatusCode, String)> {
    let store = state.certs.clone().ok_or_else(certs_unavailable)?;
    store
        .renew(&id, &body.cert, &body.key)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    // A renewed certificate must be served live by whatever selected it.
    if let Some(reload) = &state.cert_reload {
        reload();
    }
    record(
        &state,
        AuditEntry::success(&identity.username, "certs.renew").target(&id),
    );
    Ok(Json(cert_list(&state)?))
}

/// Remove a certificate. Refused while any feature still selects it.
async fn delete_cert(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> Result<Json<Vec<CertListItem>>, (StatusCode, String)> {
    let store = state.certs.clone().ok_or_else(certs_unavailable)?;
    let settings = state.config.get();
    let used_by = cert_usage(&settings, &id, store.default_id().as_deref());
    if !used_by.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            format!("certificate is in use by {}", used_by.join(", ")),
        ));
    }
    store
        .delete(&id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    if let Some(reload) = &state.cert_reload {
        reload();
    }
    record(
        &state,
        AuditEntry::success(&identity.username, "certs.delete").target(&id),
    );
    Ok(Json(cert_list(&state)?))
}

/// Download a minted certificate's local CA (PEM) so clients can trust it. An
/// uploaded certificate has no CA and returns 404.
async fn download_cert_ca(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let store = state.certs.clone().ok_or_else(certs_unavailable)?;
    match store
        .ca_pem(&id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    {
        Some(pem) => Ok((
            [
                (header::CONTENT_TYPE, "application/x-pem-file".to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"justdns-ca.pem\"".to_owned(),
                ),
            ],
            pem,
        )
            .into_response()),
        None => Err((
            StatusCode::NOT_FOUND,
            "no local CA — the certificate was uploaded".to_owned(),
        )),
    }
}

/// The marker a backup file carries so a restore can tell it is ours.
const BACKUP_APP: &str = "justdns";
/// The file kind, in case JustDNS ever exports something else one day.
const BACKUP_KIND: &str = "backup";
/// The backup format version. Bumped only on a breaking shape change, so a
/// restore can refuse a file it does not understand.
const BACKUP_VERSION: u32 = 1;

/// A full configuration backup: everything the user sets in the UI that lives
/// on the data volume — their settings, lists and rewrites — in one versioned
/// envelope. The query log and the feed caches are left out: they are history
/// and regenerable, not setup. The admin account is left out too, so the file
/// carries no credentials. `app`/`kind`/`version` let a restore reject a foreign
/// or future-format file before it touches anything.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Backup {
    app: String,
    kind: String,
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at_ms: Option<u64>,
    settings: Settings,
    lists: UserLists,
    rewrites: Vec<Rewrite>,
    /// Reusable address objects the rewrites can reference. Defaults to empty so
    /// a backup written by an older build (version 1 without this field) still
    /// restores.
    #[serde(default)]
    aliases: Vec<Alias>,
    /// Named clients (devices). Defaults to empty so a backup written by an older
    /// build (version 1 without this field) still restores.
    #[serde(default)]
    clients: Vec<Device>,
    /// Device groups and their policy deltas. Defaults to empty so a backup
    /// written before groups existed (version 1 without this field) still
    /// restores; devices in it simply resolve to the global default.
    #[serde(default)]
    groups: Vec<Group>,
    /// Schedules (recurring time windows). Defaults to empty so a backup written
    /// before schedules existed (version 1 without this field) still restores.
    #[serde(default)]
    schedules: Vec<Schedule>,
}

/// The download-backup request body: the password the file is encrypted with.
/// Required — a backup leaves the server and must never be readable in the clear.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportRequest {
    password: String,
}

/// Export the whole UI-set configuration as one **password-encrypted** download.
/// The password is mandatory: a downloaded backup may be stored anywhere, so it is
/// sealed (Argon2id + ChaCha20-Poly1305) and is useless without the password. The
/// `Content-Disposition` header makes the browser save it.
async fn export_backup(
    State(state): State<AppState>,
    Json(request): Json<ExportRequest>,
) -> Result<Response, (StatusCode, String)> {
    if request.password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "a backup password is required".to_owned(),
        ));
    }
    let mut backup = build_backup(&state);
    // The write-only webhook credential is stripped even though the file is
    // encrypted, so the password alone never reveals it; the admin re-enters it
    // after a restore. (The on-disk pre-update backup keeps it.)
    redact_settings_secrets(&mut backup.settings);
    let json = serde_json::to_vec_pretty(&backup)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let sealed = justdns_secret::seal_backup(&request.password, &json);
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"justdns-backup.jdbak\"".to_owned(),
            ),
        ],
        sealed,
    )
        .into_response())
}

/// Restore a backup produced by [`export_backup`]. The envelope is checked
/// first, so a foreign or future-format file is refused with 400 before
/// anything is written. The rewrites are applied first because they are
/// validated as they are stored — an invalid entry rejects the restore before
/// the lists or settings are touched. Each piece takes effect live, without a
/// restart; the admin account and the query log are left untouched.
/// The restore request: the backup file's raw bytes (base64) and, for an encrypted
/// backup, its password. A legacy plaintext backup restores with no password.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreRequest {
    /// The raw backup file, base64-encoded (it may be encrypted binary).
    file: String,
    #[serde(default)]
    password: Option<String>,
}

async fn restore_backup(
    State(state): State<AppState>,
    identity: Identity,
    Json(request): Json<RestoreRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(request.file.trim())
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "the backup file is unreadable".to_owned(),
            )
        })?;
    // An encrypted backup needs its password; a legacy plaintext backup restores
    // as-is, so old exports keep working.
    let json_bytes = if justdns_secret::is_backup_encrypted(&raw) {
        let password = request.password.unwrap_or_default();
        if password.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "this backup is encrypted — enter its password".to_owned(),
            ));
        }
        justdns_secret::open_backup(&password, &raw).map_err(|error| match error {
            justdns_secret::BackupError::WrongPassword => {
                (StatusCode::BAD_REQUEST, "wrong backup password".to_owned())
            }
            other => (StatusCode::BAD_REQUEST, other.to_string()),
        })?
    } else {
        raw
    };
    let backup: Backup = serde_json::from_slice(&json_bytes).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "this is not a JustDNS backup file".to_owned(),
        )
    })?;

    let (config, lists, rewrites, clients, schedules) = (
        &state.config,
        &state.lists,
        &state.rewrites,
        &state.clients,
        &state.schedules,
    );
    if backup.app != BACKUP_APP || backup.kind != BACKUP_KIND {
        return Err((
            StatusCode::BAD_REQUEST,
            "this is not a JustDNS backup file".to_owned(),
        ));
    }
    if backup.version != BACKUP_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unsupported backup version {}", backup.version),
        ));
    }

    // Snapshot every store before touching it, so a failure partway through rolls
    // back to the pre-restore state instead of leaving a half-applied mix (e.g. new
    // rewrites and devices but the old lists and settings).
    let prev_rewrites = rewrites.snapshot();
    let prev_aliases = rewrites.alias_snapshot();
    let prev_clients = clients.snapshot();
    let prev_groups = clients.snapshot_groups();
    let prev_schedules = schedules.snapshot();
    let prev_lists = lists.snapshot();
    let prev_settings = config.get();

    let applied = (move || -> Result<(), (StatusCode, String)> {
        rewrites
            .replace_all(backup.rewrites, backup.aliases)
            .map_err(rewrite_error)?;
        clients
            .replace(backup.clients, backup.groups)
            .map_err(client_error)?;
        schedules
            .replace(backup.schedules)
            .map_err(schedule_error)?;
        lists.replace(backup.lists).map_err(list_error)?;
        config
            .set(backup.settings)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        Ok(())
    })();

    if let Err(error) = applied {
        // Best-effort rollback to the snapshots taken above. Any store already
        // swapped is put back; one that never changed is simply re-set to itself.
        let _ = rewrites.replace_all(prev_rewrites, prev_aliases);
        let _ = clients.replace(prev_clients, prev_groups);
        let _ = schedules.replace(prev_schedules);
        let _ = lists.replace(prev_lists);
        let _ = config.set(prev_settings);
        warn!(error = %error.1, "backup restore failed; rolled back");
        return Err(error);
    }
    record(
        &state,
        AuditEntry::success(&identity.username, "backup.restore"),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Milliseconds since the Unix epoch, for a backup's `createdAtMs` stamp.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

// --- Scheduled summary reports -------------------------------------------------

/// Build a report for `[from_ms, to_ms)`: read the aggregate from the durable log
/// and freeze it, resolving device names from the current catalogue. The DuckDB
/// read is blocking, so callers already off the async executor (the scheduler
/// thread) call it directly; the async handler wraps it in `spawn_blocking`.
fn generate_report(state: &AppState, period: ReportPeriod, from_ms: i64, to_ms: i64) -> Report {
    let aggregate = state
        .store
        .period_report(from_ms, to_ms, REPORT_TOP_N)
        .unwrap_or_default();
    let resolver = ClientResolver::new(state.clients.view());
    let summary = ReportSummary::from_period(aggregate, &resolver);
    state
        .reports
        .add(Report::new(period, from_ms, to_ms, summary))
}

/// Generate any report whose window has come due and is not already stored. Run
/// from the scheduler thread; a no-op when both cadences are off.
fn run_due_reports(state: &AppState) {
    let settings = state.config.get();
    let tz = parse_tz(&settings.timezone);
    let now = now_ms() as i64;
    let reports = &settings.reports;

    if let Some((from, to)) = due_daily(
        reports,
        tz,
        now,
        state.reports.latest_from(ReportPeriod::Daily),
    ) {
        generate_report(state, ReportPeriod::Daily, from, to);
        info!(from, to, "generated daily report");
    }
    if let Some((from, to)) = due_weekly(
        reports,
        tz,
        now,
        state.reports.latest_from(ReportPeriod::Weekly),
    ) {
        generate_report(state, ReportPeriod::Weekly, from, to);
        info!(from, to, "generated weekly report");
    }
}

/// One row of the report list: enough to show the list without the full top-tables.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportListItem {
    id: String,
    period: ReportPeriod,
    from_ms: i64,
    to_ms: i64,
    generated_at_ms: i64,
    total: u64,
    blocked: u64,
}

/// List the generated reports, newest first, as compact rows for the index.
async fn reports_list(State(reports): State<ReportStore>) -> Json<Vec<ReportListItem>> {
    let rows = reports
        .list()
        .into_iter()
        .map(|report| ReportListItem {
            id: report.id,
            period: report.period,
            from_ms: report.from_ms,
            to_ms: report.to_ms,
            generated_at_ms: report.generated_at_ms,
            total: report.summary.total,
            blocked: report.summary.blocked,
        })
        .collect();
    Json(rows)
}

/// The full report (headline numbers plus the three top-tables) for the viewer.
async fn reports_get(
    State(reports): State<ReportStore>,
    Path(id): Path<String>,
) -> Result<Json<Report>, StatusCode> {
    reports.get(&id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportRunParams {
    period: Option<String>,
}

/// Generate a report on demand for the last full day or week, so the admin need
/// not wait for the schedule. Returns the stored report.
async fn reports_run(
    State(state): State<AppState>,
    identity: Identity,
    Query(params): Query<ReportRunParams>,
) -> Result<Json<Report>, (StatusCode, String)> {
    let period = match params.period.as_deref() {
        Some("weekly") => ReportPeriod::Weekly,
        Some("daily") | None => ReportPeriod::Daily,
        Some(other) => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unknown report period '{other}'"),
            ));
        }
    };
    let tz = parse_tz(&state.config.get().timezone);
    let (from, to) = adhoc_window(period, tz, now_ms() as i64).ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "could not compute the report window".to_owned(),
    ))?;
    // The DuckDB aggregate is a blocking read; keep it off the async executor.
    let state_for_gen = state.clone();
    let report =
        tokio::task::spawn_blocking(move || generate_report(&state_for_gen, period, from, to))
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    record(
        &state,
        AuditEntry::success(&identity.username, "reports.run"),
    );
    Ok(Json(report))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportDownloadParams {
    format: Option<String>,
    lang: Option<String>,
}

/// Download a report as a self-contained HTML document (printable → PDF) or as
/// CSV of its top-lists.
async fn reports_download(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ReportDownloadParams>,
) -> Result<Response, (StatusCode, String)> {
    let report = state
        .reports
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, "no such report".to_owned()))?;
    let settings = state.config.get();
    let tz = parse_tz(&settings.timezone);
    let lang = RenderLang::parse(params.lang.as_deref());
    let as_csv = matches!(params.format.as_deref(), Some("csv"));
    let (body, content_type, ext) = if as_csv {
        (to_csv(&report, lang), "text/csv; charset=utf-8", "csv")
    } else {
        (
            to_html(&report, &settings.instance_name, tz, lang),
            "text/html; charset=utf-8",
            "html",
        )
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"justdns-report-{id}.{ext}\""),
            ),
        ],
        body,
    )
        .into_response())
}

/// Assemble the full configuration backup from the live stores. Shared by the
/// download endpoint and the pre-update safety backup.
fn build_backup(state: &AppState) -> Backup {
    Backup {
        app: BACKUP_APP.to_owned(),
        kind: BACKUP_KIND.to_owned(),
        version: BACKUP_VERSION,
        created_at_ms: Some(now_ms()),
        settings: state.config.get(),
        lists: state.lists.snapshot(),
        rewrites: state.rewrites.snapshot(),
        aliases: state.rewrites.alias_snapshot(),
        clients: state.clients.snapshot(),
        groups: state.clients.snapshot_groups(),
        schedules: state.schedules.snapshot(),
    }
}

/// Fill the block-page server IP(s) from this node's own cluster address when the
/// admin has not set one.
///
/// A blocked name is pointed at the block page by its `server_ipv4` (and
/// `server_ipv6`); inside a container the node cannot discover its own LAN address,
/// so a cluster that enables the block page everywhere would leave a second node
/// with a blank IP — and a blank IP used to answer `0.0.0.0` (`ERR_ADDRESS_INVALID`
/// in a browser). This derives the IP from the node's own advertised HA address
/// (`self_url`, e.g. `http://192.168.20.5:8080`), which is its real LAN address.
///
/// It is node-local (each node fills its own IP from its own address; the field is
/// never synced) and conservative: it only fills a **blank** field, never overrides
/// a value the admin typed, only acts while the block page is on, and only when the
/// advertised host parses as an IP (a hostname is left for the admin). It persists
/// through the normal config path, so the binary rebuilds the block-page target and
/// the UI shows the filled value.
fn autofill_block_page_ips(state: &AppState) {
    let Some(url) = state.ha.self_url() else {
        return;
    };
    let host = ha::host_label(&url);
    // An IPv6 literal comes back bracketed (`[::1]`); strip the brackets to parse.
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(&host);
    let Ok(ip) = bare.parse::<std::net::IpAddr>() else {
        return;
    };

    let mut settings = state.config.get();
    if !settings.block_page.enabled {
        return;
    }
    let changed = match ip {
        std::net::IpAddr::V4(_) if settings.block_page.server_ipv4.trim().is_empty() => {
            settings.block_page.server_ipv4 = ip.to_string();
            true
        }
        std::net::IpAddr::V6(_) if settings.block_page.server_ipv6.trim().is_empty() => {
            settings.block_page.server_ipv6 = ip.to_string();
            true
        }
        _ => false,
    };
    if !changed {
        return;
    }
    match state.config.set(settings) {
        Ok(()) => {
            tracing::info!(%ip, "filled the block-page server IP from the cluster address")
        }
        Err(error) => {
            tracing::warn!(%error, "could not fill the block-page server IP from the cluster address")
        }
    }
}

// ----------------------------------------------------------------------------
// High-availability sync handlers and the store ⇄ ledger bridge.
//
// `ha.rs` holds the pure merge over opaque-JSON collections; these functions are
// the only place that knows the real store types. They serialise each store into
// the keyed-collection form the merge works on, and deserialise a merged result
// back to apply it — reusing exactly the live `replace`/`set` paths that
// `restore_backup` uses.
// ----------------------------------------------------------------------------

/// Serialise this node's live configuration into the keyed collections the HA
/// merge operates on: one entry per entity, keyed by its stable id. The settings
/// are a single entry under `settings/settings`.
fn ha_collections(state: &AppState) -> BTreeMap<String, ha::Collection> {
    let mut collections: BTreeMap<String, ha::Collection> = BTreeMap::new();
    let mut put = |collection: &str, id: &str, value: Result<serde_json::Value, _>| {
        if let Ok(value) = value {
            collections
                .entry(collection.to_owned())
                .or_default()
                .insert(id.to_owned(), value);
        }
    };

    // instance_name, the DoT/DoH hostname, the DoT/DoH certificate selections and
    // the block-page server address are node identity, not shared config: blank
    // them before they enter the synced snapshot so a node's own display name,
    // certificate host, DoT/DoH certificate ids (each node picks a cert matching
    // its own hostname) and block-page IP (each node points blocked names at
    // itself) never travel to — or overwrite — the peer. The block page's enable
    // flag, title, message, redirect URL and its chosen certificate id are shared;
    // the certificate store itself is synced below, so the id names the same cert
    // on every node.
    let mut settings = state.config.get();
    settings.instance_name = String::new();
    settings.encrypted_dns.hostname = String::new();
    settings.encrypted_dns.dot_cert_id = None;
    settings.encrypted_dns.doh_cert_id = None;
    settings.block_page.server_ipv4 = String::new();
    settings.block_page.server_ipv6 = String::new();
    // The block page also travels as its OWN collection entry, with its own
    // last-writer-wins timestamp. It stays inside the settings blob too, so an older
    // peer that only reads the blob still sees it; but a peer that understands the
    // separate entry applies that one (see `ha_apply`), so changing any *other*
    // setting on one node can never flip the block page on another through the shared
    // blob's single timestamp. This is what keeps the block-page listeners from being
    // switched off cluster-wide by an unrelated settings save.
    put(
        "blockPage",
        "blockPage",
        serde_json::to_value(&settings.block_page),
    );
    put("settings", "settings", serde_json::to_value(settings));

    // The certificate store is shared across the cluster: one entry per stored
    // certificate, keyed by its stable id, carrying the full material (including
    // the private key) so a certificate selected or renewed on one node is present
    // on all of them. The material travels only between the cluster's own nodes.
    if let Some(store) = &state.certs {
        for record in store.export_all() {
            put("cert", &record.id, serde_json::to_value(&record));
        }
    }
    let lists = state.lists.snapshot();
    for entry in &lists.allow {
        put("allow", &entry.domain, serde_json::to_value(entry));
    }
    for entry in &lists.block {
        put("block", &entry.domain, serde_json::to_value(entry));
    }
    for sub in &lists.subscriptions {
        put("subscription", &sub.id, serde_json::to_value(sub));
    }
    for rewrite in state.rewrites.snapshot() {
        put(
            "rewrite",
            &rewrite.name.clone(),
            serde_json::to_value(rewrite),
        );
    }
    for alias in state.rewrites.alias_snapshot() {
        put("alias", &alias.name.clone(), serde_json::to_value(alias));
    }
    for device in state.clients.snapshot() {
        put(
            "device",
            &device.target.clone(),
            serde_json::to_value(device),
        );
    }
    for group in state.clients.snapshot_groups() {
        put("group", &group.id.clone(), serde_json::to_value(group));
    }
    for schedule in state.schedules.snapshot() {
        put(
            "schedule",
            &schedule.id.clone(),
            serde_json::to_value(schedule),
        );
    }
    // Pending access requests are shared so the admin sees every node's queue; a
    // release or dismissal on one node tombstones the entry across the cluster.
    for req in state.access.snapshot() {
        put("accessRequest", &req.id.clone(), serde_json::to_value(req));
    }
    for user in state.users.snapshot() {
        put("user", &user.username.clone(), serde_json::to_value(user));
    }
    collections
}

/// Rebuild the local collections and bring the ledger up to date with them
/// (stamping edits and deletions, pruning expired tombstones), persisting the
/// result. Shared by the snapshot endpoint and the sync loop so a served snapshot
/// and a merge both see fresh metadata.
fn ha_refresh(state: &AppState) -> (BTreeMap<String, ha::Collection>, ha::Ledger) {
    let collections = ha_collections(state);
    let now = now_ms();
    let mut ledger = ha::restamp(&collections, &state.ha.ledger(), &state.ha.node_id(), now);
    ha::prune_tombstones(&mut ledger, now, ha::TOMBSTONE_RETAIN_MS);
    state.ha.set_ledger(ledger.clone());
    (collections, ledger)
}

/// Deserialise one merged collection back into a `Vec<T>`. An absent collection
/// is an empty vector (everything in it was deleted or there was never any).
fn ha_collect<T: serde::de::DeserializeOwned>(
    collections: &BTreeMap<String, ha::Collection>,
    name: &str,
) -> Result<Vec<T>, String> {
    let Some(entities) = collections.get(name) else {
        return Ok(Vec::new());
    };
    entities
        .values()
        .map(|value| serde_json::from_value(value.clone()).map_err(|error| error.to_string()))
        .collect()
}

/// Keep confirmed MFA across a sync and let it spread. Ordinary last-writer-wins
/// resolves the user record as a whole, so a node whose `admin` happened to carry
/// a later timestamp **without** MFA would strip the second factor from everyone
/// — exactly how an upgrade that first began syncing users could wipe MFA by mere
/// restart order. This closes that: for every synced account, if the local record
/// **or** the peer's record has *confirmed* MFA, the merged record keeps confirmed
/// MFA (and the stricter `mfaRequired`).
///
/// It is deterministic and symmetric — both nodes see the same local/peer pair and
/// pick the same second factor — so the nodes converge on "MFA on" instead of
/// flapping. The one deliberate trade-off is fail-safe: removing MFA no longer
/// propagates, so it must be cleared on each node (documented in the HA guide).
fn ha_reconcile_mfa(
    merged: &mut BTreeMap<String, ha::Collection>,
    local: &BTreeMap<String, ha::Collection>,
    remote: &BTreeMap<String, ha::Collection>,
) {
    let Some(users) = merged.get_mut("user") else {
        return;
    };
    // The record for `username` in a side's collection, if present and parseable.
    let user_in = |side: &BTreeMap<String, ha::Collection>, username: &str| -> Option<User> {
        side.get("user")
            .and_then(|entities| entities.get(username))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    };
    let confirmed = |user: &User| user.mfa.as_ref().is_some_and(|mfa| mfa.confirmed);

    for (username, value) in users.iter_mut() {
        let Ok(mut record) = serde_json::from_value::<User>(value.clone()) else {
            continue;
        };
        let candidates: [Option<User>; 2] = [user_in(local, username), user_in(remote, username)];

        // A user is required to use MFA if any side says so.
        let required =
            record.mfa_required || candidates.iter().flatten().any(|user| user.mfa_required);

        // If the merged record has no confirmed factor but a side does, adopt it.
        // Checking local first then peer is still symmetric: when only one side is
        // confirmed, both nodes pick that same side's secret.
        if !confirmed(&record) {
            if let Some(source) = candidates.iter().flatten().find(|user| confirmed(user)) {
                record.mfa = source.mfa.clone();
            }
        }
        record.mfa_required = required;

        if let Ok(reconciled) = serde_json::to_value(&record) {
            *value = reconciled;
        }
    }
}

/// Apply a merged set of collections to the live stores, reusing the same
/// `replace`/`set` paths as a backup restore. Rewrites go first because they are
/// validated as they are stored, so an invalid merge is rejected before anything
/// else is touched.
fn ha_apply(
    state: &AppState,
    collections: &BTreeMap<String, ha::Collection>,
) -> Result<(), String> {
    let rewrites: Vec<Rewrite> = ha_collect(collections, "rewrite")?;
    let aliases: Vec<Alias> = ha_collect(collections, "alias")?;
    state
        .rewrites
        .replace_all(rewrites, aliases)
        .map_err(|error| error.to_string())?;

    let devices: Vec<Device> = ha_collect(collections, "device")?;
    let groups: Vec<Group> = ha_collect(collections, "group")?;
    state
        .clients
        .replace(devices, groups)
        .map_err(|error| error.to_string())?;

    let schedules: Vec<Schedule> = ha_collect(collections, "schedule")?;
    state
        .schedules
        .replace(schedules)
        .map_err(|error| error.to_string())?;

    let access: Vec<AccessRequest> = ha_collect(collections, "accessRequest")?;
    state
        .access
        .replace(access)
        .map_err(|error| error.to_string())?;

    let lists = UserLists {
        allow: ha_collect::<ListEntry>(collections, "allow")?,
        block: ha_collect::<ListEntry>(collections, "block")?,
        subscriptions: ha_collect::<Subscription>(collections, "subscription")?,
    };
    state
        .lists
        .replace(lists)
        .map_err(|error| error.to_string())?;

    // Users carry the full record (hash, role, MFA secret and recovery). An
    // absent collection means the peer predates user sync — leave the local
    // accounts alone rather than wiping them. A merged set with no administrator
    // is refused by the store; skip it this cycle (logging) instead of failing
    // the whole sync, so config still converges and the node keeps its accounts.
    let users: Vec<User> = ha_collect(collections, "user")?;
    if !users.is_empty() {
        match state.users.replace(users) {
            Ok(()) => {}
            Err(AuthError::LastAdmin) => {
                warn!("ha: skipped user sync — merged set had no administrator");
            }
            Err(error) => return Err(error.to_string()),
        }
    }

    // Apply the shared certificate store before the settings, so a certificate id
    // a setting (e.g. the block page) names already exists. Skipped when this node
    // has no store.
    if let Some(store) = &state.certs {
        let certs: Vec<CertRecord> = ha_collect(collections, "cert")?;
        store
            .replace_from_records(&certs)
            .map_err(|error| error.to_string())?;
    }

    if let Some(value) = collections
        .get("settings")
        .and_then(|entities| entities.get("settings"))
    {
        let mut settings: Settings =
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
        // Restore the node-local identity fields the snapshot blanks, so a merge
        // never renames this node, re-mints its certificate, repoints its DoT/DoH
        // certificate selections (each node picks a cert for its own hostname), or
        // changes its block-page address (each node points blocked names at
        // itself). The block page's chosen certificate id is shared and kept.
        let local = state.config.get();
        settings.instance_name = local.instance_name;
        settings.encrypted_dns.hostname = local.encrypted_dns.hostname;
        settings.encrypted_dns.dot_cert_id = local.encrypted_dns.dot_cert_id;
        settings.encrypted_dns.doh_cert_id = local.encrypted_dns.doh_cert_id;
        settings.block_page.server_ipv4 = local.block_page.server_ipv4.clone();
        settings.block_page.server_ipv6 = local.block_page.server_ipv6.clone();
        // The block page is also synced as its own entry, with an independent
        // timestamp, so an unrelated settings change on a peer cannot flip it off.
        // Prefer that entry when present; a peer old enough to carry the block page
        // only inside the settings blob falls through to the value deserialised
        // above. Its own server IPs stay node-local either way.
        if let Some(block_page_value) = collections
            .get("blockPage")
            .and_then(|entities| entities.get("blockPage"))
        {
            let mut block_page: BlockPage = serde_json::from_value(block_page_value.clone())
                .map_err(|error| error.to_string())?;
            block_page.server_ipv4 = local.block_page.server_ipv4;
            block_page.server_ipv6 = local.block_page.server_ipv6;
            settings.block_page = block_page;
        }
        state
            .config
            .set(settings)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// One sync cycle against a single peer: refresh the local ledger, pull the
/// peer's snapshot, merge both the config and the roster, and apply the config
/// when it changed anything. Returns an error string on any failure so the caller
/// can record it against that peer for the status view. The first-join winner bump
/// is not here — it is applied once at connect time (see [`ha::Ha::connect`]).
fn sync_peer(state: &AppState, peer: &ha::Peer) -> Result<(), String> {
    let (local_collections, local_ledger) = ha_refresh(state);
    let remote = state.ha.fetch_snapshot(&peer.url)?;

    // Learn the peer's roster so new members become visible to the auto-mesh, and
    // refresh its displayed version from the snapshot (it upgrades without re-pairing,
    // so the version captured at connect time would otherwise stay frozen).
    state.ha.merge_roster_in(&remote.roster);
    state.ha.note_peer_version(&remote.node_id, &remote.version);

    let local = ha::SyncSnapshot {
        node_id: state.ha.node_id(),
        collections: local_collections.clone(),
        ledger: local_ledger,
        version: state.ha.version(),
        roster: state.ha.roster(),
    };
    let mut merged = ha::merge(&local, &remote);

    // Confirmed MFA must never be lost to a sync, and it must spread: if either
    // side has it for an account, the merged record keeps it. Both nodes compute
    // the same reconciliation, so they converge on "MFA on" rather than letting an
    // arbitrary last-writer-wins order silently strip it (see ha_reconcile_mfa).
    ha_reconcile_mfa(
        &mut merged.collections,
        &local.collections,
        &remote.collections,
    );

    // Only touch the stores when the merge actually changed local state, so a
    // steady state does not reload lists or rewrite the config every cycle.
    if merged.collections != local_collections {
        ha_apply(state, &merged.collections)?;
    }
    let mut ledger = merged.ledger;
    ha::prune_tombstones(&mut ledger, now_ms(), ha::TOMBSTONE_RETAIN_MS);
    state.ha.set_ledger(ledger);

    // Stage C: pull this peer's own query-log events and union them into our
    // durable log, incrementally by a per-peer watermark. Best-effort — a failed
    // or partial pull just resumes next cycle — so it never fails the config sync.
    sync_peer_queries(state, &remote.node_id, &peer.url);
    Ok(())
}

/// Pull one peer's query-log events since our watermark and ingest them. Bounded
/// to the retention window (we never want rows we would immediately prune) and to
/// [`HA_QUERY_PULL_LIMIT`] per cycle; the watermark then advances to the newest
/// row durably queued, so the next cycle resumes from there.
fn sync_peer_queries(state: &AppState, peer_node_id: &str, peer_url: &str) {
    let retention_days = i64::from(state.config.get().log_retention_days.max(1));
    let window_start = now_ms() as i64 - retention_days * MS_PER_DAY;
    // First sync for this peer: pull the whole retention window. Afterwards resume
    // at the watermark, but never reach back past the window.
    let since_ms = state
        .ha
        .query_watermark(peer_node_id)
        .map_or(window_start, |wm| wm.max(window_start));

    match state
        .ha
        .fetch_queries::<HaQueriesPage>(peer_url, since_ms, HA_QUERY_PULL_LIMIT)
    {
        Ok(page) => {
            if let Some(new_watermark) = state.store.ingest_remote(peer_node_id, &page.events) {
                state.ha.set_query_watermark(peer_node_id, new_watermark);
            }
        }
        Err(error) => warn!(%error, peer = %peer_node_id, "peer query-log pull failed"),
    }
}

/// One mesh cycle: sync every peer independently (recording each outcome against
/// that peer), then auto-connect to any roster member we have learned but are not
/// yet meshed with. Isolating the peers means one node being down never stops the
/// others from converging, and a node coming back online catches up on the next
/// cycle without any manual step.
fn sync_mesh(state: &AppState) {
    for peer in state.ha.peers() {
        let Some(node_id) = peer.peer_node_id.clone() else {
            continue;
        };
        let result = sync_peer(state, &peer);
        state.ha.record_sync(&node_id, result);
    }
    // New members discovered via any peer's roster this cycle (or earlier) get
    // meshed now. Each connection is isolated; an unreachable member is retried
    // next cycle. New peers are synced on the following cycle.
    state.ha.auto_mesh();
}

/// Best-effort: tell every listed peer that `node_id` has left, so the removal
/// reaches the mesh at once. A peer unreachable right now still learns of it from
/// the roster tombstone on a later sync cycle, so a failure here is only logged.
fn ha_broadcast_depart(state: &AppState, node_id: &str, at_ms: u64, peers: &[ha::Peer]) {
    for peer in peers {
        if let Err(error) = state.ha.notify_depart(&peer.url, node_id, at_ms) {
            warn!(%error, peer = %peer.url, "could not announce node departure");
        }
    }
}

/// Authorize a peer request: the HA peer API must be open (enabled + key set), and
/// the request must carry a valid sealed `X-Ha-Auth` token — proof that the caller
/// holds the shared key, without the key ever travelling. Returns the caller's
/// authenticated identity, or the error response to send.
fn ha_peer_authorize(state: &AppState, headers: &HeaderMap) -> Result<ha::PeerAuth, StatusCode> {
    if !state.ha.peer_api_open() {
        return Err(StatusCode::NOT_FOUND);
    }
    let token = headers
        .get("x-ha-auth")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    state
        .ha
        .verify_peer_auth(token)
        .ok_or(StatusCode::UNAUTHORIZED)
}

/// Note the caller's authenticated identity (id + address), so a node that only ever
/// pulls still becomes visible to the mesh (we connect back to it from the roster).
/// The identity comes from the verified token, so it is authenticated.
fn ha_note_caller(state: &AppState, caller: &ha::PeerAuth) {
    if let Some(url) = &caller.self_url {
        if !caller.node_id.is_empty() && !url.is_empty() {
            state.ha.note_announced(&caller.node_id, url);
        }
    }
}

/// Seal a JSON response body under the cluster key, so a peer payload (which carries
/// certificate keys, password hashes and MFA secrets) never leaves in clear. Returns
/// 404 if no key is set — in which case the peer API is closed anyway.
fn ha_sealed<T: Serialize>(state: &AppState, value: &T) -> Response {
    let json = match serde_json::to_vec(value) {
        Ok(json) => json,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    match state.ha.seal_for_peer(&json) {
        Some(sealed) => {
            ([(header::CONTENT_TYPE, "application/octet-stream")], sealed).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Peer handshake: identify this node to a peer that holds the shared key. Used
/// by the connect flow to validate a pairing before storing it. Also learns the
/// caller, so seeding one direction meshes both.
async fn ha_hello(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let caller = match ha_peer_authorize(&state, &headers) {
        Ok(caller) => caller,
        Err(deny) => return deny.into_response(),
    };
    ha_note_caller(&state, &caller);
    ha_sealed(
        &state,
        &ha::Hello {
            node_id: state.ha.node_id(),
            version: state.status.version().to_owned(),
        },
    )
}

/// Peer snapshot: this node's current collections, ledger and roster, for the peer
/// to merge. Refreshing the ledger here keeps a served snapshot current between the
/// local loop's cycles; the roster lets the peer discover the rest of the mesh.
async fn ha_snapshot(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let caller = match ha_peer_authorize(&state, &headers) {
        Ok(caller) => caller,
        Err(deny) => return deny.into_response(),
    };
    ha_note_caller(&state, &caller);
    let (collections, ledger) = ha_refresh(&state);
    ha_sealed(
        &state,
        &ha::SyncSnapshot {
            node_id: state.ha.node_id(),
            collections,
            ledger,
            version: state.ha.version(),
            roster: state.ha.roster(),
        },
    )
}

/// A node telling us it has left the cluster — sent by a node that is leaving (its
/// own id) or by the node an admin removed one somewhere else (that node's id).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaDepartRequest {
    node_id: String,
    /// When the removal was decided. Absent (or 0) falls back to now on receipt.
    #[serde(default)]
    at_ms: u64,
}

/// Peer departure: tombstone the named node in our roster and drop it as a peer, so
/// the mesh stops syncing with a node that has left instead of a stale live copy
/// resurrecting it. Key-authed like the other peer endpoints. The caller is
/// deliberately **not** learned here — it may be the departing node itself, which we
/// must not re-add.
async fn ha_depart(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<HaDepartRequest>,
) -> Response {
    if let Err(deny) = ha_peer_authorize(&state, &headers) {
        return deny.into_response();
    }
    let at_ms = if request.at_ms == 0 {
        now_ms()
    } else {
        request.at_ms
    };
    state.ha.tombstone_node(&request.node_id, at_ms);
    StatusCode::NO_CONTENT.into_response()
}

/// Query parameters for the peer query-log pull.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaQueriesParams {
    /// Return this node's own events with `at_ms >= since_ms` (inclusive).
    since_ms: Option<i64>,
    /// Cap the page; clamped to a sane maximum.
    limit: Option<usize>,
}

/// A page of a node's own query-log events, served to a peer. Carries the serving
/// node's id so the puller attributes and dedups the rows under that origin.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaQueriesPage {
    node_id: String,
    events: Vec<QueryEvent>,
}

/// Peer query-log pull: this node's own (locally answered) events since a
/// watermark, oldest first, so a peer can union them into its explorer. Only own
/// rows are served, so a full mesh never re-forwards another node's events.
async fn ha_queries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HaQueriesParams>,
) -> Response {
    let caller = match ha_peer_authorize(&state, &headers) {
        Ok(caller) => caller,
        Err(deny) => return deny.into_response(),
    };
    ha_note_caller(&state, &caller);
    let since_ms = params.since_ms.unwrap_or(0);
    let limit = params
        .limit
        .unwrap_or(HA_QUERY_PULL_LIMIT)
        .clamp(1, HA_QUERY_PULL_LIMIT);
    let store = state.store.clone();
    let events =
        match tokio::task::spawn_blocking(move || store.queries_since_local(since_ms, limit)).await
        {
            Ok(Ok(events)) => events,
            Ok(Err(error)) => {
                warn!(%error, "peer query pull failed");
                Vec::new()
            }
            Err(error) => {
                warn!(%error, "peer query pull task panicked");
                Vec::new()
            }
        };
    ha_sealed(
        &state,
        &HaQueriesPage {
            node_id: state.ha.node_id(),
            events,
        },
    )
}

/// The HA status for the management UI (never includes the key).
async fn ha_status(State(state): State<AppState>) -> Json<ha::HaStatus> {
    Json(state.ha.status())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaEnableRequest {
    enabled: bool,
}

/// Turn HA on or off (step 1 of the wizard). Pairing and ledger are kept, so it
/// can be toggled without re-pairing.
async fn ha_enable(
    State(state): State<AppState>,
    identity: Identity,
    Json(request): Json<HaEnableRequest>,
) -> StatusCode {
    state.ha.set_enabled(request.enabled);
    record(
        &state,
        AuditEntry::success(
            &identity.username,
            if request.enabled {
                "ha.enable"
            } else {
                "ha.disable"
            },
        ),
    );
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaKeyRequest {
    /// The shared key; an empty or absent value clears the stored key.
    #[serde(default)]
    key: Option<String>,
}

/// Set or clear the shared sync key (step 2 of the wizard). Refused while a key
/// is pinned by the environment, which the UI already shows as managed.
async fn ha_set_key(
    State(state): State<AppState>,
    identity: Identity,
    Json(request): Json<HaKeyRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if state.ha.key_from_env() {
        return Err((
            StatusCode::CONFLICT,
            "the sync key is set by an environment variable".to_owned(),
        ));
    }
    // The key is the sole gate on the peer snapshot, which from Stage B carries
    // every password hash and TOTP secret. A short key is brute-forceable against
    // the unthrottled peer endpoint, so refuse a weak one here (clearing — an
    // empty value — is still allowed).
    if let Some(key) = request.key.as_deref() {
        let trimmed = key.trim();
        if !trimmed.is_empty() && trimmed.chars().count() < MIN_SYNC_KEY_LEN {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("the sync key must be at least {MIN_SYNC_KEY_LEN} characters"),
            ));
        }
    }
    state.ha.set_key(request.key);
    record(&state, AuditEntry::success(&identity.username, "ha.key"));
    Ok(StatusCode::NO_CONTENT)
}

/// The shared key, revealed to an admin for copying to another node. `key` is
/// null when none is set or the key is pinned by the environment.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HaKeyReveal {
    key: Option<String>,
}

/// Reveal the shared sync key so an admin can copy it when joining a new node.
/// Admin-only (the whole `/api/ha` surface is), and never returns an env-pinned
/// key. The reveal is audited so looking up the credential leaves a trace.
async fn ha_reveal_key(State(state): State<AppState>, identity: Identity) -> Json<HaKeyReveal> {
    let key = state.ha.revealable_key();
    record(
        &state,
        AuditEntry::success(&identity.username, "ha.key.reveal"),
    );
    Json(HaKeyReveal { key })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaAddressRequest {
    /// This node's own reachable base URL; an empty or absent value clears it.
    #[serde(default)]
    url: Option<String>,
}

/// Set this node's own advertised address, so peers can mesh back to it. The UI
/// prefills it from the browser's origin; the user can correct it.
async fn ha_set_address(
    State(state): State<AppState>,
    identity: Identity,
    Json(request): Json<HaAddressRequest>,
) -> StatusCode {
    state.ha.set_self_url(request.url);
    record(
        &state,
        AuditEntry::success(&identity.username, "ha.address"),
    );
    // Now that this node knows its own address, fill a blank block-page server IP
    // from it so blocked names point at this node without the admin typing the IP.
    autofill_block_page_ips(&state);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaConnectRequest {
    /// Base URL of a node already in the cluster (the seed).
    url: String,
    /// Which config wins the first join: `local` (this node adopts nothing, its
    /// config wins) or `peer` (adopt the cluster's config). The user makes this one
    /// choice when seeding; auto-discovered peers never restamp.
    #[serde(default)]
    prefer: HaPrefer,
}

/// Who wins conflicts at the first join. Defaults to the peer, so a node that is
/// joining an existing cluster adopts it unless the user says its own config wins.
#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum HaPrefer {
    Local,
    #[default]
    Peer,
}

/// Seed into the cluster (step 3 of the wizard): handshake with the shared key,
/// apply the one-time join restamp, then mesh with the seed on success. The rest
/// of the cluster is discovered and connected automatically. Requires a key first.
async fn ha_connect(
    State(state): State<AppState>,
    identity: Identity,
    Json(request): Json<HaConnectRequest>,
) -> Result<Json<ha::Hello>, (StatusCode, String)> {
    let mode = match request.prefer {
        HaPrefer::Local => ha::JoinMode::Win,
        HaPrefer::Peer => ha::JoinMode::Adopt,
    };
    let hello = state
        .ha
        .connect(&request.url, mode)
        .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
    record(
        &state,
        AuditEntry::success(&identity.username, "ha.connect"),
    );
    Ok(Json(hello))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HaDisconnectQuery {
    /// The node id of the single peer to forget. Absent ⇒ leave the whole cluster.
    #[serde(default)]
    node: Option<String>,
}

/// Forget one peer (by node id) or, with no node, leave the cluster entirely. Both
/// leave a roster tombstone and tell the remaining peers (`/api/ha/depart`), so the
/// removal converges across the mesh and the node is not re-added from another
/// node's gossip. The ledger is kept, so a later re-join converges cheaply.
async fn ha_disconnect(
    State(state): State<AppState>,
    identity: Identity,
    Query(query): Query<HaDisconnectQuery>,
) -> StatusCode {
    let at_ms = now_ms();
    match query.node {
        Some(node) if !node.trim().is_empty() => {
            // Inform the peers that stay (everyone but the one being removed), then
            // tombstone and drop it locally so the auto-mesh never reconnects to it.
            let others: Vec<ha::Peer> = state
                .ha
                .peers()
                .into_iter()
                .filter(|peer| peer.peer_node_id.as_deref() != Some(node.as_str()))
                .collect();
            state.ha.disconnect(&node);
            let broadcast_state = state.clone();
            let departed = node.clone();
            tokio::task::spawn_blocking(move || {
                ha_broadcast_depart(&broadcast_state, &departed, at_ms, &others);
            });
            record(
                &state,
                AuditEntry::success(&identity.username, "ha.disconnect"),
            );
        }
        _ => {
            // Leaving: tell every current peer we are going before we forget them, so
            // they drop us at once instead of waiting to time out against a gone node.
            let self_id = state.ha.node_id();
            let peers = state.ha.peers();
            state.ha.leave();
            let broadcast_state = state.clone();
            tokio::task::spawn_blocking(move || {
                ha_broadcast_depart(&broadcast_state, &self_id, at_ms, &peers);
            });
            record(&state, AuditEntry::success(&identity.username, "ha.leave"));
        }
    }
    StatusCode::NO_CONTENT
}

/// Run a mesh sync immediately (the "Sync now" button). Syncs every peer and runs
/// auto-mesh; per-peer outcomes are recorded for the status view.
async fn ha_sync_now(State(state): State<AppState>) -> Result<StatusCode, (StatusCode, String)> {
    if !state.ha.enabled() || state.ha.peers().is_empty() {
        return Err((
            StatusCode::CONFLICT,
            "HA is not enabled and meshed".to_owned(),
        ));
    }
    let state_for_sync = state.clone();
    tokio::task::spawn_blocking(move || sync_mesh(&state_for_sync))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// What the System page shows about updates: the running and latest versions,
/// whether an upgrade exists, the changelog to preview, whether the sidecar is
/// present (one-click vs. guided fallback), a pre-filled persistent backup path,
/// and the sidecar's latest progress. Optional fields are omitted when unset, so
/// the UI types them as optional with defaults.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateView {
    current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_version: Option<String>,
    update_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checked_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    updater_present: bool,
    default_backup_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<UpdateProgress>,
}

fn update_view(checker: &UpdateChecker) -> UpdateView {
    let check = checker.snapshot();
    UpdateView {
        current_version: check.current,
        latest_version: check.latest,
        update_available: check.update_available,
        release_notes: check.notes,
        release_url: check.release_url,
        checked_at_ms: check.checked_at_ms,
        error: check.error,
        updater_present: checker.updater_present(),
        default_backup_path: checker.default_backup_path(),
        progress: checker.progress(),
    }
}

/// The cached update status (no network call). Admin-only.
async fn get_update(State(checker): State<Arc<UpdateChecker>>) -> Json<UpdateView> {
    Json(update_view(&checker))
}

/// Force a fresh check against GitHub now. The HTTP client is blocking, so the
/// check runs on a blocking thread. Admin-only.
async fn check_update(
    State(checker): State<Arc<UpdateChecker>>,
) -> Result<Json<UpdateView>, (StatusCode, String)> {
    let worker = checker.clone();
    tokio::task::spawn_blocking(move || worker.refresh())
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(update_view(&checker)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartUpdate {
    /// Where to write the safety backup. Must be inside the data volume so it
    /// survives the container recreate.
    backup_path: String,
    /// The password the pre-update safety backup is encrypted with. Required, so
    /// the on-disk backup is never readable in the clear. The UI collects it twice
    /// to guard against a typo in a password that cannot be verified later.
    password: String,
}

/// Start a one-click update: validate the backup path, write the safety backup to
/// the persistent volume, then ask the sidecar to pull the latest release and
/// recreate the JustDNS container. Refuses when there is nothing to update to or
/// no sidecar is present (the UI then offers the guided command). Admin-only.
async fn start_update(
    State(state): State<AppState>,
    identity: Identity,
    Json(body): Json<StartUpdate>,
) -> Result<Json<UpdateView>, (StatusCode, String)> {
    let checker = &state.update;
    let check = checker.snapshot();
    if !check.update_available {
        return Err((
            StatusCode::CONFLICT,
            "no newer release is available".to_owned(),
        ));
    }
    let Some(target) = check.latest.clone() else {
        return Err((
            StatusCode::CONFLICT,
            "no newer release is available".to_owned(),
        ));
    };

    if body.password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "a backup password is required".to_owned(),
        ));
    }

    let path = std::path::PathBuf::from(&body.backup_path);
    if !checker.is_within_data(&path) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "the backup must be saved under {} so it survives the update",
                checker.data_dir().display()
            ),
        ));
    }

    // Without the sidecar nothing can be recreated; refuse before writing a backup
    // the server cannot act on. The UI shows the guided command for this case.
    if !checker.updater_present() {
        return Err((
            StatusCode::CONFLICT,
            "the updater sidecar is not running on this install".to_owned(),
        ));
    }

    // The safety net on disk comes before anything is triggered. It is encrypted
    // with the admin's password, so the on-disk backup is not readable in the
    // clear even though it keeps the webhook credential.
    let backup = build_backup(&state);
    let json = serde_json::to_vec_pretty(&backup)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let sealed = justdns_secret::seal_backup(&body.password, &json);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    std::fs::write(&path, &sealed)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    checker
        .request_update(&target, &identity.username)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    record(
        &state,
        AuditEntry::success(&identity.username, "update.start").target(&target),
    );
    Ok(Json(update_view(checker)))
}

/// A lighter poll than `GET /api/update`: just the running version (so the UI can
/// see when the new container is up) and the sidecar's progress. Admin-only.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStatusView {
    current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<UpdateProgress>,
}

async fn update_status(State(checker): State<Arc<UpdateChecker>>) -> Json<UpdateStatusView> {
    Json(UpdateStatusView {
        current_version: checker.current_version().to_owned(),
        progress: checker.progress(),
    })
}

/// Return the current settings so the dashboard can show them.
async fn get_settings(identity: Identity, State(config): State<ConfigStore>) -> Json<Settings> {
    let mut settings = config.get();
    // The webhook URL is a write-only credential (a Slack/Discord webhook URL, or a
    // Telegram bot token). Only an administrator — who may change it — ever receives
    // its value; a lower-privileged reader gets it blanked.
    if !identity.role.at_least(Role::Admin) {
        redact_settings_secrets(&mut settings);
    }
    Json(settings)
}

/// Blank the write-only secret fields in a settings snapshot (the notification
/// webhook URL and the Prometheus metrics token), for any copy that leaves the
/// admin trust boundary.
fn redact_settings_secrets(settings: &mut Settings) {
    settings.notifications.webhook_url = String::new();
    settings.metrics_token = String::new();
}

/// Replace the settings. The new values take effect at once (IPv6 live); a
/// failure to write them to disk is reported so the UI knows the change was
/// not saved durably.
async fn put_settings(
    State(state): State<AppState>,
    identity: Identity,
    Json(settings): Json<Settings>,
) -> Result<Json<Settings>, (StatusCode, String)> {
    state
        .config
        .set(settings)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    record(
        &state,
        AuditEntry::success(&identity.username, "settings.update"),
    );
    // If the admin just turned the block page on without a server IP, fill it from
    // this node's own cluster address so blocked names resolve to the right node.
    autofill_block_page_ips(&state);
    Ok(Json(state.config.get()))
}

/// The pending "ask to unblock this page" queue, newest first (admin only).
async fn list_access_requests(State(state): State<AppState>) -> Json<Vec<AccessRequest>> {
    Json(state.access.view())
}

/// How far a release reaches: just the device that asked, its whole group, or
/// everyone.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum ReleaseScope {
    Device,
    Group,
    All,
}

#[derive(Deserialize)]
struct ReleaseRequest {
    scope: ReleaseScope,
}

/// Release a requested domain at the chosen scope (an allow always wins over a
/// block, so it resolves normally again), then drop the request from the queue.
/// Audited.
async fn release_access_request(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
    Json(body): Json<ReleaseRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let Some(req) = state.access.get(&id) else {
        return Err((StatusCode::NOT_FOUND, "no such access request".to_owned()));
    };
    let domain = req.domain.clone();
    match body.scope {
        ReleaseScope::All => {
            state
                .lists
                .add_allow(&domain, None)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        }
        ReleaseScope::Device => {
            release_for_device(&state, &req, &domain)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        }
        ReleaseScope::Group => {
            release_for_group(&state, &req, &domain)
                .map_err(|error| (StatusCode::CONFLICT, error))?;
        }
    }
    // The acted-on allow is the lasting record; drop the queue entry (ignoring a
    // double-release race where it is already gone).
    let _ = state.access.remove(&id);
    record(
        &state,
        AuditEntry::success(&identity.username, "access.release"),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Dismiss a request without releasing anything. Audited.
async fn reject_access_request(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .access
        .remove(&id)
        .map_err(|_| (StatusCode::NOT_FOUND, "no such access request".to_owned()))?;
    record(
        &state,
        AuditEntry::success(&identity.username, "access.reject"),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Send a test notification with the current settings, so the admin can confirm
/// the webhook is reachable before relying on it. 204 on success; 400 with the
/// delivery error (no URL, transport failure, …) otherwise.
async fn notifications_test(
    State(state): State<AppState>,
    identity: Identity,
) -> Result<StatusCode, (StatusCode, String)> {
    let settings = state.config.get();
    let cfg = settings.notifications;
    let body = format!(
        "[{}] This is a test notification from JustDNS.",
        settings.instance_name
    );
    // The webhook client is blocking, so send off the async runtime.
    let result = tokio::task::spawn_blocking(move || {
        notify::send(&cfg, "JustDNS: test notification", &body)
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the test task failed".to_owned(),
        )
    })?;
    match result {
        Ok(()) => {
            record(
                &state,
                AuditEntry::success(&identity.username, "notifications.test"),
            );
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => {
            record(
                &state,
                AuditEntry::failure(&identity.username, "notifications.test"),
            );
            Err((StatusCode::BAD_REQUEST, error))
        }
    }
}

/// Add `domain` to the allow delta of the device that asked (resolved live from the
/// request's client address), merging it into the device's existing policy so
/// nothing else is lost. An address with no named device gets a device entry of its
/// own, scoped to that single IP.
fn release_for_device(state: &AppState, req: &AccessRequest, domain: &str) -> Result<(), String> {
    let client: std::net::IpAddr = req.client_ip.parse().map_err(|_| {
        format!(
            "the request has no usable client address: {}",
            req.client_ip
        )
    })?;
    let resolver = ClientResolver::new(state.clients.snapshot());
    let (target, group, mut policy) = match resolver.resolve(client) {
        Some(device) => (
            device.target.clone(),
            device.group.clone(),
            device.policy.clone(),
        ),
        None => (client.to_string(), None, PolicyDelta::default()),
    };
    if !policy.allow.iter().any(|existing| existing == domain) {
        policy.allow.push(domain.to_owned());
    }
    state
        .clients
        .set_device_policy(&target, group.as_deref(), policy)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Add `domain` to the allow delta of the group the asking device belongs to,
/// merging into the group's existing policy. Errors when the device (or its group)
/// can no longer be resolved, so the admin can fall back to a device or global
/// release.
fn release_for_group(state: &AppState, req: &AccessRequest, domain: &str) -> Result<(), String> {
    let client: std::net::IpAddr = req.client_ip.parse().map_err(|_| {
        format!(
            "the request has no usable client address: {}",
            req.client_ip
        )
    })?;
    let resolver = ClientResolver::new(state.clients.snapshot());
    let group_id = resolver
        .resolve(client)
        .and_then(|device| device.group.clone())
        .ok_or_else(|| {
            "this device is not in a group — release for the device or for everyone instead"
                .to_owned()
        })?;
    let group = state
        .clients
        .groups()
        .into_iter()
        .find(|group| group.id == group_id)
        .ok_or_else(|| "the device's group no longer exists".to_owned())?;
    let mut policy = group.policy.clone();
    if !policy.allow.iter().any(|existing| existing == domain) {
        policy.allow.push(domain.to_owned());
    }
    state
        .clients
        .edit_group(&group.id, &group.name, policy)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Totals for the dashboard.
#[derive(Serialize)]
struct StatsResponse {
    total: u64,
    cache_hits: u64,
    upstream: u64,
    local: u64,
    nxdomain: u64,
    servfail: u64,
    blocked: u64,
    active_clients: u64,
}

async fn stats(State(store): State<Store>) -> Json<StatsResponse> {
    let stats = store.stats();
    let active_clients = store
        .recent(ACTIVE_CLIENT_WINDOW)
        .iter()
        .map(|event| event.client)
        .collect::<HashSet<_>>()
        .len() as u64;

    Json(StatsResponse {
        total: stats.total,
        cache_hits: stats.cache_hits,
        upstream: stats.upstream,
        local: stats.local,
        nxdomain: stats.nxdomain,
        servfail: stats.servfail,
        blocked: stats.blocked,
        active_clients,
    })
}

/// How many top clients the statistics view asks for.
const TOP_CLIENTS: usize = 5;

/// A busiest-client entry with the device name resolved, so the overview can
/// show "Living room TV" rather than a bare address. `name` is absent when no
/// device matches the address.
#[derive(Serialize)]
struct NamedClientCount {
    client: String,
    count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

/// The last-24h figures for the dashboard's statistics: queries handled, how
/// many were blocked, the mean handling time, and the busiest clients. The
/// live query rate is derived on the client from successive `/api/stats` polls,
/// so it is not part of this payload.
#[derive(Serialize)]
struct DayStatsResponse {
    total: u64,
    blocked: u64,
    /// Mean, median and 95th-percentile handling time across every query (cache
    /// hits included, so a cache-heavy day reads fast); the median and p95 keep a
    /// single slow outlier from skewing the picture.
    avg_duration_ms: f64,
    median_duration_ms: f64,
    p95_duration_ms: f64,
    top_clients: Vec<NamedClientCount>,
}

async fn stats_day(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
) -> Json<DayStatsResponse> {
    let day = store.day_stats();
    let resolver = ClientResolver::new(clients.view());
    let top_clients = store
        .top_clients(TOP_CLIENTS)
        .into_iter()
        .map(|entry| NamedClientCount {
            name: resolver.name_for(entry.client).map(str::to_owned),
            client: entry.client.to_string(),
            count: entry.count,
        })
        .collect();
    Json(DayStatsResponse {
        total: day.total,
        blocked: day.blocked,
        avg_duration_ms: day.avg_duration_ms,
        median_duration_ms: day.median_duration_ms,
        p95_duration_ms: day.p95_duration_ms,
        top_clients,
    })
}

async fn activity(State(store): State<Store>) -> Json<Vec<ActivityBucket>> {
    Json(store.activity())
}

/// How many top domains the per-client view asks for by default, and the most it
/// will return at once.
const DEFAULT_TOP_DOMAINS: usize = 10;
const MAX_TOP_DOMAINS: usize = 50;

#[derive(Deserialize)]
struct TopDomainsParams {
    /// The client address to rank domains for.
    client: String,
    /// How many to return (defaults to [`DEFAULT_TOP_DOMAINS`], capped at
    /// [`MAX_TOP_DOMAINS`]).
    limit: Option<usize>,
}

/// The domains a single client looked up most over the last day, most lookups
/// first. Takes the client address as a query parameter; an unparseable or
/// never-seen client simply yields an empty list.
async fn top_domains(
    State(store): State<Store>,
    Query(params): Query<TopDomainsParams>,
) -> Json<Vec<DomainCount>> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_TOP_DOMAINS)
        .min(MAX_TOP_DOMAINS);
    match params.client.trim().parse() {
        Ok(client) => Json(store.top_domains_for_client(client, limit)),
        Err(_) => Json(Vec::new()),
    }
}

/// How many resolvers the response-time view returns. A home setup forwards to a
/// handful; this is a generous ceiling.
const TOP_RESOLVERS: usize = 20;

/// The upstream resolvers that answered over the last day, busiest first, each
/// with its answer count and handling-time figures (mean, median, p95). Only
/// forwarded queries count — cache hits, blocks and local answers never reach a
/// resolver — so the list is empty on a cache-only or freshly started instance.
async fn resolvers(State(store): State<Store>) -> Json<Vec<ResolverStat>> {
    Json(store.resolver_stats(TOP_RESOLVERS))
}

// --- Privacy: tracker attribution ---------------------------------------------

/// How many name rows to scan when building the privacy view. Only a fraction of
/// names belong to the tracker catalogue, so scan generously to catch them all.
const TRACKER_SCAN_LIMIT: usize = 200_000;

#[derive(Deserialize)]
struct TrackerParams {
    /// The window to attribute over: `day` (default), `week` or `month`.
    window: Option<String>,
}

/// How many devices and domains a company's drill-down carries at most, busiest
/// first — plenty for the detail view without an unbounded payload.
const COMPANY_TOP_DEVICES: usize = 50;
const COMPANY_TOP_DOMAINS: usize = 40;

/// One company a device (or the whole network) reached, with how many lookups
/// went to it and how many of those the filter blocked versus let through. For a
/// network-wide company the drill-down fields `devices` and `domains` are filled
/// (which devices reached it, which names under it were looked up); for a single
/// device's company they stay empty and are omitted.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CompanyContact {
    org: String,
    category: String,
    contacts: u64,
    blocked: u64,
    allowed: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    devices: Vec<CompanyDevice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    domains: Vec<CompanyDomain>,
}

/// One device in a company's drill-down: how much it reached that company.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CompanyDevice {
    client: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    contacts: u64,
    blocked: u64,
    allowed: u64,
}

/// One name in a company's drill-down: how often that exact domain was looked up.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CompanyDomain {
    name: String,
    contacts: u64,
    blocked: u64,
    allowed: u64,
}

/// One device's tracker contacts over the window, with its per-company breakdown.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceTrackers {
    client: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    contacts: u64,
    blocked: u64,
    allowed: u64,
    companies: Vec<CompanyContact>,
}

/// The privacy dashboard payload: network-wide totals, the companies contacted
/// most, and a per-device breakdown.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackerReport {
    window: String,
    from_ms: i64,
    to_ms: i64,
    total_contacts: u64,
    total_blocked: u64,
    total_allowed: u64,
    companies: Vec<CompanyContact>,
    devices: Vec<DeviceTrackers>,
}

/// Running total for one company while aggregating a scan.
#[derive(Default)]
struct TrackerAccum {
    category: &'static str,
    contacts: u64,
    blocked: u64,
    allowed: u64,
}

impl TrackerAccum {
    fn add(&mut self, category: &'static str, count: u64, blocked: bool) {
        self.category = category;
        self.contacts += count;
        if blocked {
            self.blocked += count;
        } else {
            self.allowed += count;
        }
    }
}

/// Turn a per-company accumulation into the response rows, busiest company first.
/// Used for a single device's list, so the drill-down fields stay empty.
fn tracker_companies(
    map: std::collections::HashMap<&'static str, TrackerAccum>,
) -> Vec<CompanyContact> {
    let mut companies: Vec<CompanyContact> = map
        .into_iter()
        .map(|(org, a)| CompanyContact {
            org: org.to_string(),
            category: a.category.to_string(),
            contacts: a.contacts,
            blocked: a.blocked,
            allowed: a.allowed,
            devices: Vec::new(),
            domains: Vec::new(),
        })
        .collect();
    companies.sort_by(|a, b| b.contacts.cmp(&a.contacts).then_with(|| a.org.cmp(&b.org)));
    companies
}

/// A network-wide company's running totals plus its drill-down maps: which
/// devices reached it (by client address) and which exact names under it were
/// looked up.
#[derive(Default)]
struct OrgAgg {
    totals: TrackerAccum,
    devices: std::collections::HashMap<String, TrackerAccum>,
    domains: std::collections::HashMap<String, TrackerAccum>,
}

/// Build the network company list from the per-org aggregation, busiest company
/// first, each carrying its busiest devices and domains (capped). Device names
/// are resolved from the client catalogue.
fn network_companies(
    network: std::collections::HashMap<&'static str, OrgAgg>,
    resolver: &ClientResolver,
) -> Vec<CompanyContact> {
    let mut companies: Vec<CompanyContact> = network
        .into_iter()
        .map(|(org, agg)| {
            let mut devices: Vec<CompanyDevice> = agg
                .devices
                .into_iter()
                .map(|(client, a)| {
                    let name = client
                        .parse()
                        .ok()
                        .and_then(|ip| resolver.name_for(ip).map(str::to_owned));
                    CompanyDevice {
                        client,
                        name,
                        contacts: a.contacts,
                        blocked: a.blocked,
                        allowed: a.allowed,
                    }
                })
                .collect();
            devices.sort_by(|a, b| {
                b.contacts
                    .cmp(&a.contacts)
                    .then_with(|| a.client.cmp(&b.client))
            });
            devices.truncate(COMPANY_TOP_DEVICES);

            let mut domains: Vec<CompanyDomain> = agg
                .domains
                .into_iter()
                .map(|(name, a)| CompanyDomain {
                    name,
                    contacts: a.contacts,
                    blocked: a.blocked,
                    allowed: a.allowed,
                })
                .collect();
            domains.sort_by(|a, b| {
                b.contacts
                    .cmp(&a.contacts)
                    .then_with(|| a.name.cmp(&b.name))
            });
            domains.truncate(COMPANY_TOP_DOMAINS);

            CompanyContact {
                org: org.to_string(),
                category: agg.totals.category.to_string(),
                contacts: agg.totals.contacts,
                blocked: agg.totals.blocked,
                allowed: agg.totals.allowed,
                devices,
                domains,
            }
        })
        .collect();
    companies.sort_by(|a, b| b.contacts.cmp(&a.contacts).then_with(|| a.org.cmp(&b.org)));
    companies
}

/// Attribute each device's DNS lookups to the companies behind the names over the
/// chosen window (`day`/`week`/`month`): read the durable log, map every queried
/// name through the compiled tracker catalogue, and aggregate per device and per
/// company — blocked lookups counted apart from the ones that got through.
/// Node-local only. Readable by any signed-in user, like the other stats views.
async fn privacy_trackers(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    Query(params): Query<TrackerParams>,
) -> Result<Json<TrackerReport>, StatusCode> {
    let window = params.window.as_deref().unwrap_or("day").to_owned();
    let span_ms = match window.as_str() {
        "day" => MS_PER_DAY,
        "week" => 7 * MS_PER_DAY,
        "month" => 30 * MS_PER_DAY,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let to_ms = now_ms() as i64;
    let from_ms = to_ms - span_ms;

    let rows =
        tokio::task::spawn_blocking(move || store.tracker_scan(from_ms, to_ms, TRACKER_SCAN_LIMIT))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resolver = ClientResolver::new(clients.view());

    let mut per_device: std::collections::HashMap<
        String,
        std::collections::HashMap<&'static str, TrackerAccum>,
    > = std::collections::HashMap::new();
    let mut network: std::collections::HashMap<&'static str, OrgAgg> =
        std::collections::HashMap::new();
    let mut total_contacts = 0u64;
    let mut total_blocked = 0u64;
    let mut total_allowed = 0u64;

    for row in &rows {
        let Some(tracker) = justdns_filter::classify_tracker(&row.name) else {
            continue;
        };
        total_contacts += row.count;
        if row.blocked {
            total_blocked += row.count;
        } else {
            total_allowed += row.count;
        }
        per_device
            .entry(row.client.clone())
            .or_default()
            .entry(tracker.org)
            .or_default()
            .add(tracker.category, row.count, row.blocked);
        let org = network.entry(tracker.org).or_default();
        org.totals.add(tracker.category, row.count, row.blocked);
        org.devices.entry(row.client.clone()).or_default().add(
            tracker.category,
            row.count,
            row.blocked,
        );
        org.domains.entry(row.name.clone()).or_default().add(
            tracker.category,
            row.count,
            row.blocked,
        );
    }

    let mut devices: Vec<DeviceTrackers> = per_device
        .into_iter()
        .map(|(client, orgs)| {
            let companies = tracker_companies(orgs);
            let contacts = companies.iter().map(|c| c.contacts).sum();
            let blocked = companies.iter().map(|c| c.blocked).sum();
            let allowed = companies.iter().map(|c| c.allowed).sum();
            let name = client
                .parse()
                .ok()
                .and_then(|ip| resolver.name_for(ip).map(str::to_owned));
            DeviceTrackers {
                client,
                name,
                contacts,
                blocked,
                allowed,
                companies,
            }
        })
        .collect();
    devices.sort_by(|a, b| {
        b.contacts
            .cmp(&a.contacts)
            .then_with(|| a.client.cmp(&b.client))
    });

    Ok(Json(TrackerReport {
        window,
        from_ms,
        to_ms,
        total_contacts,
        total_blocked,
        total_allowed,
        companies: network_companies(network, &resolver),
        devices,
    }))
}

/// Block request from the privacy dashboard: a whole company by `org` (every
/// domain the catalogue attributes to it) or a single `domain`. Exactly one is
/// expected; `org` wins if both are sent. With `client` (a device address) the
/// block applies to that one device's own policy instead of the global list.
#[derive(Deserialize)]
struct PrivacyBlockBody {
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    /// Optional provenance tag for a single-domain block (the company it was
    /// blocked from); ignored for a company block, which tags with the org name,
    /// and ignored for a per-device block, which has no note.
    #[serde(default)]
    note: Option<String>,
    /// When set, the block targets this device's own policy (by its address)
    /// rather than the global block list.
    #[serde(default)]
    client: Option<String>,
}

/// How many entries a block action newly added (a re-block adds nothing).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyBlockResult {
    added: usize,
}

/// Act on what the privacy dashboard shows: add a whole company's catalogue
/// domains (or a single domain) to the block list. A content change, so
/// Editor-gated like the other list edits, and idempotent — re-blocking simply
/// adds nothing. Without `client` it adds to the global block list (company-wide
/// blocks tag each entry with the company name); with `client` it adds to that
/// one device's own policy instead.
async fn privacy_block(
    State(lists): State<ListStore>,
    State(clients): State<ClientStore>,
    Json(body): Json<PrivacyBlockBody>,
) -> Result<Json<PrivacyBlockResult>, (StatusCode, String)> {
    let org = body.org.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let domain = body
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let client = body
        .client
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Resolve which domains this request blocks (and, for the global list, the
    // note each entry carries), independent of where they land.
    let (domains, note): (Vec<&str>, Option<&str>) = if let Some(org) = org {
        let catalogue = justdns_filter::tracker_domains_for_org(org)
            .ok_or((StatusCode::NOT_FOUND, format!("unknown company: {org}")))?;
        (catalogue.to_vec(), Some(org))
    } else if let Some(domain) = domain {
        if justdns_filter::parse_rule(domain).is_none() {
            return Err((StatusCode::BAD_REQUEST, format!("invalid domain: {domain}")));
        }
        let note = body
            .note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        (vec![domain], note)
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "either org or domain is required".to_owned(),
        ));
    };

    let added = if let Some(client) = client {
        clients
            .add_device_blocks(client, &domains)
            .map_err(client_error)?
    } else {
        lists.add_blocks(&domains, note).map_err(list_error)?
    };

    Ok(Json(PrivacyBlockResult { added }))
}

#[derive(Deserialize)]
struct QueryParams {
    limit: Option<usize>,
    /// Optional case-insensitive filter on the queried name, client address or
    /// device name.
    q: Option<String>,
}

/// A logged query with its client's device name resolved, so the log can show
/// who made it. `device_name` is absent when no device matches the address. The
/// query event's own fields are flattened in, so the shape is the plain event
/// plus one optional field.
#[derive(Serialize)]
struct NamedQueryEvent {
    #[serde(flatten)]
    event: QueryEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
    /// The node this row came from: empty for this node's own queries, a peer's
    /// node id for one pulled over the HA mesh. Omitted when empty, so the hot
    /// query feed (always local) and an older client both read it as this node.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    origin: String,
}

/// How many recent events to scan when a search includes device names: the whole
/// hot ring, so a device-name match reaches as far back as a name/address match.
const QUERY_SCAN_LIMIT: usize = 100_000;

/// One cluster node as the query-log explorer shows it: the stable id plus the
/// host a person recognises it by, so the origin badge reads as a server name
/// instead of a cryptic id prefix.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeInfo {
    id: String,
    /// The host of the node's advertised address (`dns-02`, `192.168.20.6`),
    /// empty when the node has not advertised one yet.
    label: String,
    /// `true` for the node serving this request, so the UI can mark it as this
    /// server.
    #[serde(rename = "self")]
    is_self: bool,
}

/// The cluster roster resolved to display labels for the query-log explorer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodesResponse {
    self_id: String,
    nodes: Vec<NodeInfo>,
}

/// Resolve the HA roster to display labels so the Protokoll explorer can show
/// which server answered a query. Readable by any signed-in user — like
/// `/api/queries` itself — because it exposes only node ids and reachable
/// hostnames, never the sync key or peer health. The local node is always
/// present (even before it has advertised an address) and flagged as self.
async fn nodes(State(state): State<AppState>) -> Json<NodesResponse> {
    let self_id = state.ha.node_id();
    let mut nodes: Vec<NodeInfo> = state
        .ha
        .roster()
        .into_iter()
        .map(|(id, member)| {
            let is_self = id == self_id;
            NodeInfo {
                label: ha::host_label(&member.url),
                id,
                is_self,
            }
        })
        .collect();
    if !nodes.iter().any(|node| node.is_self) {
        nodes.push(NodeInfo {
            id: self_id.clone(),
            label: String::new(),
            is_self: true,
        });
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Json(NodesResponse { self_id, nodes })
}

async fn queries(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    Query(params): Query<QueryParams>,
) -> Json<Vec<NamedQueryEvent>> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .min(MAX_QUERY_LIMIT);
    let resolver = ClientResolver::new(clients.view());

    let events = match params.q.as_deref().map(str::trim) {
        Some(needle) if !needle.is_empty() => {
            // Filter in the web layer so a device name counts alongside the
            // queried name and the client address. Scans the hot ring (the same
            // depth the store's own search uses), newest first, up to the limit.
            let needle = needle.to_lowercase();
            store
                .recent(QUERY_SCAN_LIMIT)
                .into_iter()
                .filter(|event| {
                    event.name.to_lowercase().contains(&needle)
                        || event.client.to_string().contains(&needle)
                        || resolver
                            .name_for(event.client)
                            .is_some_and(|name| name.to_lowercase().contains(&needle))
                })
                .take(limit)
                .collect()
        }
        _ => store.recent(limit),
    };

    let named = events
        .into_iter()
        .map(|event| NamedQueryEvent {
            device_name: resolver.name_for(event.client).map(str::to_owned),
            event,
            origin: String::new(),
        })
        .collect();
    Json(named)
}

/// Query parameters for the history explorer. All optional; omitting every one
/// returns the newest page of the whole log. `source` is a comma-separated list
/// of source labels (OR). `beforeMs`+`beforeRow` together are the keyset cursor
/// from a previous page's `nextBefore`; either one alone is ignored.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryParams {
    before_ms: Option<i64>,
    before_row: Option<i64>,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    source: Option<String>,
    record_type: Option<u16>,
    response_code: Option<u16>,
    resolver: Option<String>,
    /// Filter to one node: empty is this node's own rows, a node id a peer's,
    /// absent every node. Lets the explorer narrow the unioned view.
    origin: Option<String>,
    /// Filter to one named device, by its `target` (an address or CIDR range).
    device: Option<String>,
    /// Filter to one device group, by its stable `id`. `device` wins if both are
    /// set. Both resolve, via the device catalogue, to the set of client
    /// addresses to match.
    group: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

/// A keyset cursor returned to the client so "load older" can continue exactly
/// where the page ended. Mirrors the store's `(at_ms, rowid)`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryCursor {
    at_ms: i64,
    row: i64,
}

/// The history explorer's payload: a page of events (newest first, device names
/// resolved) and, when more may follow, the cursor for the next page. A missing
/// `nextBefore` means this was the last page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryResponse {
    rows: Vec<NamedQueryEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_before: Option<HistoryCursor>,
}

/// Filtered, paginated read over the **full durable history** in DuckDB — the
/// explorer behind `/protokoll`. Unlike `/api/queries` (the hot ring), this can
/// page back past the in-memory window. The DuckDB read runs on a blocking
/// thread so a large scan never stalls the async runtime; device names are
/// resolved afterwards from the client store, as in `queries`.
async fn queries_history(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    Query(params): Query<HistoryParams>,
) -> Json<HistoryResponse> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .clamp(1, MAX_QUERY_LIMIT);

    // The cursor needs both halves; a lone one is meaningless, so ignore it.
    let before: Option<Cursor> = match (params.before_ms, params.before_row) {
        (Some(at_ms), Some(row)) => Some((at_ms, row)),
        _ => None,
    };
    let sources = params
        .source
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .filter_map(|part| QuerySource::from_label(part.trim()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let trimmed = |value: Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    // A device- or group-filter names clients the catalogue knows, but a device
    // may be a CIDR range, so we resolve every observed client address through the
    // same most-specific-wins matcher the log's name column uses and keep those
    // that belong to the requested device or group. `device` wins over `group`.
    // The resulting `client IN (…)` set keeps keyset pagination exact; an empty
    // set (no observed client belongs) makes the result honestly empty.
    let device_target = trimmed(params.device);
    let group_id = trimmed(params.group);
    let client_filter: Option<Vec<String>> = if device_target.is_some() || group_id.is_some() {
        let resolver = ClientResolver::new(clients.view());
        let store_for_clients = store.clone();
        let observed = match tokio::task::spawn_blocking(move || {
            store_for_clients.distinct_clients(MAX_FILTER_CLIENTS)
        })
        .await
        {
            Ok(Ok(list)) => list,
            Ok(Err(error)) => {
                warn!(%error, "distinct-clients scan for device filter failed");
                Vec::new()
            }
            Err(error) => {
                warn!(%error, "distinct-clients scan task panicked");
                Vec::new()
            }
        };
        let matched = observed
            .into_iter()
            .filter(|client| {
                let Ok(ip) = client.parse::<std::net::IpAddr>() else {
                    return false;
                };
                match resolver.resolve(ip) {
                    Some(device) => match (&device_target, &group_id) {
                        (Some(target), _) => &device.target == target,
                        (None, Some(group)) => device.group.as_deref() == Some(group.as_str()),
                        (None, None) => false,
                    },
                    None => false,
                }
            })
            .collect::<Vec<_>>();
        Some(matched)
    } else {
        None
    };

    let filter = QueryFilter {
        before,
        start_ms: params.start_ms,
        end_ms: params.end_ms,
        sources,
        record_type: params.record_type,
        response_code: params.response_code,
        resolver: trimmed(params.resolver),
        origin: params.origin,
        clients: client_filter,
        needle: trimmed(params.q),
        limit,
    };

    // DuckDB reads are blocking; keep them off the async executor.
    let store_for_read = store.clone();
    let (events, next) =
        match tokio::task::spawn_blocking(move || store_for_read.history(&filter)).await {
            Ok(Ok(page)) => page,
            Ok(Err(error)) => {
                warn!(%error, "history query failed");
                (Vec::new(), None)
            }
            Err(error) => {
                warn!(%error, "history query task panicked");
                (Vec::new(), None)
            }
        };

    let resolver = ClientResolver::new(clients.view());
    let rows = events
        .into_iter()
        .map(|row| NamedQueryEvent {
            device_name: resolver.name_for(row.event.client).map(str::to_owned),
            event: row.event,
            origin: row.origin,
        })
        .collect();

    Json(HistoryResponse {
        rows,
        next_before: next.map(|(at_ms, row)| HistoryCursor { at_ms, row }),
    })
}

#[derive(Deserialize)]
struct DnssecParams {
    /// Optional filter to one verdict: `secure`, `insecure`, `bogus` or
    /// `indeterminate`. Anything else (or absent) returns every validated entry.
    status: Option<String>,
    limit: Option<usize>,
}

/// The DNSSEC page's payload: the lifetime counts for the four verdicts (the
/// counter cards) and the most recent validated entries (the table), newest
/// first, optionally filtered to one verdict. Only answers logged while
/// validation was on carry a verdict; everything else is left out of `recent`.
#[derive(Serialize)]
struct DnssecResponse {
    secure: u64,
    insecure: u64,
    bogus: u64,
    indeterminate: u64,
    recent: Vec<NamedQueryEvent>,
}

async fn dnssec(
    State(store): State<Store>,
    State(clients): State<ClientStore>,
    Query(params): Query<DnssecParams>,
) -> Json<DnssecResponse> {
    let stats = store.stats();
    let limit = params
        .limit
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .min(MAX_QUERY_LIMIT);
    let want = params.status.as_deref().and_then(DnssecStatus::from_label);
    let resolver = ClientResolver::new(clients.view());

    // Scan the hot ring newest-first for entries that carry a verdict, keeping
    // those matching the requested one (or all, when none is asked for).
    let recent = store
        .recent(QUERY_SCAN_LIMIT)
        .into_iter()
        .filter(|event| event.dnssec.is_some())
        .filter(|event| want.is_none_or(|w| event.dnssec == Some(w)))
        .take(limit)
        .map(|event| NamedQueryEvent {
            device_name: resolver.name_for(event.client).map(str::to_owned),
            event,
            origin: String::new(),
        })
        .collect();

    Json(DnssecResponse {
        secure: stats.dnssec_secure,
        insecure: stats.dnssec_insecure,
        bogus: stats.dnssec_bogus,
        indeterminate: stats.dnssec_indeterminate,
        recent,
    })
}

/// The unauthenticated liveness probe: enough for a Docker HEALTHCHECK or an
/// external monitor to confirm the server is up, without leaking the version
/// (which is admin-only, in `/api/diag`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    uptime_secs: u64,
}

async fn health(State(status): State<Arc<RuntimeStatus>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_secs: status.uptime_secs(),
    })
}

#[derive(Deserialize)]
struct LogParams {
    /// Keep only entries at or above this level (`error`/`warn`/`info`/…). Absent
    /// returns every captured level.
    level: Option<String>,
    limit: Option<usize>,
}

/// The most recent diagnostic log events, newest first, optionally filtered to a
/// minimum level. Admin-only (gated by `required_role`).
async fn system_logs(
    State(logs): State<LogBuffer>,
    Query(params): Query<LogParams>,
) -> Json<Vec<LogRecord>> {
    let limit = params.limit.unwrap_or(DEFAULT_LOG_LIMIT).min(MAX_LOG_LIMIT);
    Json(logs.snapshot(params.level.as_deref(), limit))
}

/// The active log verbosity (`normal`/`debug`/`trace`, or `custom`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogLevelResponse {
    level: String,
}

/// The verbosity an admin asks the engine to switch to.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogLevelRequest {
    level: String,
}

/// Report the active log verbosity. Admin-only (gated by [`required_role`]).
async fn get_log_level(State(control): State<Arc<LogLevelControl>>) -> Json<LogLevelResponse> {
    Json(LogLevelResponse {
        level: control.current(),
    })
}

/// Raise or lower the live log verbosity without a restart. Admin-only. The
/// change is deliberately **not** persisted: a restart returns to the shipping
/// default, so a forgotten debug session cannot quietly keep filling the disk.
async fn set_log_level(
    State(state): State<AppState>,
    identity: Identity,
    Json(request): Json<LogLevelRequest>,
) -> Response {
    match state.log_level.set(&request.level) {
        Ok(()) => {
            // An audit line, since raising verbosity can expose more detail in
            // the diagnostic log; the change itself is also worth a log event.
            record(
                &state,
                AuditEntry::success(&identity.username, "logs.level")
                    .detail(format!("level={}", request.level)),
            );
            info!(level = %request.level, "log verbosity changed at runtime");
            Json(LogLevelResponse {
                level: state.log_level.current(),
            })
            .into_response()
        }
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

/// Which DNS listeners are up. Plain UDP/TCP are up whenever the process serves;
/// DoT/DoH reflect whether the encrypted endpoints are configured on.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Listeners {
    udp: bool,
    tcp: bool,
    dot: bool,
    doh: bool,
}

/// When the subscribed feeds were last refreshed and whether it failed.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedsInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    last_refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

/// Rule counts across the active block/allow sets.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListCounts {
    ads: usize,
    malware: usize,
    allow: usize,
    custom: usize,
}

/// The admin diagnostics snapshot behind the System page's status tiles.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagResponse {
    version: String,
    uptime_secs: u64,
    listeners: Listeners,
    feeds: FeedsInfo,
    cache_entries: u64,
    lists: ListCounts,
    active_clients: u64,
    /// The active log verbosity (`normal`/`debug`/`trace`/`custom`), so the
    /// System page can show and offer to change it.
    log_level: String,
}

/// A runtime snapshot for the System page: version, uptime, listener and feed
/// health, cache size, list sizes and how many clients are currently active.
/// Admin-only.
async fn diag(
    State(status): State<Arc<RuntimeStatus>>,
    State(blocker): State<Arc<Blocker>>,
    State(cache): State<DnsCache>,
    State(store): State<Store>,
    State(log_level): State<Arc<LogLevelControl>>,
) -> Json<DiagResponse> {
    let (udp, tcp, dot, doh) = status.listeners();
    let feed = status.feed();
    let active_clients = store
        .recent(ACTIVE_CLIENT_WINDOW)
        .iter()
        .map(|event| event.client)
        .collect::<HashSet<_>>()
        .len() as u64;

    Json(DiagResponse {
        version: status.version().to_string(),
        uptime_secs: status.uptime_secs(),
        listeners: Listeners { udp, tcp, dot, doh },
        feeds: FeedsInfo {
            last_refresh: feed.last_refresh,
            last_error: feed.last_error,
        },
        cache_entries: cache.len(),
        lists: ListCounts {
            ads: blocker.ads_len(),
            malware: blocker.malware_len(),
            allow: blocker.allow_len(),
            custom: blocker.custom_len(),
        },
        active_clients,
        log_level: log_level.current(),
    })
}

/// How many upstream resolvers to include in the metrics. Bounded, though the
/// configured upstreams keep the real count small anyway.
const RESOLVER_METRICS_LIMIT: usize = 20;

/// The Prometheus scrape endpoint. Off (404) unless a bearer token is set; when
/// set, the scraper must present it as `Authorization: Bearer <token>`. The token
/// is managed in the dashboard (stored in the encrypted config); an install that
/// still sets the `JUSTDNS_METRICS_TOKEN` environment variable keeps working as a
/// fallback when no token is configured in the UI. A scraper cannot hold a login
/// session, so this carries its own token check rather than sitting behind the
/// auth guard. The figures come from the same in-memory handles the diagnostics
/// endpoint reads, so a scrape never touches DuckDB.
async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // The UI-managed token (live config) wins; the startup env token is the
    // fallback. `configured` is kept alive so `expected` can borrow it.
    let configured = state.config.get().metrics_token;
    let configured = configured.trim();
    let expected = if !configured.is_empty() {
        configured
    } else if let Some(env_token) = state.metrics_token.as_deref() {
        env_token
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !metrics::token_matches(expected, provided) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
        )
            .into_response();
    }

    let stats = state.store.stats();
    let day = state.store.day_stats();
    let resolvers = state.store.resolver_stats(RESOLVER_METRICS_LIMIT);
    let active_clients = state
        .store
        .recent(ACTIVE_CLIENT_WINDOW)
        .iter()
        .map(|event| event.client)
        .collect::<HashSet<_>>()
        .len() as u64;
    let (udp, tcp, dot, doh) = state.status.listeners();
    let feed = state.status.feed();
    let feed_last_refresh_unix = feed
        .last_refresh
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.timestamp());
    let feed_ok = feed
        .last_refresh
        .as_ref()
        .map(|_| feed.last_error.is_none());

    let body = metrics::render(&metrics::Snapshot {
        version: state.status.version(),
        uptime_secs: state.status.uptime_secs(),
        stats: &stats,
        day: &day,
        resolvers: &resolvers,
        cache_entries: state.cache.len(),
        lists: metrics::ListSizes {
            ads: state.blocker.ads_len() as u64,
            malware: state.blocker.malware_len() as u64,
            allow: state.blocker.allow_len() as u64,
            custom: state.blocker.custom_len() as u64,
        },
        active_clients,
        listeners: (udp, tcp, dot, doh),
        feed_last_refresh_unix,
        feed_ok,
    });

    ([(header::CONTENT_TYPE, metrics::CONTENT_TYPE)], body).into_response()
}

/// Serve the requested asset, falling back to `index.html` for client-side
/// routes (any path without a file extension) so the SPA can handle them.
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(response) = serve_asset(path) {
        return response;
    }

    // A path with no extension is a client route, not a missing file.
    if !last_segment_has_extension(path) {
        if let Some(response) = serve_asset("index.html") {
            return response;
        }
    }

    StatusCode::NOT_FOUND.into_response()
}

fn serve_asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let content_type = file.metadata.mimetype();
    Some(
        (
            [
                (header::CONTENT_TYPE, content_type.to_owned()),
                (header::CACHE_CONTROL, asset_cache_control(path).to_owned()),
            ],
            file.data,
        )
            .into_response(),
    )
}

/// The `Cache-Control` for a served asset. The app shell and the service worker
/// must always be revalidated, so a new release is picked up at once instead of a
/// stale copy keeping the installed PWA pinned to an old version (the cause of a
/// phone still showing a months-old version). Content-hashed build assets never
/// change under their name, so they are cached hard; everything else gets a short
/// day cache.
fn asset_cache_control(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    if path == "index.html"
        || name == "sw.js"
        || name == "registerSW.js"
        || name.ends_with(".webmanifest")
        || name == "manifest.json"
    {
        "no-cache"
    } else if path.starts_with("assets/") || name.starts_with("workbox-") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=86400"
    }
}

fn last_segment_has_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicU32, Ordering};

    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use justdns_store::QuerySource;
    use tower::ServiceExt;

    /// A blocker with a tiny ads list, for the handler tests.
    fn test_blocker() -> Arc<Blocker> {
        use std::sync::atomic::AtomicBool;
        Arc::new(Blocker::new(
            justdns_filter::DomainSet::from_rules("doubleclick.net\n"),
            Arc::new(AtomicBool::new(true)),
            justdns_filter::DomainSet::new(),
            Arc::new(AtomicBool::new(true)),
        ))
    }

    /// A throwaway config store backed by a unique temp file.
    fn test_config() -> ConfigStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-config-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        ConfigStore::load(
            path,
            Settings::default(),
            &justdns_secret::Sealer::ephemeral(),
        )
        .unwrap()
    }

    /// A throwaway user store backed by a unique temp file (no admin yet).
    fn test_users() -> UserStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-auth-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        UserStore::load(path, &justdns_secret::Sealer::ephemeral()).unwrap()
    }

    /// A throwaway list store backed by a unique temp directory, pushing its
    /// (empty) merged sets into `blocker` so the two share one engine.
    fn test_lists(blocker: Arc<Blocker>) -> ListStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir().join(format!(
            "justdns-web-lists-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        ListStore::load(base.join("lists.json"), base.join("cache"), blocker).unwrap()
    }

    /// A throwaway rewrite store backed by a unique temp file, with its own
    /// empty live table.
    fn test_rewrites() -> RewriteStore {
        use justdns_rewrite::Rewriter;
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-rewrites-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        RewriteStore::load(path, Arc::new(Rewriter::new())).unwrap()
    }

    /// A throwaway client (device) store backed by a unique temp file.
    fn test_clients() -> ClientStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-clients-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        ClientStore::load(path).unwrap()
    }

    /// A throwaway schedule store backed by a unique temp file.
    fn test_schedules() -> ScheduleStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-schedules-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        ScheduleStore::load(path).unwrap()
    }

    /// A throwaway audit log backed by a unique temp file.
    fn test_audit() -> AuditStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-audit-{}-{}.log",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        AuditStore::load(path).unwrap()
    }

    fn test_state(store: Store) -> AppState {
        let blocker = test_blocker();
        AppState {
            store,
            config: test_config(),
            lists: test_lists(blocker.clone()),
            rewrites: test_rewrites(),
            clients: test_clients(),
            schedules: test_schedules(),
            access: test_access(),
            cache: DnsCache::new(100),
            blocker,
            users: test_users(),
            sessions: SessionStore::new(justdns_auth::DEFAULT_SESSION_TTL),
            audit: test_audit(),
            throttle: LoginThrottle::new(),
            certs: None,
            cert_reload: None,
            dot_port: 853,
            doh_port: 8443,
            logs: LogBuffer::new(100),
            status: RuntimeStatus::new("test".into()),
            log_level: LogLevelControl::disabled("normal"),
            metrics_token: None,
            update: Arc::new(UpdateChecker::from_env("v0.0.0-test")),
            ha: Arc::new(ha::Ha::from_env("v0.0.0-test")),
            reports: test_reports(),
        }
    }

    /// A throwaway access-request store backed by a unique temp file.
    fn test_access() -> AccessStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-access-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        AccessStore::load(&path).unwrap()
    }

    /// A throwaway report store backed by a unique temp file.
    fn test_reports() -> ReportStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-reports-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        ReportStore::load(&path)
    }

    /// A router whose data API is already unlocked, plus the `Cookie` header
    /// value that unlocks it: the state carries an admin and a live session for
    /// them. Handler tests that predate the login guard attach this cookie.
    fn app_and_cookie(store: Store) -> (Router, String) {
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let token = state.sessions.create("admin");
        (router(state.clone()), format!("{SESSION_COOKIE}={token}"))
    }

    fn sample(client: Ipv4Addr, name: &str, source: QuerySource) -> QueryEvent {
        QueryEvent {
            at_ms: 1_700_000_000_000,
            client: IpAddr::V4(client),
            name: name.to_owned(),
            record_type: 1,
            response_code: 0,
            source,
            duration_ms: 5,
            answers: 1,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
        }
    }

    async fn body_of(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn bytes_of(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    async fn get(store: Store, path: &str) -> Response {
        let (app, cookie) = app_and_cookie(store);
        app.oneshot(
            Request::get(path)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn history_filters_and_paginates_over_the_durable_log() {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-history-{}-{}.duckdb",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));

        // Persist events to a file-backed log, then reopen so the history reader
        // sees the fully flushed rows — deterministic, with no reliance on the
        // async writer's flush timing.
        {
            let (store, writer) = Store::open(&path, 100).unwrap();
            let at = |at_ms, client, name, source| {
                let mut event = sample(client, name, source);
                event.at_ms = at_ms;
                event
            };
            store.record(at(
                10,
                Ipv4Addr::new(10, 0, 0, 1),
                "a.example",
                QuerySource::Blocked,
            ));
            store.record(at(
                20,
                Ipv4Addr::new(10, 0, 0, 2),
                "b.example",
                QuerySource::Upstream,
            ));
            store.record(at(
                30,
                Ipv4Addr::new(10, 0, 0, 3),
                "c.example",
                QuerySource::Cache,
            ));
            drop(store);
            writer.await.unwrap();
        }
        let (store, _writer) = Store::open(&path, 100).unwrap();

        // Newest-first page of two: c then b, a left for the next page, and a
        // cursor pointing past b.
        let response = get(store.clone(), "/api/queries/history?limit=2").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        assert!(body.contains("c.example"), "{body}");
        assert!(body.contains("b.example"), "{body}");
        assert!(
            !body.contains("a.example"),
            "only two on the first page: {body}"
        );
        assert!(
            body.contains("\"nextBefore\""),
            "a full page carries a cursor: {body}"
        );

        // Filter by source — only the blocked entry, and no cursor on the last page.
        let response = get(store.clone(), "/api/queries/history?source=blocked").await;
        let body = body_of(response).await;
        assert!(body.contains("a.example"), "{body}");
        assert!(!body.contains("b.example"), "{body}");
        assert!(
            !body.contains("\"nextBefore\""),
            "a short final page omits the cursor: {body}"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("duckdb.wal"));
    }

    #[tokio::test]
    async fn privacy_attributes_names_to_companies() {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-privacy-{}-{}.duckdb",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));

        let now = now_ms() as i64;
        {
            let (store, writer) = Store::open(&path, 100).unwrap();
            let at = |at_ms, client, name, source| {
                let mut event = sample(client, name, source);
                event.at_ms = at_ms;
                event
            };
            // Two Google lookups (one blocked), one Meta lookup that got through,
            // and a non-tracker name that must stay unattributed.
            store.record(at(
                now - 1000,
                Ipv4Addr::new(10, 0, 0, 1),
                "ad.doubleclick.net",
                QuerySource::Blocked,
            ));
            store.record(at(
                now - 2000,
                Ipv4Addr::new(10, 0, 0, 1),
                "pagead2.googlesyndication.com",
                QuerySource::Upstream,
            ));
            store.record(at(
                now - 3000,
                Ipv4Addr::new(10, 0, 0, 2),
                "graph.facebook.com",
                QuerySource::Upstream,
            ));
            store.record(at(
                now - 4000,
                Ipv4Addr::new(10, 0, 0, 2),
                "news.example.com",
                QuerySource::Cache,
            ));
            drop(store);
            writer.await.unwrap();
        }
        let (store, _writer) = Store::open(&path, 100).unwrap();

        let response = get(store.clone(), "/api/privacy/trackers?window=day").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        assert!(body.contains("Google"), "{body}");
        assert!(body.contains("Meta"), "{body}");
        assert!(
            !body.contains("news.example.com"),
            "a non-tracker name must not be attributed: {body}"
        );
        // Three tracker lookups in all, one of them blocked.
        assert!(body.contains("\"totalContacts\":3"), "{body}");
        assert!(body.contains("\"totalBlocked\":1"), "{body}");
        assert!(body.contains("\"totalAllowed\":2"), "{body}");

        // Two devices, each with its own company.
        assert!(body.contains("10.0.0.1"), "{body}");
        assert!(body.contains("10.0.0.2"), "{body}");

        // The company drill-down keeps the exact domains that were looked up, so
        // the detail view can list them.
        assert!(body.contains("ad.doubleclick.net"), "{body}");
        assert!(body.contains("pagead2.googlesyndication.com"), "{body}");
        assert!(body.contains("graph.facebook.com"), "{body}");
        assert!(body.contains("\"domains\""), "{body}");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("duckdb.wal"));
    }

    #[tokio::test]
    async fn privacy_block_adds_a_company_or_a_single_domain() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);

        let post = |path: &'static str, json: &'static str| {
            let app = app.clone();
            let cookie = cookie.clone();
            async move {
                app.oneshot(
                    Request::post(path)
                        .header("content-type", "application/json")
                        .header("cookie", &cookie)
                        .body(Body::from(json))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        // Block a whole company: every catalogue domain for Google is added.
        let res = post("/api/privacy/block", r#"{"org":"Google"}"#).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"added\""), "{body}");
        let first_added: u64 = serde_json::from_str::<serde_json::Value>(&body).unwrap()["added"]
            .as_u64()
            .unwrap();
        assert!(first_added >= 2, "Google has several domains: {body}");

        // The block list now holds a known Google domain.
        let res = post("/api/privacy/block", r#"{"org":"Google"}"#).await;
        let body = body_of(res).await;
        assert!(
            body.contains("\"added\":0"),
            "re-block adds nothing: {body}"
        );

        // A single domain can be blocked on its own.
        let res = post(
            "/api/privacy/block",
            r#"{"domain":"telemetry.example.com"}"#,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(body_of(res).await.contains("\"added\":1"));

        // An unknown company is a 404, and an empty body is a 400.
        let res = post("/api/privacy/block", r#"{"org":"NoSuchCo"}"#).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = post("/api/privacy/block", r#"{}"#).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // A single domain that does not parse as a host is a 400, not a silent 0.
        let res = post("/api/privacy/block", r#"{"domain":"not a host!!"}"#).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // The custom block list ended up with the Google domains.
        let res = app
            .oneshot(
                Request::get("/api/lists")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(body_of(res).await.contains("doubleclick.net"));
    }

    #[tokio::test]
    async fn privacy_block_can_target_one_device() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);

        let post = |path: &'static str, json: &'static str| {
            let app = app.clone();
            let cookie = cookie.clone();
            async move {
                app.oneshot(
                    Request::post(path)
                        .header("content-type", "application/json")
                        .header("cookie", &cookie)
                        .body(Body::from(json))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };
        let get = |path: &'static str| {
            let app = app.clone();
            let cookie = cookie.clone();
            async move {
                app.oneshot(
                    Request::get(path)
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        // Block a whole company for one device.
        let res = post(
            "/api/privacy/block",
            r#"{"org":"Google","client":"192.168.20.10"}"#,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let added =
            serde_json::from_str::<serde_json::Value>(&body_of(res).await).unwrap()["added"]
                .as_u64()
                .unwrap();
        assert!(added >= 2, "Google has several domains");

        // Re-blocking for the same device adds nothing.
        let res = post(
            "/api/privacy/block",
            r#"{"org":"Google","client":"192.168.20.10"}"#,
        )
        .await;
        assert!(body_of(res).await.contains("\"added\":0"));

        // The device now carries the Google domains in its own policy …
        let devices = body_of(get("/api/clients").await).await;
        assert!(devices.contains("192.168.20.10"), "{devices}");
        assert!(devices.contains("doubleclick.net"), "{devices}");

        // … while the global block list was left untouched.
        let lists = body_of(get("/api/lists").await).await;
        assert!(
            !lists.contains("doubleclick.net"),
            "per-device block must not touch the global list: {lists}"
        );
    }

    /// A router whose `/metrics` endpoint is configured with `token` (or off when
    /// `None`). No session is needed — the endpoint is public and gates itself.
    fn metrics_app(store: Store, token: Option<&str>) -> Router {
        let mut state = test_state(store);
        state.metrics_token = token.map(Arc::from);
        router(state)
    }

    async fn get_metrics(app: Router, authorization: Option<&str>) -> Response {
        let mut request = Request::get("/metrics");
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        app.oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn metrics_is_off_without_a_configured_token() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let response = get_metrics(metrics_app(store, None), Some("Bearer anything")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metrics_requires_the_right_bearer_token() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let app = metrics_app(store.clone(), Some("s3cr3t-token"));

        // No header at all.
        let response = get_metrics(app.clone(), None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Wrong token.
        let response = get_metrics(app, Some("Bearer nope")).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn metrics_serves_prometheus_text_with_the_token() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "a.example",
            QuerySource::Cache,
        ));
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 2),
            "b.example",
            QuerySource::Upstream,
        ));

        let app = metrics_app(store, Some("s3cr3t-token"));
        let response = get_metrics(app, Some("Bearer s3cr3t-token")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(content_type.contains("version=0.0.4"), "{content_type}");

        let body = body_of(response).await;
        assert!(
            body.contains("# TYPE justdns_queries_total counter"),
            "{body}"
        );
        assert!(body.contains("\njustdns_queries_total 2\n"), "{body}");
        assert!(body.contains("\njustdns_cache_hits_total 1\n"), "{body}");
        assert!(
            body.contains("justdns_dnssec_verdicts_total{verdict=\"secure\"} 0"),
            "{body}"
        );
        assert!(
            body.contains("justdns_listener_up{listener=\"udp\"}"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn metrics_uses_the_token_managed_in_the_config() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        // No env token: the endpoint is driven purely by the UI-managed config
        // token, which starts empty (off) and turns on once set.
        let state = test_state(store);
        assert!(state.metrics_token.is_none());
        let mut settings = state.config.get();
        settings.metrics_token = "ui-managed-token".to_owned();
        state.config.set(settings).unwrap();
        let app = router(state);

        let response = get_metrics(app.clone(), Some("Bearer wrong")).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = get_metrics(app, Some("Bearer ui-managed-token")).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn ha_peer_endpoints_are_closed_by_default() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        // HA is off by default, so the machine-to-machine handshake answers 404 —
        // the sync surface does not exist until the user enables and pairs it.
        // (The enabled+keyed+paired open path and key matching are covered by the
        // ha module's unit tests, which need no live peer.)
        let app = router(state);
        let response = app
            .oneshot(
                Request::get("/api/ha/hello")
                    .header("authorization", "Bearer whatever")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "off ⇒ 404");
    }

    #[tokio::test]
    async fn ha_syncs_users_with_mfa_to_a_peer() {
        // Node 1: an admin with MFA enrolled and confirmed.
        let (store1, _w1) = Store::in_memory(100).unwrap();
        let node1 = test_state(store1);
        node1.users.create_admin("admin", "hunter2hunter2").unwrap();
        let secret = justdns_auth::mfa::generate_secret();
        let codes = justdns_auth::mfa::generate_recovery_codes();
        node1.users.begin_mfa("admin", &secret, &codes).unwrap();
        node1.users.confirm_mfa("admin").unwrap();

        // The admin, its TOTP secret and all, is emitted as a `user` entry.
        let collections = ha_collections(&node1);
        let entry = collections
            .get("user")
            .and_then(|users| users.get("admin"))
            .expect("the admin is a user collection entry");
        assert_eq!(entry["username"], "admin");
        assert!(
            entry["mfa"]["secret"].is_string(),
            "the TOTP secret travels"
        );
        assert!(entry["mfa"]["confirmed"].as_bool().unwrap());

        // Node 2 starts with its own MFA-less admin of the same name — exactly the
        // reported situation. Applying node 1's merged set adopts the enrolled
        // record, so MFA is now in force on node 2 and node 1's password wins.
        let (store2, _w2) = Store::in_memory(100).unwrap();
        let node2 = test_state(store2);
        node2
            .users
            .create_admin("admin", "node2-local-pass")
            .unwrap();
        assert!(!node2.users.mfa_enabled("admin"), "no MFA on node 2 yet");

        ha_apply(&node2, &collections).unwrap();

        assert!(
            node2.users.mfa_enabled("admin"),
            "MFA propagated from node 1 to node 2"
        );
        assert!(
            node2.users.verify("admin", "hunter2hunter2").is_some(),
            "node 1's password now authenticates on node 2"
        );
        assert!(
            node2.users.verify("admin", "node2-local-pass").is_none(),
            "node 2's old password no longer works after the merge"
        );
    }

    #[tokio::test]
    async fn ha_sync_never_strips_confirmed_mfa_and_both_nodes_converge_on_it() {
        // The reported regression: node A's admin has confirmed MFA, node B's admin
        // (same name) has none, and B's record carries the LATER timestamp — so a
        // plain last-writer-wins hands the win to B and MFA vanishes everywhere.
        let (store_a, _wa) = Store::in_memory(100).unwrap();
        let node_a = test_state(store_a);
        node_a
            .users
            .create_admin("admin", "alpha-pass-1234")
            .unwrap();
        let secret = justdns_auth::mfa::generate_secret();
        let codes = justdns_auth::mfa::generate_recovery_codes();
        node_a.users.begin_mfa("admin", &secret, &codes).unwrap();
        node_a.users.confirm_mfa("admin").unwrap();

        let (store_b, _wb) = Store::in_memory(100).unwrap();
        let node_b = test_state(store_b);
        node_b
            .users
            .create_admin("admin", "bravo-pass-1234")
            .unwrap();

        let cols_a = ha_collections(&node_a);
        let cols_b = ha_collections(&node_b);
        // B's admin is the later writer, so the raw merge resolves to B (no MFA).
        let snap_a = ha::SyncSnapshot {
            node_id: "a".into(),
            collections: cols_a.clone(),
            ledger: ha::restamp(&cols_a, &ha::Ledger::new(), "a", 100),
            version: String::new(),
            roster: std::collections::BTreeMap::new(),
        };
        let snap_b = ha::SyncSnapshot {
            node_id: "b".into(),
            collections: cols_b.clone(),
            ledger: ha::restamp(&cols_b, &ha::Ledger::new(), "b", 200),
            version: String::new(),
            roster: std::collections::BTreeMap::new(),
        };

        let raw = ha::merge(&snap_a, &snap_b);
        assert!(
            raw.collections["user"]["admin"]["mfa"].is_null(),
            "sanity: raw last-writer-wins would drop MFA (B won)"
        );

        // Reconciliation, run from each node's point of view (local/peer swapped),
        // must restore confirmed MFA — and to the SAME record, so the nodes agree.
        let reconcile = |local: &BTreeMap<String, ha::Collection>,
                         remote: &BTreeMap<String, ha::Collection>| {
            let mut merged = raw.collections.clone();
            ha_reconcile_mfa(&mut merged, local, remote);
            merged
        };
        let from_a = reconcile(&cols_a, &cols_b);
        let from_b = reconcile(&cols_b, &cols_a);

        for merged in [&from_a, &from_b] {
            let admin = &merged["user"]["admin"];
            assert_eq!(admin["mfa"]["secret"], secret, "the TOTP secret is kept");
            assert!(
                admin["mfa"]["confirmed"].as_bool().unwrap(),
                "and confirmed"
            );
            // Non-MFA fields still follow last-writer-wins: B's password remains.
            assert_eq!(
                admin["password_hash"], raw.collections["user"]["admin"]["password_hash"],
                "only MFA is reconciled; the rest stays as the merge decided",
            );
        }
        assert_eq!(
            from_a, from_b,
            "both nodes reconcile to the identical record"
        );

        // Applied on node B, MFA is now actually in force there.
        ha_apply(&node_b, &from_b).unwrap();
        assert!(
            node_b.users.mfa_enabled("admin"),
            "MFA now active on node B"
        );
    }

    #[tokio::test]
    async fn ha_reconcile_leaves_mfa_free_accounts_untouched() {
        // With MFA nowhere, reconciliation must not invent a second factor.
        let (store, _w) = Store::in_memory(100).unwrap();
        let node = test_state(store);
        node.users.create_admin("admin", "plain-pass-1234").unwrap();
        let cols = ha_collections(&node);
        let mut merged = cols.clone();
        ha_reconcile_mfa(&mut merged, &cols, &cols);
        assert!(
            merged["user"]["admin"]["mfa"].is_null(),
            "no MFA is conjured from nothing",
        );
    }

    #[tokio::test]
    async fn ha_apply_without_a_user_collection_leaves_accounts_alone() {
        // A peer that predates user sync sends no `user` collection. Applying such
        // a set must not wipe the node's own accounts.
        let (store, _w) = Store::in_memory(100).unwrap();
        let node = test_state(store);
        node.users.create_admin("admin", "hunter2hunter2").unwrap();
        let mut collections = ha_collections(&node);
        collections.remove("user");
        ha_apply(&node, &collections).unwrap();
        assert!(
            node.users.exists("admin"),
            "accounts survive a userless peer"
        );
    }

    #[tokio::test]
    async fn ha_rejects_a_short_sync_key() {
        let (store, _w) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);
        // The key gates the credential-carrying snapshot, so a short one is refused.
        let short = app
            .clone()
            .oneshot(
                Request::put("/api/ha/key")
                    .header("cookie", &cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"key":"short"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(short.status(), StatusCode::BAD_REQUEST);

        // A sufficiently long key is accepted.
        let ok = app
            .oneshot(
                Request::put("/api/ha/key")
                    .header("cookie", &cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"key":"a-long-enough-sync-key-123"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn ha_keeps_instance_name_and_cert_host_node_local() {
        // Node 1 has its own display name and DoT/DoH hostname.
        let (store1, _w1) = Store::in_memory(100).unwrap();
        let node1 = test_state(store1);
        let mut s1 = node1.config.get();
        s1.instance_name = "Node One".into();
        s1.encrypted_dns.hostname = "node1.local".into();
        node1.config.set(s1).unwrap();

        // The synced settings value carries neither the name nor the cert host.
        let collections = ha_collections(&node1);
        let settings = collections
            .get("settings")
            .and_then(|m| m.get("settings"))
            .expect("a settings entry");
        assert_eq!(settings["instanceName"], "");
        assert_eq!(settings["encryptedDns"]["hostname"], "");

        // Node 2 applies node 1's snapshot but keeps its own identity fields — the
        // name and certificate host never cross the sync.
        let (store2, _w2) = Store::in_memory(100).unwrap();
        let node2 = test_state(store2);
        let mut s2 = node2.config.get();
        s2.instance_name = "Node Two".into();
        s2.encrypted_dns.hostname = "node2.local".into();
        node2.config.set(s2).unwrap();
        ha_apply(&node2, &collections).unwrap();
        let got = node2.config.get();
        assert_eq!(got.instance_name, "Node Two");
        assert_eq!(got.encrypted_dns.hostname, "node2.local");
    }

    #[tokio::test]
    async fn ha_shares_block_page_address_but_keeps_server_ip_local() {
        // Node 1 configures the block page: the shared fields (enable, mode and the
        // landing "Block-page address") plus its own node-local server IP.
        let (store1, _w1) = Store::in_memory(100).unwrap();
        let node1 = test_state(store1);
        let mut s1 = node1.config.get();
        s1.block_page.enabled = true;
        s1.block_page.redirect_url = "https://block.justfuture.at".into();
        s1.block_page.behind_reverse_proxy = true;
        s1.block_page.title = "Gesperrt".into();
        s1.block_page.server_ipv4 = "192.168.20.5".into();
        node1.config.set(s1).unwrap();

        // The synced snapshot carries the address, mode, enable flag and title, but
        // blanks the node-local server IP.
        let collections = ha_collections(&node1);
        let settings = collections
            .get("settings")
            .and_then(|m| m.get("settings"))
            .expect("a settings entry");
        assert_eq!(
            settings["blockPage"]["redirectUrl"],
            "https://block.justfuture.at"
        );
        assert_eq!(settings["blockPage"]["behindReverseProxy"], true);
        assert_eq!(settings["blockPage"]["enabled"], true);
        assert_eq!(settings["blockPage"]["title"], "Gesperrt");
        assert_eq!(settings["blockPage"]["serverIpv4"], "");

        // Node 2 has its own server IP. Applying node 1's snapshot adopts the shared
        // address, mode, flag and title, but keeps its own server IP — each node
        // points blocked names at itself.
        let (store2, _w2) = Store::in_memory(100).unwrap();
        let node2 = test_state(store2);
        let mut s2 = node2.config.get();
        s2.block_page.server_ipv4 = "192.168.20.25".into();
        node2.config.set(s2).unwrap();
        ha_apply(&node2, &collections).unwrap();
        let got = node2.config.get();
        assert_eq!(got.block_page.redirect_url, "https://block.justfuture.at");
        assert!(got.block_page.behind_reverse_proxy);
        assert!(got.block_page.enabled);
        assert_eq!(got.block_page.title, "Gesperrt");
        assert_eq!(got.block_page.server_ipv4, "192.168.20.25");
    }

    #[tokio::test]
    async fn ha_block_page_survives_unrelated_settings_change_on_peer() {
        // The block page travels as its own collection entry with an independent
        // timestamp. A peer that changed some *other* setting (so its settings blob
        // is newer and carries the block page turned off there) must not switch this
        // node's block page off: `ha_apply` applies the separate "blockPage" entry.
        let (store, _w) = Store::in_memory(100).unwrap();
        let node = test_state(store);
        let mut local = node.config.get();
        local.block_page.enabled = true;
        local.block_page.server_ipv4 = "192.168.20.25".into();
        node.config.set(local).unwrap();

        // An incoming snapshot whose settings blob disagrees (block page off) but
        // whose own "blockPage" entry keeps it on — the shape a peer produces after
        // an unrelated settings save while the block page is enabled cluster-wide.
        let mut collections = ha_collections(&node);
        collections
            .get_mut("settings")
            .and_then(|m| m.get_mut("settings"))
            .map(|value| value["blockPage"]["enabled"] = serde_json::Value::Bool(false))
            .expect("a settings entry");

        ha_apply(&node, &collections).unwrap();
        let got = node.config.get();
        assert!(
            got.block_page.enabled,
            "the separate block-page entry must win over the settings blob"
        );
        assert_eq!(got.block_page.server_ipv4, "192.168.20.25");
    }

    #[tokio::test]
    async fn ha_block_page_falls_back_to_settings_blob_for_old_peer() {
        // A peer too old to emit a separate "blockPage" entry carries the block page
        // only inside its settings blob. `ha_apply` must still adopt it from there.
        let (store, _w) = Store::in_memory(100).unwrap();
        let node = test_state(store);

        let mut collections = ha_collections(&node);
        collections.remove("blockPage");
        collections
            .get_mut("settings")
            .and_then(|m| m.get_mut("settings"))
            .map(|value| value["blockPage"]["enabled"] = serde_json::Value::Bool(true))
            .expect("a settings entry");

        ha_apply(&node, &collections).unwrap();
        assert!(node.config.get().block_page.enabled);
    }

    #[tokio::test]
    async fn stats_counts_recorded_queries() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "a.example",
            QuerySource::Cache,
        ));
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 2),
            "b.example",
            QuerySource::Upstream,
        ));

        let response = get(store, "/api/stats").await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_of(response).await;
        assert!(body.contains("\"total\":2"), "{body}");
        assert!(body.contains("\"cache_hits\":1"), "{body}");
        assert!(body.contains("\"active_clients\":2"), "{body}");
        assert!(body.contains("\"blocked\":0"), "{body}");
    }

    #[tokio::test]
    async fn top_domains_ranks_a_single_clients_lookups() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let client = Ipv4Addr::new(10, 0, 0, 1);
        store.record(sample(client, "news.example.", QuerySource::Upstream));
        store.record(sample(client, "news.example.", QuerySource::Cache));
        store.record(sample(client, "ads.example.", QuerySource::Upstream));
        // Another client's lookup must not appear in .1's ranking.
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 2),
            "other.example.",
            QuerySource::Upstream,
        ));

        let response = get(store, "/api/stats/top-domains?client=10.0.0.1").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        assert!(body.contains("\"domain\":\"news.example.\""), "{body}");
        assert!(body.contains("\"count\":2"), "{body}");
        assert!(body.contains("ads.example."), "{body}");
        assert!(
            !body.contains("other.example."),
            "other client leaked: {body}"
        );
    }

    #[tokio::test]
    async fn top_domains_unknown_client_is_empty() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "news.example.",
            QuerySource::Upstream,
        ));
        // A never-seen client (and a missing/garbled one) yields an empty list.
        let response = get(store, "/api/stats/top-domains?client=10.0.0.9").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_of(response).await, "[]");
    }

    #[tokio::test]
    async fn resolvers_reports_per_upstream_counts() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let answered = |resolver: &str| {
            let mut event = sample(
                Ipv4Addr::new(10, 0, 0, 1),
                "example.com.",
                QuerySource::Upstream,
            );
            event.resolver = Some(resolver.to_owned());
            event
        };
        store.record(answered("dns.quad9.net"));
        store.record(answered("dns.quad9.net"));
        store.record(answered("protective.joindns4.eu"));
        // A cache hit carries no resolver and must not appear.
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "example.com.",
            QuerySource::Cache,
        ));

        let response = get(store, "/api/stats/resolvers").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        // Busiest resolver first, with its count; both upstreams present.
        assert!(body.contains("\"resolver\":\"dns.quad9.net\""), "{body}");
        assert!(body.contains("\"count\":2"), "{body}");
        assert!(body.contains("protective.joindns4.eu"), "{body}");
        let quad9 = body.find("dns.quad9.net").unwrap();
        let dns4eu = body.find("protective.joindns4.eu").unwrap();
        assert!(quad9 < dns4eu, "busiest resolver listed first: {body}");
    }

    #[tokio::test]
    async fn resolvers_empty_without_upstream_answers() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "example.com.",
            QuerySource::Cache,
        ));
        let response = get(store, "/api/stats/resolvers").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_of(response).await, "[]");
    }

    #[tokio::test]
    async fn stats_day_reports_window_blocked_average_and_top_clients() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        // Two queries from .1 (one of them blocked) and one from .2; every sample
        // has a 5 ms handling time, so the mean across all three is 5.
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "a.example",
            QuerySource::Upstream,
        ));
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "ad.doubleclick.net",
            QuerySource::Blocked,
        ));
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 2),
            "b.example",
            QuerySource::Cache,
        ));

        let body = body_of(get(store, "/api/stats/day").await).await;
        assert!(body.contains("\"total\":3"), "{body}");
        assert!(body.contains("\"blocked\":1"), "{body}");
        assert!(body.contains("\"avg_duration_ms\":5"), "{body}");
        // Busiest client first, with its count.
        assert!(
            body.contains("\"client\":\"10.0.0.1\",\"count\":2"),
            "{body}"
        );
        assert!(
            body.contains("\"client\":\"10.0.0.2\",\"count\":1"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn queries_respects_the_limit() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        for i in 0..5 {
            store.record(sample(
                Ipv4Addr::new(10, 0, 0, 1),
                &format!("q{i}.example"),
                QuerySource::Upstream,
            ));
        }

        let response = get(store, "/api/queries?limit=2").await;
        let body = body_of(response).await;
        assert_eq!(body.matches("\"name\"").count(), 2, "{body}");
    }

    #[tokio::test]
    async fn queries_filter_matches_name_or_client() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 1),
            "news.example",
            QuerySource::Upstream,
        ));
        store.record(sample(
            Ipv4Addr::new(10, 0, 0, 2),
            "shop.other",
            QuerySource::Cache,
        ));

        // Match on the queried name, case-insensitively.
        let body = body_of(get(store.clone(), "/api/queries?q=NEWS").await).await;
        assert!(body.contains("news.example"), "{body}");
        assert!(!body.contains("shop.other"), "{body}");

        // Match on the client address.
        let body = body_of(get(store.clone(), "/api/queries?q=0.0.2").await).await;
        assert!(body.contains("shop.other"), "{body}");
        assert!(!body.contains("news.example"), "{body}");

        // A blank filter behaves like no filter.
        let body = body_of(get(store, "/api/queries?q=").await).await;
        assert_eq!(body.matches("\"name\"").count(), 2, "{body}");
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        // Clone the router (not rebuild it) so PUT and the later GET share one
        // config handle, and reuse one session cookie across the requests.
        let (app, cookie) = app_and_cookie(store);

        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/settings")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"ipv6\":false"), "defaults: {body}");

        let put = Request::put("/api/settings")
            .header("content-type", "application/json")
            .header("cookie", &cookie)
            .body(Body::from(
                r#"{"blockAdsTrackers":true,"safeSearch":false,"blockMalware":true,"upstreamMode":"fallback","encryptUpstream":true,"ipv6":true,"queryLog":true,"logRetentionDays":14,"cacheServeStale":false}"#,
            ))
            .unwrap();
        let res = app.clone().oneshot(put).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"ipv6\":true"), "echo: {body}");
        assert!(
            body.contains("\"upstreamMode\":\"fallback\""),
            "echo: {body}"
        );

        let body = body_of(
            app.oneshot(
                Request::get("/api/settings")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body.contains("\"ipv6\":true"), "persisted: {body}");
        assert!(
            body.contains("\"logRetentionDays\":14"),
            "persisted: {body}"
        );
    }

    #[tokio::test]
    async fn classify_reports_the_list_and_rule() {
        let (store, _writer) = Store::in_memory(100).unwrap();

        // A sub-domain of a listed ad domain: reports the category and the rule.
        let body =
            body_of(get(store.clone(), "/api/classify?name=ad.doubleclick.net.").await).await;
        assert!(body.contains("\"list\":\"ads\""), "{body}");
        assert!(body.contains("\"rule\":\"doubleclick.net\""), "{body}");

        // A name on no list classifies to null.
        let body = body_of(get(store, "/api/classify?name=example.com").await).await;
        assert_eq!(body, "null", "{body}");
    }

    #[tokio::test]
    async fn lists_view_includes_the_preset_catalogue() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let body = body_of(get(store, "/api/lists").await).await;
        assert!(body.contains("\"allow\":[]"), "{body}");
        assert!(
            body.contains("hagezi-light"),
            "catalogue is present: {body}"
        );
        assert!(
            body.contains("falsePositive"),
            "FP impact described: {body}"
        );
    }

    #[tokio::test]
    async fn adding_an_allow_entry_via_the_api_unblocks_live() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        // One app/state across the calls so the allow edit and the later
        // classify share the one blocker.
        let (app, cookie) = app_and_cookie(store);

        // doubleclick.net is on the test ads list, so it classifies as blocked.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/classify?name=doubleclick.net")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"list\":\"ads\""), "blocked first: {body}");

        // Whitelist it through the API (the log's "Whitelisten" button path).
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/lists/allow")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"domain":"doubleclick.net"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(
            body.contains("doubleclick.net"),
            "echoed in the view: {body}"
        );

        // Now it classifies as allowed (the allow set wins).
        let body = body_of(
            app.oneshot(
                Request::get("/api/classify?name=doubleclick.net")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(body, "null", "allow entry wins, live: {body}");
    }

    #[tokio::test]
    async fn adding_a_block_entry_via_the_api_is_rejected_when_invalid() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);
        let res = app
            .oneshot(
                Request::post("/api/lists/block")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"domain":"not a domain"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_note_rides_along_on_add_and_can_be_edited() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);

        // Add a block entry carrying a note.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/lists/block")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"domain":"tracker.test","note":"smart-TV phone-home"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("smart-TV phone-home"), "note echoed: {body}");

        // Edit the note in place via PUT on the same path.
        let res = app
            .oneshot(
                Request::put("/api/lists/block")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"domain":"tracker.test","note":"vendor telemetry"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(
            body.contains("vendor telemetry") && !body.contains("smart-TV"),
            "note replaced: {body}"
        );
    }

    #[tokio::test]
    async fn rewrites_round_trip_through_the_api() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);

        // Empty to start with.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/rewrites")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"rewrites\":[]"), "{body}");

        // Add two names at once sharing one address; both come back normalised.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/rewrites")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"names":["NAS.home.","PiHole.home"],"ips":["192.168.20.5"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"nas.home\""), "normalised + echoed: {body}");
        assert!(body.contains("\"pihole.home\""), "second name: {body}");
        assert!(body.contains("192.168.20.5"), "{body}");
        assert!(
            body.contains("\"enabled\":true"),
            "enabled by default: {body}"
        );

        // A bad IP is refused.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/rewrites")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"names":["x.home"],"ips":["nope"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Disable one of them; it stays listed but is marked off.
        let res = app
            .clone()
            .oneshot(
                Request::put("/api/rewrites/enabled")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"name":"nas.home","enabled":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"enabled\":false"), "disabled: {body}");

        // Rename it; the old name is gone, the new one is present.
        let res = app
            .clone()
            .oneshot(
                Request::put("/api/rewrites")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"oldName":"nas.home","name":"storage.home","ips":["192.168.20.9"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"storage.home\""), "renamed: {body}");
        assert!(!body.contains("\"nas.home\""), "old name gone: {body}");

        // Delete both; the view is empty again.
        for name in ["storage.home", "pihole.home"] {
            let res = app
                .clone()
                .oneshot(
                    Request::delete("/api/rewrites")
                        .header("content-type", "application/json")
                        .header("cookie", &cookie)
                        .body(Body::from(format!(r#"{{"name":"{name}"}}"#)))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }
        let body = body_of(
            app.oneshot(
                Request::get("/api/rewrites")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(body.contains("\"rewrites\":[]"), "{body}");
    }

    #[tokio::test]
    async fn address_objects_round_trip_and_guard_their_deletion() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);

        // Create an address object.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/aliases")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"name":"reverse-proxy","ips":["192.168.20.9"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"reverse-proxy\""), "object listed: {body}");

        // Point a rewrite at it with no address of its own.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/rewrites")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"names":["blog.home"],"aliases":["reverse-proxy"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"blog.home\""), "{body}");
        assert!(body.contains("\"aliases\":[\"reverse-proxy\"]"), "{body}");

        // A reference to an object that does not exist is refused.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/rewrites")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"names":["x.home"],"aliases":["ghost"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Deleting an object that is still in use is refused with 409, and the
        // message names the rewrite that still points at it.
        let res = app
            .clone()
            .oneshot(
                Request::delete("/api/aliases")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"name":"reverse-proxy"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let body = body_of(res).await;
        assert!(
            body.contains("blog.home"),
            "names the referencing rewrite: {body}"
        );

        // Changing the object's address updates what the rewrite resolves to.
        let res = app
            .clone()
            .oneshot(
                Request::put("/api/aliases")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"oldName":"reverse-proxy","name":"reverse-proxy","ips":["192.168.20.50"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(
            body.contains("192.168.20.50"),
            "object address changed: {body}"
        );
    }

    #[tokio::test]
    async fn devices_round_trip_and_name_the_log_and_top_clients() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        // Two clients made queries: .10 (twice) and .20 (once).
        store.record(sample(
            Ipv4Addr::new(192, 168, 20, 10),
            "a.example",
            QuerySource::Upstream,
        ));
        store.record(sample(
            Ipv4Addr::new(192, 168, 20, 10),
            "b.example",
            QuerySource::Cache,
        ));
        store.record(sample(
            Ipv4Addr::new(192, 168, 20, 20),
            "c.example",
            QuerySource::Upstream,
        ));
        let (app, cookie) = app_and_cookie(store);

        // Before naming, both clients show up unnamed in the devices view.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/clients")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"192.168.20.10\""), "unnamed client: {body}");
        assert!(body.contains("\"known\":false"), "{body}");
        assert!(body.contains("\"activeWindowSecs\":300"), "{body}");

        // Name .10 as a laptop; the response lists it as a known device.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"target":"192.168.20.10","name":"Justins Laptop","kind":"laptop"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("Justins Laptop"), "named: {body}");
        assert!(body.contains("\"known\":true"), "{body}");
        // The named client folds its two queries into one row.
        assert!(body.contains("\"count\":2"), "folded count: {body}");

        // The log now carries the device name for that client's queries.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/queries")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(
            body.contains("\"device_name\":\"Justins Laptop\""),
            "{body}"
        );

        // The log search matches the device name, returning that client's queries.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/queries?q=laptop")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("a.example"), "device-name search hit: {body}");
        assert!(!body.contains("c.example"), "other client excluded: {body}");

        // The overview's top clients carry the name too.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/stats/day")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"name\":\"Justins Laptop\""), "{body}");

        // A CIDR device names a whole range: .20 falls under it.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"target":"192.168.20.0/24","name":"Heimnetz","kind":"other"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        // .20 now resolves to the range (…10 keeps its exact name).
        assert!(body.contains("Heimnetz"), "range listed: {body}");

        // A bad target is refused.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"target":"nope","name":"X"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Delete the laptop; it goes back to being an unnamed sighting.
        let res = app
            .clone()
            .oneshot(
                Request::delete("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"target":"192.168.20.10"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(!body.contains("Justins Laptop"), "name gone: {body}");
    }

    #[tokio::test]
    async fn groups_and_device_policies_round_trip_through_the_api() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);

        // The devices view carries the list catalogue, with the two built-in
        // objects, so the editor knows what a scope can switch.
        let body = body_of(
            app.clone()
                .oneshot(
                    Request::get("/api/clients")
                        .header("cookie", &cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"groups\":[]"), "no groups yet: {body}");
        assert!(body.contains("\"id\":\"ads\""), "ads in catalogue: {body}");
        assert!(
            body.contains("\"id\":\"malware\""),
            "malware in catalogue: {body}"
        );

        // Create a group that disables the ads object and blocks one domain.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/groups")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"name":"Kinderzimmer","listsDisabled":["ads"],"block":["spiele.test"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("Kinderzimmer"), "group listed: {body}");
        // Capture the generated group id for the device assignment.
        let group_id = {
            let value: serde_json::Value = serde_json::from_str(&body).unwrap();
            value["groups"][0]["id"].as_str().unwrap().to_owned()
        };

        // Put a device in the group and give it its own allow + added list.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(format!(
                        r#"{{"target":"192.168.20.30","name":"Tablet","kind":"tablet","policy":{{"group":"{group_id}","listsAdded":["malware"],"allow":["ok.test"]}}}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("Tablet"), "device named: {body}");
        assert!(
            body.contains(&format!("\"group\":\"{group_id}\"")),
            "device grouped: {body}"
        );
        assert!(
            body.contains("\"listsAdded\":[\"malware\"]"),
            "delta: {body}"
        );
        assert!(
            body.contains("\"allow\":[\"ok.test\"]"),
            "delta allow: {body}"
        );

        // An unknown group on a device is a bad request.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"target":"192.168.20.31","name":"X","policy":{"group":"ghost"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Delete the group; the device is detached but kept.
        let res = app
            .clone()
            .oneshot(
                Request::delete("/api/groups")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(format!(r#"{{"id":"{group_id}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"groups\":[]"), "group gone: {body}");
        assert!(body.contains("Tablet"), "device kept: {body}");
        assert!(
            !body.contains(&format!("\"group\":\"{group_id}\"")),
            "device detached: {body}"
        );
    }

    #[tokio::test]
    async fn backup_exports_and_restores_into_a_fresh_instance() {
        // Instance A: set a custom block entry (with a note), a rewrite and a
        // couple of settings, then export a backup.
        let (store_a, _writer_a) = Store::in_memory(100).unwrap();
        let (app_a, cookie_a) = app_and_cookie(store_a);

        let res = app_a
            .clone()
            .oneshot(
                Request::post("/api/lists/block")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_a)
                    .body(Body::from(r#"{"domain":"tracker.test","note":"smart-TV"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app_a
            .clone()
            .oneshot(
                Request::post("/api/rewrites")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_a)
                    .body(Body::from(
                        r#"{"names":["nas.home"],"ips":["192.168.20.5"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app_a
            .clone()
            .oneshot(
                Request::put("/api/settings")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_a)
                    .body(Body::from(r#"{"ipv6":true,"logRetentionDays":21}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // A group with a delta, and a device placed in it with its own delta, so
        // the backup carries the whole policy picture.
        let res = app_a
            .clone()
            .oneshot(
                Request::post("/api/groups")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_a)
                    .body(Body::from(
                        r#"{"name":"Gaeste","listsDisabled":["malware"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let group_id = {
            let body = body_of(res).await;
            let value: serde_json::Value = serde_json::from_str(&body).unwrap();
            value["groups"][0]["id"].as_str().unwrap().to_owned()
        };
        let res = app_a
            .clone()
            .oneshot(
                Request::post("/api/clients")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_a)
                    .body(Body::from(format!(
                        r#"{{"target":"192.168.20.40","name":"Gastgeraet","policy":{{"group":"{group_id}","block":["werbung.test"]}}}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let backup = bytes_of(
            app_a
                .oneshot(
                    Request::post("/api/backup")
                        .header("content-type", "application/json")
                        .header("cookie", &cookie_a)
                        .body(Body::from(r#"{"password":"backup-pass-123"}"#))
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        // The downloaded file is encrypted: it carries the backup magic and none
        // of the plaintext leaks through.
        assert!(
            justdns_secret::is_backup_encrypted(&backup),
            "backup file is password-encrypted"
        );
        let as_text = String::from_utf8_lossy(&backup);
        assert!(!as_text.contains("tracker.test"), "no plaintext lists leak");
        assert!(!as_text.contains("Gaeste"), "no plaintext group leaks");

        // Instance B: a fresh box. Restoring the (encrypted) backup with its
        // password brings everything back.
        let (store_b, _writer_b) = Store::in_memory(100).unwrap();
        let (app_b, cookie_b) = app_and_cookie(store_b);

        let restore_body = serde_json::json!({
            "file": base64::engine::general_purpose::STANDARD.encode(&backup),
            "password": "backup-pass-123",
        })
        .to_string();
        let res = app_b
            .clone()
            .oneshot(
                Request::post("/api/backup/restore")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_b)
                    .body(Body::from(restore_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let lists = body_of(
            app_b
                .clone()
                .oneshot(
                    Request::get("/api/lists")
                        .header("cookie", &cookie_b)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(lists.contains("tracker.test"), "block restored: {lists}");
        assert!(lists.contains("smart-TV"), "note restored: {lists}");

        let rewrites = body_of(
            app_b
                .clone()
                .oneshot(
                    Request::get("/api/rewrites")
                        .header("cookie", &cookie_b)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(
            rewrites.contains("nas.home"),
            "rewrite restored: {rewrites}"
        );

        let settings = body_of(
            app_b
                .clone()
                .oneshot(
                    Request::get("/api/settings")
                        .header("cookie", &cookie_b)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(
            settings.contains("\"ipv6\":true"),
            "settings restored: {settings}"
        );
        assert!(
            settings.contains("\"logRetentionDays\":21"),
            "settings restored: {settings}"
        );

        // The group and the device's delta came back together.
        let clients = body_of(
            app_b
                .oneshot(
                    Request::get("/api/clients")
                        .header("cookie", &cookie_b)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(clients.contains("Gaeste"), "group restored: {clients}");
        assert!(clients.contains("Gastgeraet"), "device restored: {clients}");
        assert!(
            clients.contains("\"block\":[\"werbung.test\"]"),
            "device delta restored: {clients}"
        );
    }

    #[tokio::test]
    async fn restore_rejects_a_file_that_is_not_ours() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);
        // Well-formed (legacy plaintext) backup, right shape, but a foreign `app`
        // marker: refused before anything is written.
        let foreign =
            r#"{"app":"evil","kind":"backup","version":1,"settings":{},"lists":{},"rewrites":[]}"#;
        let body = serde_json::json!({
            "file": base64::engine::general_purpose::STANDARD.encode(foreign),
        })
        .to_string();
        let res = app
            .oneshot(
                Request::post("/api/backup/restore")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn clearing_the_cache_returns_no_content() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);
        let res = app
            .oneshot(
                Request::post("/api/cache/clear")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    /// The `Set-Cookie` value a response sends for the session cookie, if any.
    fn set_cookie(response: &Response) -> Option<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with(SESSION_COOKIE))
            .map(|value| value.to_owned())
    }

    #[tokio::test]
    async fn data_api_is_locked_without_a_session() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        // An admin exists, but this request carries no cookie.
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let app = router(state);

        let res = app
            .oneshot(Request::get("/api/stats").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn setup_creates_admin_logs_in_then_guards_the_api() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let app = router(test_state(store));

        // Before setup: unconfigured and unauthenticated.
        let body = body_of(
            app.clone()
                .oneshot(Request::get("/api/auth/state").body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"configured\":false"), "{body}");

        // Setup succeeds and hands back a session cookie.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/auth/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"hunter2hunter2"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = set_cookie(&res).expect("setup sets a session cookie");
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Strict"), "{cookie}");

        // That cookie unlocks the data API.
        let res = app
            .clone()
            .oneshot(
                Request::get("/api/stats")
                    .header("cookie", cookie.split(';').next().unwrap())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // A second setup attempt is refused now that an admin exists.
        let res = app
            .oneshot(
                Request::post("/api/auth/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"eve","password":"hunter2hunter2"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn login_rejects_bad_credentials_and_accepts_good_ones() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let app = router(state);

        // Wrong password: 401, no cookie.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"username":"admin","password":"wrong"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(set_cookie(&res).is_none());

        // Right password: 200 with a session cookie.
        let res = app
            .oneshot(
                Request::post("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"hunter2hunter2"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(set_cookie(&res).is_some());
    }

    #[tokio::test]
    async fn logout_ends_the_session() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let (app, cookie) = app_and_cookie(store);
        let bare = cookie.split(';').next().unwrap().to_owned();

        // Logged in: the API is reachable.
        let res = app
            .clone()
            .oneshot(
                Request::get("/api/stats")
                    .header("cookie", &bare)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Log out, then the same cookie no longer unlocks the API.
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/auth/logout")
                    .header("cookie", &bare)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let res = app
            .oneshot(
                Request::get("/api/stats")
                    .header("cookie", &bare)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn client_routes_have_no_extension() {
        assert!(!last_segment_has_extension("einstellungen"));
        assert!(!last_segment_has_extension("foo/bar"));
        assert!(last_segment_has_extension("assets/index-abc.js"));
        assert!(last_segment_has_extension("favicon.svg"));
    }

    // ---- Roles, authorization and the MFA login flow ----------------------

    /// A cookie header value for a full session as `user`.
    fn cookie_for(state: &AppState, user: &str) -> String {
        format!("{SESSION_COOKIE}={}", state.sessions.create(user))
    }

    async fn send(app: &Router, req: Request<Body>) -> Response {
        app.clone().oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn login_without_mfa_authenticates_in_one_step() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let app = router(state);

        let res = send(
            &app,
            Request::post("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"admin","password":"hunter2hunter2"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_of(res).await;
        assert!(body.contains("\"status\":\"authenticated\""), "{body}");
    }

    #[tokio::test]
    async fn wrong_password_is_unauthorized() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let app = router(state);

        let res = send(
            &app,
            Request::post("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"nope"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn viewer_is_denied_writes_and_admin_areas_but_can_read() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        state
            .users
            .add_user("bob", "viewerpass12", Role::Viewer, false)
            .unwrap();
        let cookie = cookie_for(&state, "bob");
        let app = router(state);

        // A viewer can read stats.
        let res = send(
            &app,
            Request::get("/api/stats")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);

        // But not change settings…
        let res = send(
            &app,
            Request::put("/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(
                    serde_json::to_string(&Settings::default()).unwrap(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // …nor add a block entry (an editor's job)…
        let res = send(
            &app,
            Request::post("/api/lists/block")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"domain":"example.com"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // …nor see the user list (an admin area).
        let res = send(
            &app,
            Request::get("/api/users")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn editor_can_write_content_but_not_settings_or_users() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        state
            .users
            .add_user("ed", "editorpass12", Role::Editor, false)
            .unwrap();
        let cookie = cookie_for(&state, "ed");
        let app = router(state);

        let res = send(
            &app,
            Request::post("/api/lists/block")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"domain":"example.com"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);

        let res = send(
            &app,
            Request::put("/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(
                    serde_json::to_string(&Settings::default()).unwrap(),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn services_catalogue_lists_every_service_with_its_global_state() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let cookie = cookie_for(&state, "admin");
        let app = router(state);

        let body = body_of(
            send(
                &app,
                Request::get("/api/services")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await,
        )
        .await;
        let rows: serde_json::Value = serde_json::from_str(&body).unwrap();
        let rows = rows.as_array().expect("a JSON array of services");
        assert_eq!(rows.len(), justdns_filter::services().len());
        let tiktok = rows
            .iter()
            .find(|row| row["id"] == "tiktok")
            .expect("tiktok in the catalogue");
        assert_eq!(tiktok["name"], "TikTok");
        assert_eq!(tiktok["category"], "social");
        assert_eq!(tiktok["globalOn"], false, "off by default");
    }

    #[tokio::test]
    async fn admin_can_block_a_service_globally_and_it_is_audited() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let cookie = cookie_for(&state, "admin");
        let audit = state.audit.clone();
        let app = router(state);

        // The PUT returns the fresh catalogue with the service now on.
        let body = body_of(
            send(
                &app,
                Request::put("/api/services/tiktok")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(r#"{"on":true}"#))
                    .unwrap(),
            )
            .await,
        )
        .await;
        let rows: serde_json::Value = serde_json::from_str(&body).unwrap();
        let tiktok = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "tiktok")
            .unwrap()
            .clone();
        assert_eq!(tiktok["globalOn"], true);

        // And it sticks: a follow-up GET still reports it on.
        let body = body_of(
            send(
                &app,
                Request::get("/api/services")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await,
        )
        .await;
        let rows: serde_json::Value = serde_json::from_str(&body).unwrap();
        let on = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "tiktok")
            .unwrap()["globalOn"]
            .clone();
        assert_eq!(on, true, "the global default persisted");

        assert!(
            audit
                .recent(10)
                .iter()
                .any(|entry| entry.action == "services.update"
                    && entry.target.as_deref() == Some("tiktok")),
            "the toggle was written to the audit log"
        );
    }

    #[tokio::test]
    async fn toggling_an_unknown_service_is_not_found() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let cookie = cookie_for(&state, "admin");
        let app = router(state);

        let res = send(
            &app,
            Request::put("/api/services/not-a-real-service")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"on":true}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_editor_cannot_change_the_global_blocked_services() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        state
            .users
            .add_user("ed", "editorpass12", Role::Editor, false)
            .unwrap();
        let cookie = cookie_for(&state, "ed");
        let app = router(state);

        // Reading the catalogue is fine for any signed-in role.
        let res = send(
            &app,
            Request::get("/api/services")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);

        // Setting a global default is Admin-only.
        let res = send(
            &app,
            Request::put("/api/services/tiktok")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"on":true}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn update_endpoints_are_admin_only() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        state
            .users
            .add_user("ed", "editorpass12", Role::Editor, false)
            .unwrap();
        let app = router(state.clone());

        // An editor may not even read the update status.
        let res = send(
            &app,
            Request::get("/api/update")
                .header("cookie", cookie_for(&state, "ed"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // An admin can, and gets the running version back.
        let res = send(
            &app,
            Request::get("/api/update")
                .header("cookie", cookie_for(&state, "admin"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(body_of(res).await.contains("currentVersion"));
    }

    #[tokio::test]
    async fn cert_endpoints_are_admin_only() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        state
            .users
            .add_user("ed", "editorpass12", Role::Editor, false)
            .unwrap();
        let app = router(state.clone());

        // An editor may not even read the certificate catalogue.
        let res = send(
            &app,
            Request::get("/api/certs")
                .header("cookie", cookie_for(&state, "ed"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // An admin passes the gate; with no store prepared in tests it reports
        // the feature unavailable rather than failing.
        let res = send(
            &app,
            Request::get("/api/certs")
                .header("cookie", cookie_for(&state, "admin"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn starting_an_update_rejects_a_backup_path_outside_the_data_volume() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let cookie = cookie_for(&state, "admin");
        let app = router(state);

        let res = send(
            &app,
            Request::post("/api/update")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(
                    r#"{"backupPath":"/etc/justdns-backup.json","password":"update-pass-123"}"#,
                ))
                .unwrap(),
        )
        .await;
        // No update is available in a test (no network), so this is refused before
        // the path is ever reached — either way it must never be accepted.
        assert_ne!(res.status(), StatusCode::NO_CONTENT);
        assert_ne!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_can_manage_users_and_actions_are_audited() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        let cookie = cookie_for(&state, "admin");
        let audit = state.audit.clone();
        let app = router(state);

        let res = send(
            &app,
            Request::post("/api/users")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(
                    r#"{"username":"kid","password":"kidpass1234","role":"viewer","mfaRequired":true}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let body = body_of(
            send(
                &app,
                Request::get("/api/users")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await,
        )
        .await;
        assert!(body.contains("\"username\":\"kid\""), "{body}");
        assert!(body.contains("\"role\":\"viewer\""), "{body}");
        // No password hash ever leaves the box.
        assert!(!body.contains("argon2"), "{body}");

        // The creation was recorded.
        let recent = audit.recent(10);
        assert!(
            recent
                .iter()
                .any(|e| e.action == "user.create" && e.target.as_deref() == Some("kid")),
            "audit missing user.create: {recent:?}"
        );
    }

    #[tokio::test]
    async fn mfa_required_user_must_enroll_at_login() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        state.users.set_mfa_required("admin", true).unwrap();
        let app = router(state);

        let body = body_of(
            send(
                &app,
                Request::post("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"hunter2hunter2"}"#,
                    ))
                    .unwrap(),
            )
            .await,
        )
        .await;
        assert!(body.contains("\"status\":\"enroll\""), "{body}");
    }

    #[tokio::test]
    async fn two_step_login_with_a_recovery_code_promotes_the_session() {
        let (store, _writer) = Store::in_memory(100).unwrap();
        let state = test_state(store);
        state.users.create_admin("admin", "hunter2hunter2").unwrap();
        // Enrol MFA out of band, keeping the recovery codes the test will use.
        let secret = justdns_auth::mfa::generate_secret();
        let codes = justdns_auth::mfa::generate_recovery_codes();
        state.users.begin_mfa("admin", &secret, &codes).unwrap();
        state.users.confirm_mfa("admin").unwrap();
        let app = router(state);

        // Step one: password accepted, a code is still needed.
        let res = send(
            &app,
            Request::post("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"admin","password":"hunter2hunter2"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert!(body_of(res).await.contains("\"status\":\"mfa\""));

        // A guarded endpoint is still closed to the pending session.
        let res = send(
            &app,
            Request::get("/api/stats")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Step two: a recovery code finishes the login.
        let res = send(
            &app,
            Request::post("/api/auth/mfa")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(Body::from(format!(r#"{{"code":"{}"}}"#, codes[0])))
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(body_of(res).await.contains("\"status\":\"authenticated\""));

        // Now the same cookie opens the guarded API.
        let res = send(
            &app,
            Request::get("/api/stats")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
}
