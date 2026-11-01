// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query pipeline: cache lookup, upstream forwarding and response building.
//!
//! This is where an incoming query becomes an answer. A cache hit is served
//! straight away; a miss goes to the [`Router`], and the upstream's answer is
//! cached before it is returned. IPv6 answers are suppressed unless enabled.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, CNAME};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_proto::xfer::DnsResponse;
use justdns_filter::Blocker;
use justdns_rewrite::Rewriter;

use crate::cache::{CacheHit, CacheKey, CachedEntry, CachedResponse, DnsCache};
use crate::observer::{AnswerSource, QueryObserver, QueryRecord};
use crate::router::Resolve;
use crate::safesearch;

/// Negative answers without an SOA are cached this long (seconds).
const DEFAULT_NEGATIVE_TTL: u64 = 60;
/// Upper bound on any negative-cache TTL (seconds).
const MAX_NEGATIVE_TTL: u64 = 3600;
/// TTL on a locally rewritten answer. Short, since a LAN mapping can change and
/// the lookup is cheap enough to repeat; long enough to spare a flood of repeats.
const REWRITE_TTL: u32 = 300;

/// Options the pipeline reads while handling a query. `ipv6_enabled` is a
/// shared flag rather than a plain bool so the config store can flip it at
/// runtime and the next query sees the change without a restart.
#[derive(Clone, Debug)]
pub struct PipelineOptions {
    /// Whether to answer AAAA queries. Off by default, so IPv6 names resolve to
    /// nothing and clients fall back to IPv4.
    pub ipv6_enabled: Arc<AtomicBool>,
    /// Whether to force the big search engines to their safe-search endpoints.
    /// A shared flag so the config store can flip it live, like `ipv6_enabled`.
    pub safe_search: Arc<AtomicBool>,
}

