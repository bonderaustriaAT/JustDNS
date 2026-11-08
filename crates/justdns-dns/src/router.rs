// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Chooses which upstream answers a query and runs the configured strategy.
//!
//! Where [`crate::client`] speaks to one resolver, this layer picks between
//! several. The selection is written against the [`Resolve`] trait so it can be
//! exercised without a network.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use arc_swap::ArcSwap;
use hickory_proto::ProtoError;
use hickory_proto::op::DnsResponse;
use hickory_proto::rr::{Name, RecordType};

use crate::client::UpstreamClient;
use crate::observer::DnssecStatus;
use crate::upstream::{UpstreamConfig, UpstreamMode};

/// A resolved answer together with which resolver produced it and its DNSSEC
/// verdict.
///
/// The [`Resolve`] layer reports this so the query log can attribute an upstream
/// answer to the resolver that gave it — the basis of the per-resolver
/// response-time statistics. `resolver` is a short, human-readable label (the
/// DoT/DoH certificate name, or the plain IP) and is `None` when the answer did
/// not come from a tracked upstream (a test double, say). `dnssec` is the
/// validation verdict the upstream client derived (from the AD flag, with a
/// checking-disabled retry to confirm Bogus); it is `None` when validation is
/// off or the answer did not come from a validating lookup.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub response: DnsResponse,
    pub resolver: Option<Arc<str>>,
    pub dnssec: Option<DnssecStatus>,
}

impl Resolved {
    /// An answer attributed to `resolver` (or to nothing, with `None`), carrying
    /// the DNSSEC `verdict`.
    pub fn new(
        response: DnsResponse,
        resolver: Option<Arc<str>>,
        dnssec: Option<DnssecStatus>,
    ) -> Self {
        Self {
            response,
            resolver,
            dnssec,
        }
    }

    /// An answer whose resolver is not tracked and which carries no verdict.
    pub fn anonymous(response: DnsResponse) -> Self {
        Self {
            response,
            resolver: None,
            dnssec: None,
        }
    }
}

/// Anything that can answer a single query. Implemented by
/// [`UpstreamClient`] for real traffic and by test doubles. The answer carries
/// the resolver that produced it (see [`Resolved`]).
pub trait Resolve {
    fn resolve(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> impl Future<Output = Result<Resolved, ProtoError>> + Send;
}

impl Resolve for UpstreamClient {
    async fn resolve(&self, name: Name, record_type: RecordType) -> Result<Resolved, ProtoError> {
        let (response, dnssec) = self.query(name, record_type).await?;
        Ok(Resolved::new(response, Some(self.label()), dnssec))
    }
}

/// A rule routing a domain (and its subdomains) to a specific resolver.
struct Route<R> {
    suffix: Name,
    resolver: R,
}

/// Dispatches queries to a pool of resolvers using the configured
/// [`UpstreamMode`].
pub struct Router<R> {
    mode: UpstreamMode,
    pool: Vec<R>,
    routes: Vec<Route<R>>,
    next: AtomicUsize,
}

impl<R> Router<R> {
    pub fn new(mode: UpstreamMode, pool: Vec<R>) -> Self {
        Self {
            mode,
            pool,
            routes: Vec::new(),
            next: AtomicUsize::new(0),
        }
    }

    /// Add a rule routing `suffix` and its subdomains to `resolver`. Only
    /// consulted in [`UpstreamMode::RuleBased`].
    pub fn with_route(mut self, suffix: Name, resolver: R) -> Self {
        self.routes.push(Route { suffix, resolver });
        self
    }

    /// The resolver routing `name`, longest matching suffix first.
    fn route_for(&self, name: &Name) -> Option<&R> {
        self.routes
            .iter()
            .filter(|route| route.suffix.zone_of(name))
            .max_by_key(|route| route.suffix.num_labels())
            .map(|route| &route.resolver)
    }
}

impl Router<UpstreamClient> {
    /// Build a router whose resolvers are real upstream clients. `validate` is
    /// the shared DNSSEC switch handed to every client, so flipping it toggles
    /// validation across the whole pool at once; the same handle survives a
    /// router rebuild, so an upstream change does not reset the setting.
    pub fn from_config(config: &UpstreamConfig, validate: Arc<AtomicBool>) -> Self {
        let pool = config
            .upstreams
            .iter()
            .map(|upstream| UpstreamClient::new(upstream, validate.clone()))
            .collect();
        let routes = config
            .rules
            .iter()
            .map(|rule| Route {
                suffix: rule.suffix.clone(),
                resolver: UpstreamClient::new(&rule.upstream, validate.clone()),
            })
            .collect();

        Self {
            mode: config.mode,
            pool,
            routes,
            next: AtomicUsize::new(0),
        }
    }

