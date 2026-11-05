// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! HTTP API, WebSocket streams, authentication and the embedded web UI.
//!
//! The JSON API under `/api` reads the query log's hot view so the dashboard
//! can show live figures. Everything else falls through to the embedded
//! single-page UI: its assets are embedded into the binary in release builds;
//! in debug builds `rust-embed` reads them from `ui/dist` at runtime, so a
//! debug build compiles even before the UI is built.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{FromRef, Path, Query, Request, State};
use axum::http::{StatusCode, Uri, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post, put};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use justdns_auth::{SessionStore, UserStore};
use justdns_clients::{
    ClientError, ClientResolver, ClientStore, Device, DeviceKind, Group, PolicyDelta, parse_target,
};
use justdns_config::{ConfigStore, Settings};
use justdns_dns::cache::DnsCache;
use justdns_filter::Blocker;
use justdns_lists::{ListError, ListKind, ListStore, ListsView, RefreshOutcome, UserLists};
use justdns_rewrite::{Alias, Rewrite, RewriteError, RewriteStore, RewritesView};
use justdns_store::{ActivityBucket, QueryEvent, Store};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::info;

/// Name of the cookie that carries the session token. HttpOnly and
/// SameSite=Strict; not marked Secure because the dashboard is served over
/// plain HTTP on the LAN, where a Secure cookie would never be sent back.
const SESSION_COOKIE: &str = "justdns_session";

/// Default number of recent queries returned by `/api/queries`.
const DEFAULT_QUERY_LIMIT: usize = 50;
/// Upper bound on how many recent queries a caller may request at once.
const MAX_QUERY_LIMIT: usize = 500;
/// How many recent queries to scan when counting the active clients.
const ACTIVE_CLIENT_WINDOW: usize = 1000;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

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
    cache: DnsCache,
    users: UserStore,
    sessions: SessionStore,
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
    cache: DnsCache,
    users: UserStore,
    sessions: SessionStore,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "serving web UI and API");
    axum::serve(
        listener,
        router(AppState {
            store,
            config,
            blocker,
            lists,
            rewrites,
            clients,
            cache,
            users,
            sessions,
        }),
    )
    .await
}

fn router(state: AppState) -> Router {
    // The data API sits behind a login guard. The auth endpoints and the static
    // UI (so the login screen itself can load) stay public.
    let protected = Router::new()
        .route("/api/stats", get(stats))
        .route("/api/stats/day", get(stats_day))
        .route("/api/activity", get(activity))
        .route("/api/queries", get(queries))
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
        .route("/api/cache/clear", post(clear_cache))
        .route("/api/backup", get(export_backup))
        .route("/api/backup/restore", post(restore_backup))
        .layer(from_fn_with_state(state.clone(), require_auth));

    let public = Router::new()
        .route("/api/auth/state", get(auth_state))
        .route("/api/auth/setup", post(auth_setup))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .fallback(static_handler);

    protected.merge(public).with_state(state)
}