impl PipelineOptions {
    /// Build options with a fresh IPv6 flag set to `enabled` and safe search off.
    pub fn with_ipv6(enabled: bool) -> Self {
        Self {
            ipv6_enabled: Arc::new(AtomicBool::new(enabled)),
            safe_search: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self::with_ipv6(false)
    }
}

pub struct Pipeline<R> {
    cache: DnsCache,
    /// The resolver a cache miss is forwarded to. Held as `Arc<R>` rather than a
    /// concrete router so it can be a `SharedRouter` (live-swappable) in the
    /// binary and a plain `Router` in tests.
    router: Arc<R>,
    options: PipelineOptions,
    observer: Option<Arc<dyn QueryObserver>>,
    blocker: Option<Arc<Blocker>>,
    rewriter: Option<Arc<Rewriter>>,
    /// Keys with a background refresh in flight, so however many clients hit a
    /// stale or near-expiry entry at once, only one of them forwards upstream.
    refreshing: Arc<Mutex<HashSet<CacheKey>>>,
}

impl<R: Resolve + Send + Sync + 'static> Pipeline<R> {
    pub fn new(cache: DnsCache, router: Arc<R>, options: PipelineOptions) -> Self {
        Self {
            cache,
            router,
            options,
            observer: None,
            blocker: None,
            rewriter: None,
            refreshing: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Report every handled query to `observer`, which feeds the query log.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn QueryObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Filter queries through `blocker`: a blocked name is answered with
    /// NXDOMAIN before any cache or upstream lookup. Without one, nothing is
    /// blocked.
    #[must_use]
    pub fn with_blocker(mut self, blocker: Arc<Blocker>) -> Self {
        self.blocker = Some(blocker);
        self
    }

    /// Answer names with a local rewrite from `rewriter` instead of forwarding
    /// them. Without one, nothing is rewritten.
    #[must_use]
    pub fn with_rewriter(mut self, rewriter: Arc<Rewriter>) -> Self {
        self.rewriter = Some(rewriter);
        self
    }

    /// Turn a request message into a response, recording how it was answered.
    pub async fn handle(&self, request: &Message, client: IpAddr) -> Message {
        let start = Instant::now();

        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(request.op_code());
        response.set_recursion_desired(request.recursion_desired());
        response.set_recursion_available(true);

        let Some(query) = request.queries().first().cloned() else {
            // No question to answer, so nothing meaningful to record either.
            response.set_response_code(ResponseCode::FormErr);
            return response;
        };
        response.add_query(query.clone());

        let source = self.answer(&query, &mut response).await;

        if let Some(observer) = &self.observer {
            observer.observe(QueryRecord {
                client,
                name: query.name().to_string(),
                record_type: u16::from(query.query_type()),
                response_code: u16::from(response.response_code()),
                source,
                duration: start.elapsed(),
                answers: u16::try_from(response.answers().len()).unwrap_or(u16::MAX),
                addresses: answer_addresses(&response),
            });
        }

        response
    }

    /// Fill `response` with the answer to `query` and report where it came from.
    async fn answer(&self, query: &Query, response: &mut Message) -> AnswerSource {
        // A local rewrite is the strongest, most explicit intent the user can
        // express, so it is answered first — before the block check and any
        // lookup — and owns the name for every record type.
        if let Some(rewriter) = &self.rewriter {
            if let Some(ips) = rewriter.lookup(&query.name().to_string()) {
                self.answer_rewrite(query, response, &ips);
                return AnswerSource::Rewrite;
            }
        }

        // Blocked names are refused with NXDOMAIN before any lookup, for every
        // record type, so a blocked host resolves to nothing on IPv4 and IPv6.
        if let Some(blocker) = &self.blocker {
            if blocker.blocks(&query.name().to_string()) {
                response.set_response_code(ResponseCode::NXDomain);
                return AnswerSource::Blocked;
            }
        }

        // IPv6 off: answer AAAA with NODATA without touching cache or upstream.
        if query.query_type() == RecordType::AAAA
            && !self.options.ipv6_enabled.load(Ordering::Relaxed)
        {
            response.set_response_code(ResponseCode::NoError);
            return AnswerSource::Local;
        }

        // Safe search: force the big search engines to their filtered endpoint
        // by resolving that target and returning it under a CNAME. Checked before
        // the cache so a previously cached unfiltered answer is never served, and
        // never cached itself so flipping the flag off takes effect at once.
        if self.options.safe_search.load(Ordering::Relaxed) {
            if let Some(target) = safesearch::target(query.name(), query.query_type()) {
                return self.answer_safe_search(query, response, target).await;
            }
        }

        let key = CacheKey::new(query.name(), query.query_type());

        if let Some(hit) = self.cache.get(&key).await {
            let CacheHit {
                response: cached,
                needs_refresh,
            } = hit;
            apply_cached(response, cached);
            // The answer is already served; a near-expiry or stale entry is
            // renewed behind it so the next client keeps hitting a warm cache.
            if needs_refresh {
                self.spawn_refresh(key, query.name().clone(), query.query_type());
            }
            return AnswerSource::Cache;
        }

        match self
            .router
            .resolve(query.name().clone(), query.query_type())
            .await
        {
            Ok(upstream) => {
                response.set_response_code(upstream.response_code());
                for record in upstream.answers() {
                    response.add_answer(record.clone());
                }
                self.cache.insert(key, entry_from_response(&upstream)).await;
            }
            Err(_) => {
                response.set_response_code(ResponseCode::ServFail);
            }
        }

        AnswerSource::Upstream
    }

    /// Refresh `key` from upstream in the background and store the fresh answer,
    /// so the serving path never waits on it. At most one refresh per key runs at
    /// a time; concurrent triggers for the same name are dropped.
    fn spawn_refresh(&self, key: CacheKey, name: Name, record_type: RecordType) {
        if !self
            .refreshing
            .lock()
            .expect("refresh set is not poisoned")
            .insert(key.clone())
        {
            // A refresh for this name is already in flight.
            return;
        }

        let cache = self.cache.clone();
        let router = self.router.clone();
        let refreshing = self.refreshing.clone();
        tokio::spawn(async move {
            if let Ok(upstream) = router.resolve(name, record_type).await {
                cache
                    .insert(key.clone(), entry_from_response(&upstream))
                    .await;
            }
            refreshing
                .lock()
                .expect("refresh set is not poisoned")
                .remove(&key);
        });
    }

    /// Answer `query` from a rewrite's `ips`. An A query gets the IPv4 addresses
    /// and an AAAA query the IPv6 ones (the latter only when IPv6 is enabled);
    /// any other type, or a family with no address, is NODATA (NOERROR with no
    /// answers). Either way the name is owned locally and never forwarded.
    fn answer_rewrite(&self, query: &Query, response: &mut Message, ips: &[IpAddr]) {
        response.set_response_code(ResponseCode::NoError);
        let name = query.name().clone();
        match query.query_type() {
            RecordType::A => {
                for ip in ips {
                    if let IpAddr::V4(v4) = ip {
                        response.add_answer(Record::from_rdata(
                            name.clone(),
                            REWRITE_TTL,
                            RData::A(A(*v4)),
                        ));
                    }
                }
            }
            RecordType::AAAA if self.options.ipv6_enabled.load(Ordering::Relaxed) => {
                for ip in ips {
                    if let IpAddr::V6(v6) = ip {
                        response.add_answer(Record::from_rdata(
                            name.clone(),
                            REWRITE_TTL,
                            RData::AAAA(AAAA(*v6)),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    /// Answer `query` from its safe-search `target`: resolve the target for the
    /// same record type upstream, then return a CNAME from the queried name to
    /// the target followed by the target's address records. Not cached, so the
    /// mapping disappears the instant safe search is switched off.
    async fn answer_safe_search(
        &self,
        query: &Query,
        response: &mut Message,
        target: Name,
    ) -> AnswerSource {
        match self
            .router
            .resolve(target.clone(), query.query_type())
            .await
        {
            Ok(upstream) => {
                response.set_response_code(upstream.response_code());
                response.add_answer(Record::from_rdata(
                    query.name().clone(),
                    REWRITE_TTL,
                    RData::CNAME(CNAME(target)),
                ));
                for record in upstream.answers() {
                    response.add_answer(record.clone());
                }
            }
            Err(_) => {
                response.set_response_code(ResponseCode::ServFail);
            }
        }
        AnswerSource::Upstream
    }
}

/// The IPv4/IPv6 addresses in a response's answer section, in order. Non-address
/// records (CNAME, SOA, …) are skipped, so a CNAME chain reports only the final
/// addresses. Used to record what a query actually resolved to in the query log.
fn answer_addresses(response: &Message) -> Vec<IpAddr> {
    response
        .answers()
        .iter()
        .filter_map(|record| match record.data() {
            RData::A(a) => Some(IpAddr::V4(a.0)),
            RData::AAAA(aaaa) => Some(IpAddr::V6(aaaa.0)),
            _ => None,
        })
        .collect()
}

/// Turn an upstream response into the cache entry to store for it: a positive
/// entry at the smallest record TTL, or a negative entry at the negative TTL.
/// Shared by the synchronous miss path and the background refresh.
fn entry_from_response(upstream: &DnsResponse) -> CachedEntry {
    let answers = upstream.answers();
    if answers.is_empty() {
        CachedEntry::negative(
            upstream.response_code(),
            Duration::from_secs(negative_ttl(upstream)),
        )
    } else {
        let ttl = answers.iter().map(|record| record.ttl()).min().unwrap_or(0);
        CachedEntry::positive(answers.to_vec(), Duration::from_secs(u64::from(ttl)))
    }
}

fn apply_cached(response: &mut Message, hit: CachedResponse) {
    match hit {
        CachedResponse::Positive(records) => {
            response.set_response_code(ResponseCode::NoError);
            for record in records {
                response.add_answer(record);
            }
        }
        CachedResponse::Negative(code) => {
            response.set_response_code(code);
        }
    }
}

/// The TTL to cache a negative answer for: the SOA minimum from the authority
/// section when present, otherwise a short default, and never above the cap.
fn negative_ttl(message: &Message) -> u64 {
    message
        .name_servers()
        .iter()
        .find_map(|record| record.data().as_soa().map(|soa| u64::from(soa.minimum())))
        .unwrap_or(DEFAULT_NEGATIVE_TTL)
        .min(MAX_NEGATIVE_TTL)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::observer::{AnswerSource, QueryObserver, QueryRecord};
    use crate::router::Router;

    const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));

    #[derive(Default)]
    struct Recording {
        seen: Mutex<Vec<QueryRecord>>,
    }

    impl QueryObserver for Recording {
        fn observe(&self, record: QueryRecord) {
            self.seen.lock().unwrap().push(record);
        }
    }

    use hickory_proto::ProtoError;
    use hickory_proto::op::Query;
    use hickory_proto::rr::rdata::{A, AAAA, SOA};
    use hickory_proto::rr::{Name, RData, Record};
    use hickory_proto::xfer::DnsResponse;

    use crate::upstream::UpstreamMode;

    enum Reply {
        A(Ipv4Addr),
        Aaaa(Ipv6Addr),
        Empty(ResponseCode),
        Fail,
    }

    struct Mock {
        reply: Reply,
        ttl: u32,
        calls: Arc<AtomicUsize>,
    }

    impl Mock {
        fn new(reply: Reply) -> Self {
            Self {
                reply,
                ttl: 120,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        /// Answer with records carrying `ttl` seconds instead of the default, so
        /// a test can make an answer expire (or go stale) on demand.
        fn with_ttl(mut self, ttl: u32) -> Self {
            self.ttl = ttl;
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
            let mut message = Message::new();
            message.set_message_type(MessageType::Response);
            match &self.reply {
                Reply::A(ip) => {
                    message.set_response_code(ResponseCode::NoError);
                    message.add_answer(Record::from_rdata(name, self.ttl, RData::A(A(*ip))));
                }
                Reply::Aaaa(ip) => {
                    message.set_response_code(ResponseCode::NoError);
                    message.add_answer(Record::from_rdata(name, self.ttl, RData::AAAA(AAAA(*ip))));
                }
                Reply::Empty(code) => {
                    message.set_response_code(*code);
                }
                Reply::Fail => return Err(ProtoError::from("mock failure")),
            }
            DnsResponse::from_message(message)
        }
    }

    fn pipeline(
        reply: Reply,
        options: PipelineOptions,
    ) -> (Pipeline<Router<Mock>>, Arc<AtomicUsize>) {
        let mock = Mock::new(reply);
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        (Pipeline::new(DnsCache::new(100), router, options), calls)
    }

    fn request(name: &str, record_type: RecordType) -> Message {
        let mut message = Message::new();
        message.set_id(4242);
        message.set_recursion_desired(true);
        message.add_query(Query::query(Name::from_ascii(name).unwrap(), record_type));
        message
    }

    #[tokio::test]
    async fn miss_forwards_and_caches() {
        let (pipe, calls) = pipeline(
            Reply::A(Ipv4Addr::new(93, 184, 216, 34)),
            PipelineOptions::default(),
        );
        let req = request("example.com.", RecordType::A);

        let first = pipe.handle(&req, CLIENT).await;
        assert_eq!(first.id(), 4242);
        assert_eq!(first.response_code(), ResponseCode::NoError);
        assert_eq!(
            first.answers()[0].data().as_a(),
            Some(&A(Ipv4Addr::new(93, 184, 216, 34)))
        );

        let second = pipe.handle(&req, CLIENT).await;
        assert_eq!(second.answers().len(), 1);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "second query should hit the cache"
        );
    }

    #[tokio::test]
    async fn stale_hit_is_served_and_refreshed_in_background() {
        // A zero-TTL answer is stale the moment it is cached, so the second query
        // serves it from the stale copy and kicks off exactly one background
        // refresh instead of making the client wait on upstream.
        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34))).with_ttl(0);
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default());
        let req = request("example.com.", RecordType::A);

        // First query misses and forwards (call 1).
        pipe.handle(&req, CLIENT).await;
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        // Second query is answered straight from the stale entry, and a refresh
        // is spawned behind it.
        let stale = pipe.handle(&req, CLIENT).await;
        assert_eq!(stale.answers().len(), 1, "stale answer is still served");

        // The background refresh reaches upstream once more (call 2).
        for _ in 0..200 {
            if calls.load(Ordering::Relaxed) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "a stale hit refreshes exactly once in the background"
        );
    }

    #[tokio::test]
    async fn upstream_failure_is_servfail() {
        let (pipe, _calls) = pipeline(Reply::Fail, PipelineOptions::default());
        let response = pipe
            .handle(&request("example.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(response.response_code(), ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn negative_answer_is_cached() {
        let (pipe, calls) = pipeline(
            Reply::Empty(ResponseCode::NXDomain),
            PipelineOptions::default(),
        );
        let req = request("nope.example.", RecordType::A);

        let first = pipe.handle(&req, CLIENT).await;
        assert_eq!(first.response_code(), ResponseCode::NXDomain);

        pipe.handle(&req, CLIENT).await;
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "negative answer should be cached"
        );
    }

    #[tokio::test]
    async fn aaaa_is_nodata_when_ipv6_disabled() {
        let (pipe, calls) = pipeline(
            Reply::Aaaa(Ipv6Addr::LOCALHOST),
            PipelineOptions::with_ipv6(false),
        );
        let response = pipe
            .handle(&request("example.com.", RecordType::AAAA), CLIENT)
            .await;

        assert_eq!(response.response_code(), ResponseCode::NoError);
        assert!(response.answers().is_empty());
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "AAAA must not reach the upstream"
        );
    }

    #[tokio::test]
    async fn aaaa_is_forwarded_when_ipv6_enabled() {
        let (pipe, calls) = pipeline(
            Reply::Aaaa(Ipv6Addr::new(0x2606, 0x2800, 0, 0, 0, 0, 0, 1)),
            PipelineOptions::with_ipv6(true),
        );
        let response = pipe
            .handle(&request("example.com.", RecordType::AAAA), CLIENT)
            .await;

        assert_eq!(response.answers().len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn blocked_name_is_nxdomain_without_lookup() {
        use justdns_filter::{Blocker, DomainSet};

        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let blocker = Arc::new(Blocker::new(
            DomainSet::from_rules("ads.example\n"),
            Arc::new(AtomicBool::new(true)),
            DomainSet::new(),
            Arc::new(AtomicBool::new(false)),
        ));
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_blocker(blocker);

        // A blocked name (and a sub-domain of it) is refused without a lookup.
        let blocked = pipe
            .handle(&request("sub.ads.example.", RecordType::A), CLIENT)
            .await;
        assert_eq!(blocked.response_code(), ResponseCode::NXDomain);
        assert!(blocked.answers().is_empty());
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "blocked name must not reach upstream"
        );

        // An allowed name still resolves normally.
        let allowed = pipe
            .handle(&request("example.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(allowed.response_code(), ResponseCode::NoError);
        assert_eq!(allowed.answers().len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn rewrite_answers_locally_without_upstream() {
        use justdns_rewrite::{Rewrite, Rewriter};

        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let rewriter = Arc::new(Rewriter::new());
        rewriter.set(&[Rewrite {
            name: "nas.home".to_owned(),
            ips: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 20, 5))],
            enabled: true,
        }]);
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_rewriter(rewriter);

        // The A query is answered from the rewrite, never forwarded.
        let answered = pipe
            .handle(&request("nas.home.", RecordType::A), CLIENT)
            .await;
        assert_eq!(answered.response_code(), ResponseCode::NoError);
        assert_eq!(
            answered.answers()[0].data().as_a(),
            Some(&A(Ipv4Addr::new(192, 168, 20, 5)))
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0, "rewrite must not forward");

        // AAAA for the same name is NODATA (no IPv6 address, IPv6 off), and still
        // never forwarded.
        let aaaa = pipe
            .handle(&request("nas.home.", RecordType::AAAA), CLIENT)
            .await;
        assert_eq!(aaaa.response_code(), ResponseCode::NoError);
        assert!(aaaa.answers().is_empty());
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        // A name without a rewrite still forwards.
        let other = pipe
            .handle(&request("example.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(other.answers().len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn safe_search_rewrites_to_the_filtered_endpoint() {
        let mock = Mock::new(Reply::A(Ipv4Addr::new(216, 239, 38, 120)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let options = PipelineOptions {
            ipv6_enabled: Arc::new(AtomicBool::new(false)),
            safe_search: Arc::new(AtomicBool::new(true)),
        };
        let pipe = Pipeline::new(DnsCache::new(100), router, options);

        let response = pipe
            .handle(&request("www.google.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(response.response_code(), ResponseCode::NoError);

        // The answer carries a CNAME to the safe-search host plus its address.
        let cname = response
            .answers()
            .iter()
            .find_map(|r| r.data().as_cname().map(|c| c.0.to_ascii()));
        assert_eq!(
            cname.as_deref(),
            Some("forcesafesearch.google.com."),
            "CNAME to the safe-search host"
        );
        assert!(
            response.answers().iter().any(|r| r.data().as_a().is_some()),
            "address record included"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1, "target was resolved once");
    }

    #[tokio::test]
    async fn safe_search_off_leaves_search_engines_alone() {
        let (pipe, calls) = pipeline(
            Reply::A(Ipv4Addr::new(142, 250, 0, 0)),
            PipelineOptions::default(),
        );
        let response = pipe
            .handle(&request("google.com.", RecordType::A), CLIENT)
            .await;
        // Plain A answer, no CNAME rewrite.
        assert!(
            response
                .answers()
                .iter()
                .all(|r| r.data().as_cname().is_none()),
            "no safe-search CNAME when the flag is off"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn missing_question_is_formerr() {
        let (pipe, _calls) = pipeline(Reply::Fail, PipelineOptions::default());
        let mut req = Message::new();
        req.set_id(7);
        let response = pipe.handle(&req, CLIENT).await;
        assert_eq!(response.response_code(), ResponseCode::FormErr);
    }

    #[tokio::test]
    async fn observer_sees_the_handled_query() {
        let observer = Arc::new(Recording::default());
        let router = Arc::new(Router::new(
            UpstreamMode::Racing,
            vec![Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)))],
        ));
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_observer(observer.clone());

        pipe.handle(&request("example.com.", RecordType::A), CLIENT)
            .await;

        let seen = observer.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].client, CLIENT);
        assert_eq!(seen[0].name, "example.com.");
        assert_eq!(seen[0].source, AnswerSource::Upstream);
        assert_eq!(seen[0].answers, 1);
        assert_eq!(
            seen[0].addresses,
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))],
            "records what the query resolved to"
        );
    }

    #[tokio::test]
    async fn observer_reports_rewrite_source_and_addresses() {
        use justdns_rewrite::Rewrite;

        let observer = Arc::new(Recording::default());
        // The rewrite short-circuits before upstream, so this mock is never hit.
        let router = Arc::new(Router::new(
            UpstreamMode::Racing,
            vec![Mock::new(Reply::Fail)],
        ));
        let rewriter = Arc::new(Rewriter::new());
        rewriter.set(&[Rewrite {
            name: "nas.home".to_owned(),
            ips: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 20, 5))],
            enabled: true,
        }]);
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_observer(observer.clone())
            .with_rewriter(rewriter);

        pipe.handle(&request("nas.home.", RecordType::A), CLIENT)
            .await;

        let seen = observer.seen.lock().unwrap();
        assert_eq!(seen[0].source, AnswerSource::Rewrite);
        assert_eq!(
            seen[0].addresses,
            vec![IpAddr::V4(Ipv4Addr::new(192, 168, 20, 5))]
        );
    }

    #[test]
    fn negative_ttl_prefers_soa_minimum() {
        let mut with_soa = Message::new();
        with_soa.add_name_server(Record::from_rdata(
            Name::from_ascii("example.").unwrap(),
            300,
            RData::SOA(SOA::new(
                Name::from_ascii("ns.example.").unwrap(),
                Name::from_ascii("hostmaster.example.").unwrap(),
                1,
                3600,
                600,
                86400,
                30,
            )),
        ));
        assert_eq!(negative_ttl(&with_soa), 30);
        assert_eq!(negative_ttl(&Message::new()), DEFAULT_NEGATIVE_TTL);
    }
}
