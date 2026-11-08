// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query pipeline: cache lookup, upstream forwarding and response building.
//!
//! This is where an incoming query becomes an answer. A cache hit is served
//! straight away; a miss goes to the [`Router`], and the upstream's answer is
//! cached before it is returned. IPv6 answers are suppressed unless enabled.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use hickory_proto::op::DnsResponse;
use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, CNAME};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use justdns_filter::PolicyEngine;
use justdns_rewrite::Rewriter;

use crate::cache::{CacheHit, CacheKey, CachedEntry, CachedResponse, DnsCache};
use crate::observer::{AnswerSource, DnssecStatus, QueryObserver, QueryRecord};
use crate::router::Resolve;
use crate::safesearch;

/// Negative answers without an SOA are cached this long (seconds).
const DEFAULT_NEGATIVE_TTL: u64 = 60;
/// Upper bound on any negative-cache TTL (seconds).
const MAX_NEGATIVE_TTL: u64 = 3600;
/// TTL on a locally rewritten answer. Short, since a LAN mapping can change and
/// the lookup is cheap enough to repeat; long enough to spare a flood of repeats.
const REWRITE_TTL: u32 = 300;
/// TTL on a null-IP answer for a blocked name. Short, so switching a block off
/// (or allowing the name) takes effect almost at once.
const BLOCK_TTL: u32 = 10;

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
    /// How a blocked name is answered: `true` hands back the null IP
    /// (`0.0.0.0` / `::`), `false` answers NXDOMAIN. A shared flag so the config
    /// store flips it live. Defaults to `false` here (NXDOMAIN); the product
    /// default of null-IP is set by the config layer and wired in by the binary.
    pub block_null_ip: Arc<AtomicBool>,
    /// Where a blocked name points when the custom block page is on: the IP the
    /// block-page web server listens on, so the browser connects to it and sees
    /// the explanation page instead of a dead connection. Empty (the common case)
    /// means the block page is off and `block_null_ip` decides the answer. A
    /// shared, swappable slot so the binary can flip it live from the settings.
    pub block_page: Arc<ArcSwapOption<BlockPageTarget>>,
    /// Whether DNSSEC validation is on. The validating is done entirely by the
    /// upstream client (it sets the DO bit, reads the AD flag, and confirms Bogus
    /// with a checking-disabled retry) and the per-query verdict rides out on the
    /// resolved answer, so the pipeline no longer consults this flag directly. It
    /// is kept here because the binary wires the one shared switch to both the
    /// router and these options; flipping it toggles validation live. With
    /// validation off the client never reports a verdict, so no answer is ever
    /// judged Bogus and that path stays inert.
    pub dnssec_validate: Arc<AtomicBool>,
}

/// The address the custom block page is reached at, per family. A blocked `A`
/// query is answered with [`v4`](Self::v4) and a blocked `AAAA` with
/// [`v6`](Self::v6); a family with no address falls back to the null IP so the
/// connection still fails fast rather than leaking to a real host.
#[derive(Clone, Debug)]
pub struct BlockPageTarget {
    /// The IPv4 the block-page server listens on, if any.
    pub v4: Option<Ipv4Addr>,
    /// The IPv6 the block-page server listens on, if any.
    pub v6: Option<Ipv6Addr>,
}

impl PipelineOptions {
    /// Build options with a fresh IPv6 flag set to `enabled`, safe search off,
    /// blocked names answered with NXDOMAIN, the block page off, and DNSSEC
    /// validation off.
    pub fn with_ipv6(enabled: bool) -> Self {
        Self {
            ipv6_enabled: Arc::new(AtomicBool::new(enabled)),
            safe_search: Arc::new(AtomicBool::new(false)),
            block_null_ip: Arc::new(AtomicBool::new(false)),
            block_page: Arc::new(ArcSwapOption::empty()),
            dnssec_validate: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self::with_ipv6(false)
    }
}

/// How a query was answered: where the answer came from and, when it came from
/// an upstream, which resolver gave it. Carried out of [`Pipeline::answer`] so
/// [`Pipeline::handle`] can record both on the query.
struct Answered {
    source: AnswerSource,
    resolver: Option<Arc<str>>,
    /// The DNSSEC verdict for an upstream answer when validation was on; `None`
    /// for a local answer or when validation is off.
    dnssec: Option<DnssecStatus>,
}

impl Answered {
    /// An answer produced without a tracked upstream resolver (cache, block,
    /// rewrite, local). Never carries a DNSSEC verdict.
    fn local(source: AnswerSource) -> Self {
        Self {
            source,
            resolver: None,
            dnssec: None,
        }
    }

