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
use axum::extract::{Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
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

/// Bind `addr` and serve the API and embedded UI until the listener fails.
pub async fn serve(addr: SocketAddr, store: Store) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "serving web UI and API");
    axum::serve(listener, router(store)).await
}

fn router(store: Store) -> Router {
    Router::new()
        .route("/api/stats", get(stats))
        .route("/api/activity", get(activity))
        .route("/api/queries", get(queries))
        .fallback(static_handler)
        .with_state(store)
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
}

async fn queries(
    State(store): State<Store>,
    Query(params): Query<QueryParams>,
) -> Json<Vec<QueryEvent>> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_QUERY_LIMIT)
        .min(MAX_QUERY_LIMIT);
    Json(store.recent(limit))
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

    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use justdns_store::QuerySource;
    use tower::ServiceExt;

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
        router(store)
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

    #[test]
    fn client_routes_have_no_extension() {
        assert!(!last_segment_has_extension("einstellungen"));
        assert!(!last_segment_has_extension("foo/bar"));
        assert!(last_segment_has_extension("assets/index-abc.js"));
        assert!(last_segment_has_extension("favicon.svg"));
    }
}
