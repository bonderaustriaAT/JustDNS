// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Chooses which upstream answers a query and runs the configured strategy.
//!
//! The strategies here are JustDNS's own, deliberately kept separate from the
//! single-upstream transport in [`crate::client`]: that layer knows how to
//! speak to one resolver, this layer knows how to pick between several. The
//! selection is written against the [`Resolve`] trait so it can be exercised
//! without a network.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use hickory_proto::ProtoError;
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::xfer::DnsResponse;

use crate::client::UpstreamClient;
use crate::upstream::{UpstreamConfig, UpstreamMode};

/// Anything that can answer a single query. Implemented by
/// [`UpstreamClient`] for real traffic and by test doubles.
pub trait Resolve {
    fn resolve(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> impl Future<Output = Result<DnsResponse, ProtoError>> + Send;
}

impl Resolve for UpstreamClient {
    async fn resolve(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> Result<DnsResponse, ProtoError> {
        self.query(name, record_type).await
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
    /// Build a router whose resolvers are real upstream clients.
    pub fn from_config(config: &UpstreamConfig) -> Self {
        let pool = config.upstreams.iter().map(UpstreamClient::new).collect();
        let routes = config
            .rules
            .iter()
            .map(|rule| Route {
                suffix: rule.suffix.clone(),
                resolver: UpstreamClient::new(&rule.upstream),
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

impl<R: Resolve + Sync> Router<R> {
    pub async fn resolve(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> Result<DnsResponse, ProtoError> {
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

    /// Query every resolver at once and take the first success; the last error
    /// stands in once every resolver has failed.
    async fn race(
        &self,
        pool: &[R],
        name: Name,
        record_type: RecordType,
    ) -> Result<DnsResponse, ProtoError> {
        if pool.is_empty() {
            return Err(ProtoError::from("no upstreams configured"));
        }

        let futures = pool.iter().map(|resolver| {
            Box::pin(resolver.resolve(name.clone(), record_type))
                as Pin<Box<dyn Future<Output = Result<DnsResponse, ProtoError>> + Send + '_>>
        });

        futures_util::future::select_ok(futures)
            .await
            .map(|(response, _)| response)
    }

    /// Query resolvers in order, moving on only when one fails.
    async fn sequential(
        &self,
        pool: &[R],
        name: Name,
        record_type: RecordType,
    ) -> Result<DnsResponse, ProtoError> {
        let mut last_error = ProtoError::from("no upstreams configured");
        for resolver in pool {
            match resolver.resolve(name.clone(), record_type).await {
                Ok(response) => return Ok(response),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    /// Pick the next resolver round-robin so load spreads across the pool.
    async fn balanced(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> Result<DnsResponse, ProtoError> {
        if self.pool.is_empty() {
            return Err(ProtoError::from("no upstreams configured"));
        }

        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.pool.len();
        self.pool[index].resolve(name, record_type).await
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
        ) -> Result<DnsResponse, ProtoError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            if self.fail {
                return Err(ProtoError::from("mock failure"));
            }

            let mut message = Message::new();
            message.set_message_type(MessageType::Response);
            message.set_response_code(ResponseCode::NoError);
            message.add_answer(Record::from_rdata(name, 60, RData::A(A(self.marker))));
            DnsResponse::from_message(message)
        }
    }

    fn marker_of(response: &DnsResponse) -> Ipv4Addr {
        response.answers()[0].data().as_a().unwrap().0
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
}