    /// An upstream answer attributed to `resolver` (which may be `None` if the
    /// lookup failed before any resolver answered), with its DNSSEC verdict when
    /// validation was on.
    fn upstream(resolver: Option<Arc<str>>, dnssec: Option<DnssecStatus>) -> Self {
        Self {
            source: AnswerSource::Upstream,
            resolver,
            dnssec,
        }
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
    policy_engine: Option<Arc<PolicyEngine>>,
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
            policy_engine: None,
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

    /// Filter queries through `engine`: a name blocked for the query's client is
    /// refused before any cache or upstream lookup. The engine resolves the
    /// client to its effective policy (device, group or global default). Without
    /// one, nothing is blocked.
    #[must_use]
    pub fn with_policy_engine(mut self, engine: Arc<PolicyEngine>) -> Self {
        self.policy_engine = Some(engine);
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

        let mut response = Message::query();
        response.metadata.id = request.id;
        response.metadata.message_type = MessageType::Response;
        response.metadata.op_code = request.op_code;
        response.metadata.recursion_desired = request.recursion_desired;
        response.metadata.recursion_available = true;

        let Some(query) = request.queries.first().cloned() else {
            // No question to answer, so nothing meaningful to record either.
            response.metadata.response_code = ResponseCode::FormErr;
            return response;
        };
        response.add_query(query.clone());

        let answered = self.answer(&query, &mut response, client).await;

        if let Some(observer) = &self.observer {
            observer.observe(QueryRecord {
                client,
                name: query.name().to_string(),
                record_type: u16::from(query.query_type()),
                response_code: u16::from(response.response_code),
                source: answered.source,
                duration: start.elapsed(),
                answers: u16::try_from(response.answers.len()).unwrap_or(u16::MAX),
                addresses: answer_addresses(&response),
                resolver: answered.resolver.as_deref().map(str::to_owned),
                dnssec: answered.dnssec,
            });
        }

        response
    }

    /// Fill `response` with the answer to `query` for `client` and report where
    /// it came from. The client decides which filtering policy applies.
    async fn answer(&self, query: &Query, response: &mut Message, client: IpAddr) -> Answered {
        // A local rewrite is answered first — before the block check and any
        // lookup — and owns the name for every record type.
        if let Some(rewriter) = &self.rewriter {
            if let Some(ips) = rewriter.lookup(&query.name().to_string()) {
                self.answer_rewrite(query, response, &ips);
                return Answered::local(AnswerSource::Rewrite);
            }
        }

        // Blocked names are refused before any lookup, under the policy that
        // applies to this client (its device, its group, or the global default).
        // How depends on the block response: the null IP (so a connection to the
        // blocked host fails at once) or NXDOMAIN (the name simply does not exist).
        if let Some(engine) = &self.policy_engine {
            if engine.blocks(client, &query.name().to_string()) {
                if let Some(target) = self.options.block_page.load_full() {
                    self.answer_blocked_redirect(query, response, &target);
                } else if self.options.block_null_ip.load(Ordering::Relaxed) {
                    self.answer_blocked_null_ip(query, response);
                } else {
                    response.metadata.response_code = ResponseCode::NXDomain;
                }
                return Answered::local(AnswerSource::Blocked);
            }
        }

        // IPv6 off: answer AAAA with NODATA without touching cache or upstream.
        if query.query_type() == RecordType::AAAA
            && !self.options.ipv6_enabled.load(Ordering::Relaxed)
        {
            response.metadata.response_code = ResponseCode::NoError;
            return Answered::local(AnswerSource::Local);
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
            return Answered::local(AnswerSource::Cache);
        }

        let (resolver, dnssec) = match self
            .router
            .resolve(query.name().clone(), query.query_type())
            .await
        {
            Ok(resolved) => {
                // DNSSEC: the upstream client derives the verdict from the
                // validating resolver's AD flag (Secure/Insecure) and confirms a
                // Bogus answer with a checking-disabled retry. A Bogus answer is
                // a validation failure, not a usable answer: a validating
                // resolver must withhold it (RFC 4035 §5.5), so it becomes
                // SERVFAIL with no answers and is never cached — a forged or
                // tampered answer thus never reaches the client. The client only
                // sets a verdict while validation is on, so with it off the Bogus
                // branch stays inert and no verdict is recorded.
                if resolved.dnssec == Some(DnssecStatus::Bogus) {
                    tracing::warn!(
                        name = %query.name(),
                        record_type = %query.query_type(),
                        resolver = resolved.resolver.as_deref().unwrap_or("?"),
                        "DNSSEC validation failed (Bogus) — refusing with SERVFAIL",
                    );
                    response.metadata.response_code = ResponseCode::ServFail;
                } else {
                    response.metadata.response_code = resolved.response.response_code;
                    for record in &resolved.response.answers {
                        response.add_answer(record.clone());
                    }
                    self.cache
                        .insert(key, entry_from_response(&resolved.response))
                        .await;
                }
                (resolved.resolver, resolved.dnssec)
            }
            Err(_) => {
                response.metadata.response_code = ResponseCode::ServFail;
                (None, None)
            }
        };

        Answered::upstream(resolver, dnssec)
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
            if let Ok(resolved) = router.resolve(name, record_type).await {
                // Never refresh the cache with a bogus answer (see the miss
                // path): a failed-validation answer must not be served later.
                if resolved.dnssec != Some(DnssecStatus::Bogus) {
                    cache
                        .insert(key.clone(), entry_from_response(&resolved.response))
                        .await;
                }
            }
            refreshing
                .lock()
                .expect("refresh set is not poisoned")
                .remove(&key);
        });
    }