/// Guard the data API: let a request through only when its session cookie names
/// a live session, otherwise answer 401 so the UI redirects to the login page.
async fn require_auth(
    State(sessions): State<SessionStore>,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    match jar.get(SESSION_COOKIE) {
        Some(cookie) if sessions.username_for(cookie.value()).is_some() => next.run(request).await,
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
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
}

async fn auth_state(
    State(users): State<UserStore>,
    State(sessions): State<SessionStore>,
    jar: CookieJar,
) -> Json<AuthState> {
    let username = jar
        .get(SESSION_COOKIE)
        .and_then(|cookie| sessions.username_for(cookie.value()));
    Json(AuthState {
        configured: users.is_configured(),
        authenticated: username.is_some(),
        username,
    })
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

/// What a successful login or setup returns.
#[derive(Serialize)]
struct SessionResponse {
    username: String,
}

/// Create the first administrator on an unconfigured box and log them straight
/// in. Refused once an admin exists, so this cannot be used to take over.
async fn auth_setup(
    State(users): State<UserStore>,
    State(sessions): State<SessionStore>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<SessionResponse>), (StatusCode, String)> {
    use justdns_auth::AuthError;
    match users.create_admin(&creds.username, &creds.password) {
        Ok(()) => {
            let token = sessions.create(&creds.username);
            Ok((
                jar.add(session_cookie(token)),
                Json(SessionResponse {
                    username: creds.username,
                }),
            ))
        }
        Err(AuthError::AlreadyConfigured) => {
            Err((StatusCode::CONFLICT, "already configured".into()))
        }
        Err(AuthError::Invalid(message)) => Err((StatusCode::BAD_REQUEST, message)),
        Err(error) => Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
}

/// Check a username and password; on success start a session and set the cookie.
async fn auth_login(
    State(users): State<UserStore>,
    State(sessions): State<SessionStore>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<SessionResponse>), StatusCode> {
    match users.verify(&creds.username, &creds.password) {
        Some(username) => {
            let token = sessions.create(&username);
            Ok((
                jar.add(session_cookie(token)),
                Json(SessionResponse { username }),
            ))
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

/// End the current session and clear the cookie. Always succeeds, even without a
/// valid session, so a stale cookie can always be cleared.
async fn auth_logout(
    State(sessions): State<SessionStore>,
    jar: CookieJar,
) -> (CookieJar, StatusCode) {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        sessions.remove(cookie.value());
    }
    (
        jar.remove(Cookie::from(SESSION_COOKIE)),
        StatusCode::NO_CONTENT,
    )
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

/// Everything the devices view needs: the rows (named devices plus recently seen
/// unnamed clients), the group definitions, the list catalogue the policy editor
/// draws from, and the active-window the UI shows and the rows were aged by.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientsView {
    clients: Vec<ClientRow>,
    groups: Vec<Group>,
    lists: Vec<ListObjectRow>,
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
}

/// Export the whole UI-set configuration as one downloadable JSON file. The
/// `Content-Disposition` header makes the browser save it; the dashboard also
/// names the download itself.
async fn export_backup(
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    State(rewrites): State<RewriteStore>,
    State(clients): State<ClientStore>,
) -> Result<Response, (StatusCode, String)> {
    let backup = Backup {
        app: BACKUP_APP.to_owned(),
        kind: BACKUP_KIND.to_owned(),
        version: BACKUP_VERSION,
        created_at_ms: Some(now_ms()),
        settings: config.get(),
        lists: lists.snapshot(),
        rewrites: rewrites.snapshot(),
        aliases: rewrites.alias_snapshot(),
        clients: clients.snapshot(),
        groups: clients.snapshot_groups(),
    };
    let json = serde_json::to_vec_pretty(&backup)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"justdns-backup.json\"".to_owned(),
            ),
        ],
        json,
    )
        .into_response())
}

/// Restore a backup produced by [`export_backup`]. The envelope is checked
/// first, so a foreign or future-format file is refused with 400 before
/// anything is written. The rewrites are applied first because they are
/// validated as they are stored — an invalid entry rejects the restore before
/// the lists or settings are touched. Each piece takes effect live, without a
/// restart; the admin account and the query log are left untouched.
async fn restore_backup(
    State(config): State<ConfigStore>,
    State(lists): State<ListStore>,
    State(rewrites): State<RewriteStore>,
    State(clients): State<ClientStore>,
    Json(backup): Json<Backup>,
) -> Result<StatusCode, (StatusCode, String)> {
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

    rewrites
        .replace_all(backup.rewrites, backup.aliases)
        .map_err(rewrite_error)?;
    clients
        .replace(backup.clients, backup.groups)
        .map_err(client_error)?;
    lists.replace(backup.lists).map_err(list_error)?;
    config
        .set(backup.settings)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Milliseconds since the Unix epoch, for a backup's `createdAtMs` stamp.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Return the current settings so the dashboard can show them.
async fn get_settings(State(config): State<ConfigStore>) -> Json<Settings> {
    Json(config.get())
}

/// Replace the settings. The new values take effect at once (IPv6 live); a
/// failure to write them to disk is reported so the UI knows the change was
/// not saved durably.
async fn put_settings(
    State(config): State<ConfigStore>,
    Json(settings): Json<Settings>,
) -> Result<Json<Settings>, (StatusCode, String)> {
    config
        .set(settings)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(config.get()))
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
}

/// How many recent events to scan when a search includes device names: the whole
/// hot ring, so a device-name match reaches as far back as a name/address match.
const QUERY_SCAN_LIMIT: usize = 100_000;

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
        })
        .collect();
    Json(named)
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
    Some(([(header::CONTENT_TYPE, content_type.to_owned())], file.data).into_response())
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
        ConfigStore::load(path, Settings::default()).unwrap()
    }

    /// A throwaway user store backed by a unique temp file (no admin yet).
    fn test_users() -> UserStore {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "justdns-web-auth-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        UserStore::load(path).unwrap()
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

    fn test_state(store: Store) -> AppState {
        let blocker = test_blocker();
        AppState {
            store,
            config: test_config(),
            lists: test_lists(blocker.clone()),
            rewrites: test_rewrites(),
            clients: test_clients(),
            cache: DnsCache::new(100),
            blocker,
            users: test_users(),
            sessions: SessionStore::new(justdns_auth::DEFAULT_SESSION_TTL),
        }
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
        }
    }

    async fn body_of(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
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

        let backup = body_of(
            app_a
                .oneshot(
                    Request::get("/api/backup")
                        .header("cookie", &cookie_a)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(backup.contains("justdns"), "envelope marker: {backup}");
        assert!(backup.contains("tracker.test"), "lists in file: {backup}");
        assert!(backup.contains("Gaeste"), "group in file: {backup}");
        assert!(
            backup.contains("werbung.test"),
            "device delta in file: {backup}"
        );

        // Instance B: a fresh box. Restoring the backup brings everything back.
        let (store_b, _writer_b) = Store::in_memory(100).unwrap();
        let (app_b, cookie_b) = app_and_cookie(store_b);

        let res = app_b
            .clone()
            .oneshot(
                Request::post("/api/backup/restore")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie_b)
                    .body(Body::from(backup))
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
        // Well-formed JSON, right shape, but a foreign `app` marker: refused
        // before anything is written.
        let res = app
            .oneshot(
                Request::post("/api/backup/restore")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"app":"evil","kind":"backup","version":1,"settings":{},"lists":{},"rewrites":[]}"#,
                    ))
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
}
