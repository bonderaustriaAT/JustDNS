// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query pipeline: cache lookup, upstream forwarding and response building.
//!
//! This is where an incoming query becomes an answer. A cache hit is served
//! straight away; a miss goes to the [`Router`], and the upstream's answer is
//! cached before it is returned. IPv6 answers are suppressed unless enabled.

use std::time::Duration;

use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::rr::RecordType;

use crate::cache::{CacheKey, CachedEntry, CachedResponse, DnsCache};
use crate::router::{Resolve, Router};

/// Negative answers without an SOA are cached this long (seconds).
const DEFAULT_NEGATIVE_TTL: u64 = 60;
/// Upper bound on any negative-cache TTL (seconds).
const MAX_NEGATIVE_TTL: u64 = 3600;

#[derive(Clone, Copy, Debug, Default)]
pub struct PipelineOptions {
    /// Whether to answer AAAA queries. Off by default (`false`), so IPv6 names
    /// resolve to nothing and clients fall back to IPv4.
    pub ipv6_enabled: bool,
}

pub struct Pipeline<R> {
    cache: DnsCache,
    router: Router<R>,
    options: PipelineOptions,
}

impl<R: Resolve + Sync> Pipeline<R> {
    pub fn new(cache: DnsCache, router: Router<R>, options: PipelineOptions) -> Self {
        Self {
            cache,
            router,
            options,
        }
    }

    /// Turn a request message into a response message.
    pub async fn handle(&self, request: &Message) -> Message {
        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(request.op_code());
        response.set_recursion_desired(request.recursion_desired());
        response.set_recursion_available(true);

        let Some(query) = request.queries().first().cloned() else {
            response.set_response_code(ResponseCode::FormErr);
            return response;
        };
        response.add_query(query.clone());

        // IPv6 off: answer AAAA with NODATA without touching cache or upstream.
        if query.query_type() == RecordType::AAAA && !self.options.ipv6_enabled {
            response.set_response_code(ResponseCode::NoError);
            return response;
        }

        let key = CacheKey::new(query.name(), query.query_type());

        if let Some(hit) = self.cache.get(&key).await {
            apply_cached(&mut response, hit);
            return response;
        }

        match self
            .router
            .resolve(query.name().clone(), query.query_type())
            .await
        {
            Ok(upstream) => {
                let rcode = upstream.response_code();
                response.set_response_code(rcode);

                let answers = upstream.answers().to_vec();
                if answers.is_empty() {
                    let ttl = negative_ttl(&upstream);
                    self.cache
                        .insert(key, CachedEntry::negative(rcode, Duration::from_secs(ttl)))
                        .await;
                } else {
                    let ttl = answers.iter().map(|record| record.ttl()).min().unwrap_or(0);
                    self.cache
                        .insert(
                            key,
                            CachedEntry::positive(
                                answers.clone(),
                                Duration::from_secs(u64::from(ttl)),
                            ),
                        )
                        .await;
                    for record in answers {
                        response.add_answer(record);
                    }
                }
            }
            Err(_) => {
                response.set_response_code(ResponseCode::ServFail);
            }
        }

        response
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

    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        calls: Arc<AtomicUsize>,
    }

    impl Mock {
        fn new(reply: Reply) -> Self {
            Self {
                reply,
                calls: Arc::new(AtomicUsize::new(0)),
            }
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
                    message.add_answer(Record::from_rdata(name, 120, RData::A(A(*ip))));
                }
                Reply::Aaaa(ip) => {
                    message.set_response_code(ResponseCode::NoError);
                    message.add_answer(Record::from_rdata(name, 120, RData::AAAA(AAAA(*ip))));
                }
                Reply::Empty(code) => {
                    message.set_response_code(*code);
                }
                Reply::Fail => return Err(ProtoError::from("mock failure")),
            }
            DnsResponse::from_message(message)
        }
    }

    fn pipeline(reply: Reply, options: PipelineOptions) -> (Pipeline<Mock>, Arc<AtomicUsize>) {
        let mock = Mock::new(reply);
        let calls = mock.calls.clone();
        let router = Router::new(UpstreamMode::Racing, vec![mock]);
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

        let first = pipe.handle(&req).await;
        assert_eq!(first.id(), 4242);
        assert_eq!(first.response_code(), ResponseCode::NoError);
        assert_eq!(
            first.answers()[0].data().as_a(),
            Some(&A(Ipv4Addr::new(93, 184, 216, 34)))
        );

        let second = pipe.handle(&req).await;
        assert_eq!(second.answers().len(), 1);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "second query should hit the cache"
        );
    }

    #[tokio::test]
    async fn upstream_failure_is_servfail() {
        let (pipe, _calls) = pipeline(Reply::Fail, PipelineOptions::default());
        let response = pipe.handle(&request("example.com.", RecordType::A)).await;
        assert_eq!(response.response_code(), ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn negative_answer_is_cached() {
        let (pipe, calls) = pipeline(
            Reply::Empty(ResponseCode::NXDomain),
            PipelineOptions::default(),
        );
        let req = request("nope.example.", RecordType::A);

        let first = pipe.handle(&req).await;
        assert_eq!(first.response_code(), ResponseCode::NXDomain);

        pipe.handle(&req).await;
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
            PipelineOptions {
                ipv6_enabled: false,
            },
        );
        let response = pipe
            .handle(&request("example.com.", RecordType::AAAA))
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
            PipelineOptions { ipv6_enabled: true },
        );
        let response = pipe
            .handle(&request("example.com.", RecordType::AAAA))
            .await;

        assert_eq!(response.answers().len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn missing_question_is_formerr() {
        let (pipe, _calls) = pipeline(Reply::Fail, PipelineOptions::default());
        let mut req = Message::new();
        req.set_id(7);
        let response = pipe.handle(&req).await;
        assert_eq!(response.response_code(), ResponseCode::FormErr);
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