    /// Answer a blocked `query` with the null IP: `0.0.0.0` for an A query, `::`
    /// for an AAAA query, and NODATA (NOERROR with no answer) for any other type.
    /// The client then fails to connect to the blocked host at once, which apps
    /// and video players handle more gracefully than a retried NXDOMAIN.
    fn answer_blocked_null_ip(&self, query: &Query, response: &mut Message) {
        response.metadata.response_code = ResponseCode::NoError;
        let name = query.name().clone();
        match query.query_type() {
            RecordType::A => {
                response.add_answer(Record::from_rdata(
                    name,
                    BLOCK_TTL,
                    RData::A(A(Ipv4Addr::UNSPECIFIED)),
                ));
            }
            RecordType::AAAA => {
                response.add_answer(Record::from_rdata(
                    name,
                    BLOCK_TTL,
                    RData::AAAA(AAAA(Ipv6Addr::UNSPECIFIED)),
                ));
            }
            _ => {}
        }
    }

    /// Answer a blocked `query` by pointing it at the block-page server: an `A`
    /// query gets the configured IPv4 **only when one is set**, an `AAAA` query the
    /// configured IPv6 **only when one is set** — with none, each is NODATA (not
    /// `0.0.0.0` / `::`), and any other type is NODATA too. The browser then
    /// connects to the block page instead of the blocked host.
    ///
    /// Neither family is ever answered with the all-zeros address. A browser
    /// rejects `0.0.0.0` / `::` outright with `ERR_ADDRESS_INVALID` rather than
    /// showing an error it can explain, so when this node has no block-page IP of
    /// its own (the page is on cluster-wide but `server_ipv4` is unset here, or it
    /// was mis-typed to something that is not an IP) NODATA is the clean fallback:
    /// the name resolves to nothing and the browser reports a plain "can't reach",
    /// not the scary invalid-address page. A dual-stack browser also prefers IPv6,
    /// so a stray `::` would shadow a correct `A` — another reason to stay empty.
    fn answer_blocked_redirect(
        &self,
        query: &Query,
        response: &mut Message,
        target: &BlockPageTarget,
    ) {
        response.metadata.response_code = ResponseCode::NoError;
        let name = query.name().clone();
        match query.query_type() {
            // Only answer A with a real configured IPv4. Without one, leave the
            // answer empty (NODATA) rather than returning 0.0.0.0.
            RecordType::A => {
                if let Some(ip) = target.v4 {
                    response.add_answer(Record::from_rdata(name, BLOCK_TTL, RData::A(A(ip))));
                }
            }
            // Only answer AAAA with a real configured IPv6. Without one, leave the
            // answer empty (NODATA) so the client falls back to the IPv4 block page.
            RecordType::AAAA => {
                if let Some(ip) = target.v6 {
                    response.add_answer(Record::from_rdata(name, BLOCK_TTL, RData::AAAA(AAAA(ip))));
                }
            }
            _ => {}
        }
    }