    /// Prime every upstream by sending one throwaway query, so its connection
    /// (for DoT/DoH, the TLS session) is established and held open before real
    /// traffic arrives. Called at startup and periodically, it turns the first
    /// real lookup after an idle stretch from a cold handshake into a warm
    /// round-trip. Errors are ignored — this is best-effort warming, not a query
    /// anyone is waiting on.
    pub async fn warm(&self) {
        let name = Name::root();
        for client in &self.pool {
            let _ = client.resolve(name.clone(), RecordType::NS).await;
        }
        for route in &self.routes {
            let _ = route.resolver.resolve(name.clone(), RecordType::NS).await;
        }
    }
}

/// A router is itself a resolver: it answers a query by running its configured
/// strategy over the pool. This lets a router be used wherever a single
/// [`Resolve`] is expected — the pipeline holds one directly, and a
/// [`SharedRouter`] wraps one for live reconfiguration.
impl<R: Resolve + Sync> Resolve for Router<R> {
    async fn resolve(&self, name: Name, record_type: RecordType) -> Result<Resolved, ProtoError> {
        match self.mode {
            UpstreamMode::Racing => self.race(&self.pool, name, record_type).await,
            UpstreamMode::Fallback => self.sequential(&self.pool, name, record_type).await,
            UpstreamMode::LoadBalanced => self.balanced(name, record_type).await,
            UpstreamMode::RuleBased => match self.route_for(&name) {
                Some(resolver) => resolver.resolve(name, record_type).await,
                None => self.race(&self.pool, name, record_type).await,
            },
        }
    }
}

impl<R: Resolve + Sync> Router<R> {
    /// Query every resolver at once and take the first success; the last error
    /// stands in once every resolver has failed.
    async fn race(
        &self,
        pool: &[R],
        name: Name,
        record_type: RecordType,
    ) -> Result<Resolved, ProtoError> {
        if pool.is_empty() {
            return Err(ProtoError::from("no upstreams configured"));
        }

        let futures = pool.iter().map(|resolver| {
            Box::pin(resolver.resolve(name.clone(), record_type))
                as Pin<Box<dyn Future<Output = Result<Resolved, ProtoError>> + Send + '_>>
        });

        futures_util::future::select_ok(futures)
            .await
            .map(|(resolved, _)| resolved)
    }

