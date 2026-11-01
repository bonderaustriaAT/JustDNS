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
use justdns_config::{ConfigStore, Settings};
use justdns_dns::cache::DnsCache;
use justdns_filter::Blocker;
use justdns_lists::{ListError, ListKind, ListStore, ListsView, RefreshOutcome};
use justdns_rewrite::{RewriteError, RewriteStore, RewritesView};
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
        .route("/api/activity", get(activity))
        .route("/api/queries", get(queries))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/classify", get(classify))
        .route("/api/lists", get(get_lists))
        .route("/api/lists/allow", post(add_allow).delete(remove_allow))
        .route("/api/lists/block", post(add_block).delete(remove_block))
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
        .route("/api/cache/clear", post(clear_cache))
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

/// A single domain, as the allow/block editors and the log buttons send it.
#[derive(Deserialize)]
struct DomainBody {
    domain: String,
}

/// Mutations return the fresh view so the UI can replace its state in one step.
async fn add_allow(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.add_allow(&body.domain).map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn remove_allow(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.remove_allow(&body.domain).map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn add_block(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.add_block(&body.domain).map_err(list_error)?;
    Ok(Json(lists.view()))
}

async fn remove_block(
    State(lists): State<ListStore>,
    Json(body): Json<DomainBody>,
) -> Result<Json<ListsView>, (StatusCode, String)> {
    lists.remove_block(&body.domain).map_err(list_error)?;
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
        RewriteError::InvalidName(_) | RewriteError::InvalidIp(_) | RewriteError::NoIps => {
            StatusCode::BAD_REQUEST
        }
        RewriteError::Unknown(_) => StatusCode::NOT_FOUND,
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
    ips: Vec<String>,
}

async fn set_rewrite(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<RewriteBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .set_many(&body.names, &body.ips)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
}

/// Rename a rewrite and replace its addresses, keeping its enabled state.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RewriteEditBody {
    old_name: String,
    name: String,
    ips: Vec<String>,
}

async fn edit_rewrite(
    State(rewrites): State<RewriteStore>,
    Json(body): Json<RewriteEditBody>,
) -> Result<Json<RewritesView>, (StatusCode, String)> {
    rewrites
        .edit(&body.old_name, &body.name, &body.ips)
        .map_err(rewrite_error)?;
    Ok(Json(rewrites.view()))
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
    match config.set(settings) {
        Ok(()) => Ok(Json(config.get())),
        Err(error) => Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
    }
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

async fn activity(State(store): State<Store>) -> Json<Vec<ActivityBucket>> {
    Json(store.activity())
}

#[derive(Deserialize)]
struct QueryParams {
    limit: Option<usize>,
    /// Optional case-insensitive filter on the queried name or client address.
    q: Option<String>,
}

async fn queries(
    State(store): State<Store>,
    Query(params): Query<QueryParams>,
) -> Json<Vec<QueryEvent>> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .min(MAX_QUERY_LIMIT);
    let events = match params.q.as_deref().map(str::trim) {
        Some(needle) if !needle.is_empty() => store.search(needle, limit),
        _ => store.recent(limit),
    };
    Json(events)
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

    fn test_state(store: Store) -> AppState {
        let blocker = test_blocker();
        AppState {
            store,
            config: test_config(),
            lists: test_lists(blocker.clone()),
            rewrites: test_rewrites(),
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