    /// Answer `query` from a rewrite's `ips`. An A query gets the IPv4 addresses
    /// and an AAAA query the IPv6 ones (the latter only when IPv6 is enabled);
    /// any other type, or a family with no address, is NODATA (NOERROR with no
    /// answers). Either way the name is owned locally and never forwarded.
    fn answer_rewrite(&self, query: &Query, response: &mut Message, ips: &[IpAddr]) {
        response.metadata.response_code = ResponseCode::NoError;
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
    ) -> Answered {
        let resolver = match self
            .router
            .resolve(target.clone(), query.query_type())
            .await
        {
            Ok(resolved) => {
                response.metadata.response_code = resolved.response.response_code;
                response.add_answer(Record::from_rdata(
                    query.name().clone(),
                    REWRITE_TTL,
                    RData::CNAME(CNAME(target)),
                ));
                for record in &resolved.response.answers {
                    response.add_answer(record.clone());
                }
                resolved.resolver
            }
            Err(_) => {
                response.metadata.response_code = ResponseCode::ServFail;
                None
            }
        };
        // A safe-search answer is a local rewrite over a resolved target, not a
        // validated answer for the queried name, so it carries no DNSSEC verdict.
        Answered::upstream(resolver, None)
    }
}

/// The IPv4/IPv6 addresses in a response's answer section, in order. Non-address
/// records (CNAME, SOA, …) are skipped, so a CNAME chain reports only the final
/// addresses. Used to record what a query actually resolved to in the query log.
fn answer_addresses(response: &Message) -> Vec<IpAddr> {
    response
        .answers
        .iter()
        .filter_map(|record| match &record.data {
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
    let answers = &upstream.answers;
    if answers.is_empty() {
        CachedEntry::negative(
            upstream.response_code,
            Duration::from_secs(negative_ttl(upstream)),
        )
    } else {
        let ttl = answers.iter().map(|record| record.ttl).min().unwrap_or(0);
        CachedEntry::positive(answers.to_vec(), Duration::from_secs(u64::from(ttl)))
    }
}

fn apply_cached(response: &mut Message, hit: CachedResponse) {
    match hit {
        CachedResponse::Positive(records) => {
            response.metadata.response_code = ResponseCode::NoError;
            for record in records {
                response.add_answer(record);
            }
        }
        CachedResponse::Negative(code) => {
            response.metadata.response_code = code;
        }
    }
}

/// The TTL to cache a negative answer for: the SOA minimum from the authority
/// section when present, otherwise a short default, and never above the cap.
fn negative_ttl(message: &Message) -> u64 {
    message
        .authorities
        .iter()
        .find_map(|record| match &record.data {
            RData::SOA(soa) => Some(u64::from(soa.minimum)),
            _ => None,
        })
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
    use crate::router::{Resolved, Router};

    const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 20, 10));

    /// A policy engine whose global default blocks `ads.example` for every
    /// client, used by the block tests in place of the old global blocker.
    fn block_ads_engine() -> Arc<PolicyEngine> {
        use justdns_filter::{BlockCategory, BlockSource, DomainSet, Policy};
        let engine = PolicyEngine::new();
        engine.publish(
            Policy::new(
                DomainSet::new(),
                vec![BlockSource {
                    category: BlockCategory::AdsTrackers,
                    label: "Werbung & Tracker".into(),
                    set: DomainSet::from_rules("ads.example\n"),
                }],
            ),
            vec![],
        );
        Arc::new(engine)
    }

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
    use hickory_proto::op::DnsResponse;
    use hickory_proto::op::Query;
    use hickory_proto::rr::rdata::{A, AAAA, SOA};
    use hickory_proto::rr::{Name, RData, Record};

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
        /// The DNSSEC verdict this mock reports alongside its answer, standing in
        /// for the verdict the real upstream client derives from the AD flag (and
        /// a checking-disabled retry). `None` models validation being off.
        dnssec: Option<DnssecStatus>,
        calls: Arc<AtomicUsize>,
        resolver: Option<Arc<str>>,
    }

    impl Mock {
        fn new(reply: Reply) -> Self {
            Self {
                reply,
                ttl: 120,
                dnssec: None,
                calls: Arc::new(AtomicUsize::new(0)),
                resolver: Some(Arc::from("mock-resolver")),
            }
        }

        /// Answer with records carrying `ttl` seconds instead of the default, so
        /// a test can make an answer expire (or go stale) on demand.
        fn with_ttl(mut self, ttl: u32) -> Self {
            self.ttl = ttl;
            self
        }

        /// Report `status` as the answer's DNSSEC verdict, as the validating
        /// upstream client would.
        fn with_dnssec(mut self, status: DnssecStatus) -> Self {
            self.dnssec = Some(status);
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
            let mut message = Message::query();
            message.metadata.message_type = MessageType::Response;
            match &self.reply {
                Reply::A(ip) => {
                    message.metadata.response_code = ResponseCode::NoError;
                    message.add_answer(Record::from_rdata(name, self.ttl, RData::A(A(*ip))));
                }
                Reply::Aaaa(ip) => {
                    message.metadata.response_code = ResponseCode::NoError;
                    message.add_answer(Record::from_rdata(name, self.ttl, RData::AAAA(AAAA(*ip))));
                }
                Reply::Empty(code) => {
                    message.metadata.response_code = *code;
                }
                Reply::Fail => return Err(ProtoError::from("mock failure")),
            }
            Ok(Resolved::new(
                DnsResponse::from_message(message)?,
                self.resolver.clone(),
                self.dnssec,
            ))
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
        let mut message = Message::query();
        message.metadata.id = 4242;
        message.metadata.recursion_desired = true;
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
        assert_eq!(first.id, 4242);
        assert_eq!(first.response_code, ResponseCode::NoError);
        assert_eq!(
            first.answers[0].data,
            RData::A(A(Ipv4Addr::new(93, 184, 216, 34)))
        );

        let second = pipe.handle(&req, CLIENT).await;
        assert_eq!(second.answers.len(), 1);
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
        assert_eq!(stale.answers.len(), 1, "stale answer is still served");

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
        assert_eq!(response.response_code, ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn bogus_answer_is_servfail_and_not_cached() {
        // The upstream client reports a signature failure as a Bogus verdict
        // (with a SERVFAIL response). The pipeline must refuse it with SERVFAIL,
        // drop the answers, and never cache it — so a second query forwards again
        // rather than serving a cached forgery.
        let mock =
            Mock::new(Reply::A(Ipv4Addr::new(203, 0, 113, 9))).with_dnssec(DnssecStatus::Bogus);
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default());
        let req = request("sigfail.example.", RecordType::A);

        let first = pipe.handle(&req, CLIENT).await;
        assert_eq!(first.response_code, ResponseCode::ServFail);
        assert!(first.answers.is_empty(), "a bogus answer is withheld");

        let second = pipe.handle(&req, CLIENT).await;
        assert_eq!(second.response_code, ResponseCode::ServFail);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "a bogus answer must not be cached"
        );
    }

    #[tokio::test]
    async fn secure_and_insecure_answers_pass_through() {
        // Secure and Insecure both resolve normally — only Bogus is refused.
        for status in [
            DnssecStatus::Secure,
            DnssecStatus::Insecure,
            DnssecStatus::Indeterminate,
        ] {
            let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34))).with_dnssec(status);
            let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
            let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default());
            let response = pipe
                .handle(&request("example.com.", RecordType::A), CLIENT)
                .await;
            assert_eq!(response.response_code, ResponseCode::NoError, "{status:?}");
            assert_eq!(response.answers.len(), 1, "{status:?}");
        }
    }

    #[tokio::test]
    async fn validated_queries_record_their_verdict() {
        // The verdict the upstream client reported is carried out to the
        // observer — Secure/Insecure for a good answer, Bogus even though the
        // answer itself is refused (so the DNSSEC log still shows the failure).
        for status in [
            DnssecStatus::Secure,
            DnssecStatus::Insecure,
            DnssecStatus::Bogus,
        ] {
            let observer = Arc::new(Recording::default());
            let router = Arc::new(Router::new(
                UpstreamMode::Racing,
                vec![Mock::new(Reply::A(Ipv4Addr::new(1, 1, 1, 1))).with_dnssec(status)],
            ));
            let options = PipelineOptions::default();
            options.dnssec_validate.store(true, Ordering::Relaxed);
            let pipe =
                Pipeline::new(DnsCache::new(100), router, options).with_observer(observer.clone());

            pipe.handle(&request("example.com.", RecordType::A), CLIENT)
                .await;

            let seen = observer.seen.lock().unwrap();
            assert_eq!(seen[0].dnssec, Some(status), "{status:?}");
        }
    }

    #[tokio::test]
    async fn no_verdict_recorded_when_validation_off() {
        // Validation off: no DNSSEC verdict is recorded, so the log is not
        // flooded with Indeterminate.
        let observer = Arc::new(Recording::default());
        let router = Arc::new(Router::new(
            UpstreamMode::Racing,
            vec![Mock::new(Reply::A(Ipv4Addr::new(1, 1, 1, 1)))],
        ));
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_observer(observer.clone());

        pipe.handle(&request("example.com.", RecordType::A), CLIENT)
            .await;

        assert_eq!(observer.seen.lock().unwrap()[0].dnssec, None);
    }

    #[tokio::test]
    async fn negative_answer_is_cached() {
        let (pipe, calls) = pipeline(
            Reply::Empty(ResponseCode::NXDomain),
            PipelineOptions::default(),
        );
        let req = request("nope.example.", RecordType::A);

        let first = pipe.handle(&req, CLIENT).await;
        assert_eq!(first.response_code, ResponseCode::NXDomain);

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

        assert_eq!(response.response_code, ResponseCode::NoError);
        assert!(response.answers.is_empty());
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

        assert_eq!(response.answers.len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn blocked_name_is_nxdomain_without_lookup() {
        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_policy_engine(block_ads_engine());

        // A blocked name (and a sub-domain of it) is refused without a lookup.
        let blocked = pipe
            .handle(&request("sub.ads.example.", RecordType::A), CLIENT)
            .await;
        assert_eq!(blocked.response_code, ResponseCode::NXDomain);
        assert!(blocked.answers.is_empty());
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "blocked name must not reach upstream"
        );

        // An allowed name still resolves normally.
        let allowed = pipe
            .handle(&request("example.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(allowed.response_code, ResponseCode::NoError);
        assert_eq!(allowed.answers.len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn blocked_name_is_null_ip_when_configured() {
        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let options = PipelineOptions {
            block_null_ip: Arc::new(AtomicBool::new(true)),
            ..PipelineOptions::default()
        };
        let pipe = Pipeline::new(DnsCache::new(100), router, options)
            .with_policy_engine(block_ads_engine());

        // A blocked A query is answered with 0.0.0.0, not NXDOMAIN, and never
        // reaches upstream.
        let blocked = pipe
            .handle(&request("sub.ads.example.", RecordType::A), CLIENT)
            .await;
        assert_eq!(blocked.response_code, ResponseCode::NoError);
        assert_eq!(blocked.answers.len(), 1);
        assert_eq!(
            answer_addresses(&blocked),
            vec![IpAddr::V4(Ipv4Addr::UNSPECIFIED)],
            "A block answers 0.0.0.0"
        );

        // A blocked AAAA query is answered with :: — even with IPv6 off, since a
        // block is an explicit null answer, not a real address.
        let blocked_v6 = pipe
            .handle(&request("ads.example.", RecordType::AAAA), CLIENT)
            .await;
        assert_eq!(blocked_v6.response_code, ResponseCode::NoError);
        assert_eq!(
            answer_addresses(&blocked_v6),
            vec![IpAddr::V6(Ipv6Addr::UNSPECIFIED)],
            "AAAA block answers ::"
        );

        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "a blocked name never reaches upstream"
        );
    }

    #[tokio::test]
    async fn blocked_name_points_at_block_page_when_set() {
        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let options = PipelineOptions::default();
        // The block page takes precedence over the null-IP/NXDOMAIN response.
        options.block_page.store(Some(Arc::new(BlockPageTarget {
            v4: Some(Ipv4Addr::new(192, 168, 20, 2)),
            v6: None,
        })));
        // Keep a handle to the shared switch so the test can flip the target after
        // the pipeline has taken ownership of `options`.
        let block_page = options.block_page.clone();
        let pipe = Pipeline::new(DnsCache::new(100), router, options)
            .with_policy_engine(block_ads_engine());

        // A blocked A query is answered with the block-page IP, not 0.0.0.0.
        let blocked = pipe
            .handle(&request("ads.example.", RecordType::A), CLIENT)
            .await;
        assert_eq!(blocked.response_code, ResponseCode::NoError);
        assert_eq!(
            answer_addresses(&blocked),
            vec![IpAddr::V4(Ipv4Addr::new(192, 168, 20, 2))],
            "A block points at the block-page server"
        );

        // AAAA with no configured v6 is NODATA (NOERROR, no answer), not ::, so a
        // dual-stack browser falls back to the IPv4 block page instead of trying
        // the all-zeros address and rejecting it as ERR_ADDRESS_INVALID.
        let blocked_v6 = pipe
            .handle(&request("ads.example.", RecordType::AAAA), CLIENT)
            .await;
        assert_eq!(blocked_v6.response_code, ResponseCode::NoError);
        assert!(
            answer_addresses(&blocked_v6).is_empty(),
            "AAAA with no block-page v6 is NODATA, never ::"
        );

        // With a configured IPv6, AAAA points at it so IPv6-only clients reach the
        // block page too.
        block_page.store(Some(Arc::new(BlockPageTarget {
            v4: Some(Ipv4Addr::new(192, 168, 20, 2)),
            v6: Some(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2)),
        })));
        let blocked_v6_set = pipe
            .handle(&request("ads.example.", RecordType::AAAA), CLIENT)
            .await;
        assert_eq!(
            answer_addresses(&blocked_v6_set),
            vec![IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 2))],
            "AAAA with a configured block-page v6 answers it"
        );

        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "block never reaches upstream"
        );
    }

    #[tokio::test]
    async fn blocked_a_with_no_block_page_ipv4_is_nodata_not_zeros() {
        // The block page is on cluster-wide but this node has no block-page IPv4 of
        // its own (unset, or mis-typed to a non-IP which parses to None). A blocked
        // A query must then be NODATA, never 0.0.0.0 — a browser rejects the
        // all-zeros address with ERR_ADDRESS_INVALID instead of a clean "can't
        // reach".
        let mock = Mock::new(Reply::A(Ipv4Addr::new(93, 184, 216, 34)));
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let options = PipelineOptions::default();
        options
            .block_page
            .store(Some(Arc::new(BlockPageTarget { v4: None, v6: None })));
        let pipe = Pipeline::new(DnsCache::new(100), router, options)
            .with_policy_engine(block_ads_engine());
        let blocked = pipe
            .handle(&request("ads.example.", RecordType::A), CLIENT)
            .await;
        assert_eq!(blocked.response_code, ResponseCode::NoError);
        assert!(
            answer_addresses(&blocked).is_empty(),
            "A block with no block-page IPv4 is NODATA, never 0.0.0.0"
        );
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
            aliases: Vec::new(),
            enabled: true,
        }]);
        let pipe = Pipeline::new(DnsCache::new(100), router, PipelineOptions::default())
            .with_rewriter(rewriter);

        // The A query is answered from the rewrite, never forwarded.
        let answered = pipe
            .handle(&request("nas.home.", RecordType::A), CLIENT)
            .await;
        assert_eq!(answered.response_code, ResponseCode::NoError);
        assert_eq!(
            answered.answers[0].data,
            RData::A(A(Ipv4Addr::new(192, 168, 20, 5)))
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0, "rewrite must not forward");

        // AAAA for the same name is NODATA (no IPv6 address, IPv6 off), and still
        // never forwarded.
        let aaaa = pipe
            .handle(&request("nas.home.", RecordType::AAAA), CLIENT)
            .await;
        assert_eq!(aaaa.response_code, ResponseCode::NoError);
        assert!(aaaa.answers.is_empty());
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        // A name without a rewrite still forwards.
        let other = pipe
            .handle(&request("example.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(other.answers.len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn safe_search_rewrites_to_the_filtered_endpoint() {
        let mock = Mock::new(Reply::A(Ipv4Addr::new(216, 239, 38, 120)));
        let calls = mock.calls.clone();
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![mock]));
        let options = PipelineOptions {
            safe_search: Arc::new(AtomicBool::new(true)),
            ..PipelineOptions::default()
        };
        let pipe = Pipeline::new(DnsCache::new(100), router, options);

        let response = pipe
            .handle(&request("www.google.com.", RecordType::A), CLIENT)
            .await;
        assert_eq!(response.response_code, ResponseCode::NoError);

        // The answer carries a CNAME to the safe-search host plus its address.
        let cname = response.answers.iter().find_map(|r| match &r.data {
            RData::CNAME(c) => Some(c.0.to_ascii()),
            _ => None,
        });
        assert_eq!(
            cname.as_deref(),
            Some("forcesafesearch.google.com."),
            "CNAME to the safe-search host"
        );
        assert!(
            response
                .answers
                .iter()
                .any(|r| matches!(&r.data, RData::A(_))),
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
                .answers
                .iter()
                .all(|r| !matches!(&r.data, RData::CNAME(_))),
            "no safe-search CNAME when the flag is off"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn missing_question_is_formerr() {
        let (pipe, _calls) = pipeline(Reply::Fail, PipelineOptions::default());
        let mut req = Message::query();
        req.metadata.id = 7;
        let response = pipe.handle(&req, CLIENT).await;
        assert_eq!(response.response_code, ResponseCode::FormErr);
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
        assert_eq!(
            seen[0].resolver.as_deref(),
            Some("mock-resolver"),
            "an upstream answer records the resolver that gave it"
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
            aliases: Vec::new(),
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
        assert_eq!(
            seen[0].resolver, None,
            "a locally answered query has no upstream resolver"
        );
    }

    #[test]
    fn negative_ttl_prefers_soa_minimum() {
        let mut with_soa = Message::query();
        with_soa.add_authority(Record::from_rdata(
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
        assert_eq!(negative_ttl(&Message::query()), DEFAULT_NEGATIVE_TTL);
    }
}