    /// Query resolvers in order, moving on only when one fails.
    async fn sequential(
        &self,
        pool: &[R],
        name: Name,
        record_type: RecordType,
    ) -> Result<Resolved, ProtoError> {
        let mut last_error = ProtoError::from("no upstreams configured");
        for resolver in pool {
            match resolver.resolve(name.clone(), record_type).await {
                Ok(resolved) => return Ok(resolved),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    /// Pick the next resolver round-robin so load spreads across the pool.
    async fn balanced(&self, name: Name, record_type: RecordType) -> Result<Resolved, ProtoError> {
        if self.pool.is_empty() {
            return Err(ProtoError::from("no upstreams configured"));
        }

        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.pool.len();
        self.pool[index].resolve(name, record_type).await
    }
}

/// A live-swappable handle to the upstream router.
///
/// The pipeline holds one of these (through `Arc`) and so always resolves
/// against the current router; the config layer swaps in a freshly built one
/// when the upstream settings change, so a profile, mode or encryption change
/// takes effect on the next query without a restart. The keepalive task holds
/// the same handle and warms whichever router is current.
#[derive(Clone)]
pub struct SharedRouter {
    inner: Arc<ArcSwap<Router<UpstreamClient>>>,
}

impl SharedRouter {
    pub fn new(router: Router<UpstreamClient>) -> Self {
        Self {
            inner: Arc::new(ArcSwap::from_pointee(router)),
        }
    }

    /// Replace the live router, e.g. after the upstream settings changed. The
    /// next query (and the next keepalive tick) sees the new one.
    pub fn store(&self, router: Router<UpstreamClient>) {
        self.inner.store(Arc::new(router));
    }

    /// The router currently in force — a cheap `Arc` clone, safe to hold across
    /// an await point (unlike an `ArcSwap` guard).
    pub fn current(&self) -> Arc<Router<UpstreamClient>> {
        self.inner.load_full()
    }
}

impl Resolve for SharedRouter {
    async fn resolve(&self, name: Name, record_type: RecordType) -> Result<Resolved, ProtoError> {
        self.current().resolve(name, record_type).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use hickory_proto::op::{Message, MessageType, ResponseCode};
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{RData, Record};

    struct Mock {
        marker: Ipv4Addr,
        fail: bool,
        delay: Option<Duration>,
        calls: Arc<AtomicUsize>,
    }

    impl Mock {
        fn ok(marker: Ipv4Addr) -> Self {
            Self {
                marker,
                fail: false,
                delay: None,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn failing() -> Self {
            Self {
                marker: Ipv4Addr::UNSPECIFIED,
                fail: true,
                delay: None,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn slow(mut self, delay: Duration) -> Self {
            self.delay = Some(delay);
            self
        }
    }

    impl Resolve for Mock {
        async fn resolve(
            &self,
            name: Name,
            _record_type: RecordType,
        ) -> Result<Resolved, ProtoError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            if self.fail {
                return Err(ProtoError::from("mock failure"));
            }

            let mut message = Message::query();
            message.metadata.message_type = MessageType::Response;
            message.metadata.response_code = ResponseCode::NoError;
            message.add_answer(Record::from_rdata(name, 60, RData::A(A(self.marker))));
            let label: Arc<str> = Arc::from(self.marker.to_string());
            Ok(Resolved::new(
                DnsResponse::from_message(message)?,
                Some(label),
                None,
            ))
        }
    }

    fn marker_of(resolved: &Resolved) -> Ipv4Addr {
        let RData::A(a) = &resolved.response.answers[0].data else {
            panic!("expected an A record");
        };
        a.0
    }

    fn query(name: &str) -> Name {
        Name::from_ascii(name).unwrap()
    }

    #[tokio::test]
    async fn racing_takes_a_success_over_a_failure() {
        let router = Router::new(
            UpstreamMode::Racing,
            vec![Mock::failing(), Mock::ok(Ipv4Addr::new(2, 2, 2, 2))],
        );

        let response = router
            .resolve(query("example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(marker_of(&response), Ipv4Addr::new(2, 2, 2, 2));
    }

    #[tokio::test]
    async fn racing_prefers_the_faster_upstream() {
        let router = Router::new(
            UpstreamMode::Racing,
            vec![
                Mock::ok(Ipv4Addr::new(1, 1, 1, 1)).slow(Duration::from_millis(50)),
                Mock::ok(Ipv4Addr::new(2, 2, 2, 2)),
            ],
        );

        let response = router
            .resolve(query("example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(marker_of(&response), Ipv4Addr::new(2, 2, 2, 2));
    }

    #[tokio::test]
    async fn fallback_stops_at_the_first_success() {
        let second = Mock::ok(Ipv4Addr::new(2, 2, 2, 2));
        let second_calls = second.calls.clone();
        let router = Router::new(
            UpstreamMode::Fallback,
            vec![Mock::ok(Ipv4Addr::new(1, 1, 1, 1)), second],
        );

        let response = router
            .resolve(query("example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(marker_of(&response), Ipv4Addr::new(1, 1, 1, 1));
        assert_eq!(second_calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn fallback_skips_a_failed_upstream() {
        let router = Router::new(
            UpstreamMode::Fallback,
            vec![Mock::failing(), Mock::ok(Ipv4Addr::new(2, 2, 2, 2))],
        );

        let response = router
            .resolve(query("example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(marker_of(&response), Ipv4Addr::new(2, 2, 2, 2));
    }

    #[tokio::test]
    async fn load_balancing_rotates_through_the_pool() {
        let router = Router::new(
            UpstreamMode::LoadBalanced,
            vec![
                Mock::ok(Ipv4Addr::new(1, 1, 1, 1)),
                Mock::ok(Ipv4Addr::new(2, 2, 2, 2)),
                Mock::ok(Ipv4Addr::new(3, 3, 3, 3)),
            ],
        );

        let mut seen = Vec::new();
        for _ in 0..3 {
            let response = router
                .resolve(query("example.com."), RecordType::A)
                .await
                .unwrap();
            seen.push(marker_of(&response));
        }

        assert_eq!(
            seen,
            vec![
                Ipv4Addr::new(1, 1, 1, 1),
                Ipv4Addr::new(2, 2, 2, 2),
                Ipv4Addr::new(3, 3, 3, 3),
            ]
        );
    }

    #[tokio::test]
    async fn rule_based_routes_matching_suffix_and_falls_back_to_the_pool() {
        let router = Router::new(
            UpstreamMode::RuleBased,
            vec![Mock::ok(Ipv4Addr::new(1, 1, 1, 1))],
        )
        .with_route(query("example.com."), Mock::ok(Ipv4Addr::new(9, 9, 9, 9)));

        let routed = router
            .resolve(query("host.example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(marker_of(&routed), Ipv4Addr::new(9, 9, 9, 9));

        let pooled = router
            .resolve(query("example.org."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(marker_of(&pooled), Ipv4Addr::new(1, 1, 1, 1));
    }

    #[tokio::test]
    async fn an_empty_pool_is_an_error() {
        let router: Router<Mock> = Router::new(UpstreamMode::Racing, Vec::new());
        assert!(
            router
                .resolve(query("example.com."), RecordType::A)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reports_which_resolver_answered() {
        // Racing: the faster upstream wins and its label rides out with the
        // answer. Each mock labels itself with its marker address.
        let racing = Router::new(
            UpstreamMode::Racing,
            vec![
                Mock::ok(Ipv4Addr::new(1, 1, 1, 1)).slow(Duration::from_millis(50)),
                Mock::ok(Ipv4Addr::new(2, 2, 2, 2)),
            ],
        );
        let resolved = racing
            .resolve(query("example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(resolved.resolver.as_deref(), Some("2.2.2.2"));

        // Rule-based: the routed resolver's label is the one reported.
        let routed = Router::new(
            UpstreamMode::RuleBased,
            vec![Mock::ok(Ipv4Addr::new(1, 1, 1, 1))],
        )
        .with_route(query("example.com."), Mock::ok(Ipv4Addr::new(9, 9, 9, 9)));
        let resolved = routed
            .resolve(query("host.example.com."), RecordType::A)
            .await
            .unwrap();
        assert_eq!(resolved.resolver.as_deref(), Some("9.9.9.9"));
    }
}
