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

use axum::Router;
use axum::extract::{FromRef, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use justdns_config::{ConfigStore, Settings};
use justdns_store::{ActivityBucket, QueryEvent, Store};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::info;

/// Default number of recent queries returned by `/api/queries`.
const DEFAULT_QUERY_LIMIT: usize = 50;
/// Upper bound on how many recent queries a caller may request at once.
const MAX_QUERY_LIMIT: usize = 500;
/// How many recent queries to scan when counting the active clients.
const ACTIVE_CLIENT_WINDOW: usize = 1000;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

/// What the API handlers share: the query log and the editable configuration.
/// Each handler extracts just the piece it needs via [`FromRef`].
#[derive(Clone)]
struct AppState {
    store: Store,
    config: ConfigStore,
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

/// Bind `addr` and serve the API and embedded UI until the listener fails.
pub async fn serve(addr: SocketAddr, store: Store, config: ConfigStore) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "serving web UI and API");
    axum::serve(listener, router(AppState { store, config })).await
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/stats", get(stats))
        .route("/api/activity", get(activity))
        .route("/api/queries", get(queries))
        .route("/api/settings", get(get_settings).put(put_settings))
        .fallback(static_handler)
        .with_state(state)
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

/// Totals for the dashboard. `blocked` is always zero until a filter exists;
/// the field is present so the UI need not change when blocking lands.
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
        blocked: 0,
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

    fn app(store: Store) -> Router {
        router(AppState {
            store,
            config: test_config(),
        })
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
        app(store)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
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
        // config handle.
        let app = router(AppState {
            store,
            config: test_config(),
        });

        let body = body_of(
            app.clone()
                .oneshot(Request::get("/api/settings").body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        assert!(body.contains("\"ipv6\":false"), "defaults: {body}");

        let put = Request::put("/api/settings")
            .header("content-type", "application/json")
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
            app.oneshot(Request::get("/api/settings").body(Body::empty()).unwrap())
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

    #[test]
    fn client_routes_have_no_extension() {
        assert!(!last_segment_has_extension("einstellungen"));
        assert!(!last_segment_has_extension("foo/bar"));
        assert!(last_segment_has_extension("assets/index-abc.js"));
        assert!(last_segment_has_extension("favicon.svg"));
    }
}
