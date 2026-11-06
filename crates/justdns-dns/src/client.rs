// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Network client that forwards a query to a single upstream.
//!
//! Each client wraps a hickory name-server pool configured for one upstream.
//! For a plain upstream the pool holds both a UDP and a TCP name server, so a
//! truncated UDP answer is retried over TCP. Selecting between several
//! upstreams (racing, fallback, rule-based) is handled a layer up; this client
//! only speaks to the one it was built for.

use std::sync::Arc;

use hickory_proto::ProtoError;
use hickory_proto::op::{
    DnsRequestOptions, DnsResponse, Message, MessageType, Query, ResponseCode,
};
use hickory_proto::rr::{Name, Record, RecordType};
use hickory_resolver::config::{NameServerConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::net::xfer::FirstAnswer;
use hickory_resolver::net::{DnsError, DnsHandle, NetError};
use hickory_resolver::{NameServerPool, PoolContext, TlsConfig};

use crate::upstream::{Transport, Upstream};

pub struct UpstreamClient {
    pool: NameServerPool<TokioRuntimeProvider>,
    /// A short label identifying this resolver (its certificate name, or plain
    /// IP), computed once so each answer can be attributed to it in the query
    /// log without re-deriving it per query.
    label: Arc<str>,
}

impl UpstreamClient {
    pub fn new(upstream: &Upstream) -> Self {
        // `trust_negative_responses` only matters when a pool spreads a query
        // across several servers; each client holds one upstream, so it has no
        // effect here and we leave it at the config defaults.
        let ip = upstream.address;
        let mut config = match &upstream.transport {
            Transport::Plain => NameServerConfig::udp_and_tcp(ip),
            Transport::Dot { server_name } => {
                NameServerConfig::tls(ip, Arc::from(server_name.as_str()))
            }
            Transport::Doh { server_name } => {
                NameServerConfig::https(ip, Arc::from(server_name.as_str()), None)
            }
        };
        // The transport constructors default to each protocol's standard port;
        // apply the upstream's configured port to every connection so a custom
        // port (and the ephemeral ports the tests bind to) is honoured.
        for connection in &mut config.connections {
            connection.port = upstream.port;
        }

        let context = Arc::new(PoolContext::new(
            ResolverOpts::default(),
            // Building the default client TLS config (ring provider + webpki
            // roots) is deterministic; it only fails if the crypto provider is
            // missing, which the `tls-ring` feature compiles in, so a failure
            // here is a fatal misconfiguration rather than a runtime condition.
            TlsConfig::new().expect("ring TLS client config initialises"),
        ));

        let pool = NameServerPool::from_config([config], context, TokioRuntimeProvider::new());

        Self {
            pool,
            label: Arc::from(upstream.label()),
        }
    }

    /// The label identifying this resolver, attached to each answer it gives so
    /// the query log can attribute it.
    pub(crate) fn label(&self) -> Arc<str> {
        self.label.clone()
    }

    /// Forward a single query and return the upstream's raw response, TTLs and
    /// response code intact, for the cache and the response path to use.
    pub async fn query(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> Result<DnsResponse, ProtoError> {
        let mut options = DnsRequestOptions::default();
        options.recursion_desired = true;

        let error = match self
            .pool
            .lookup(Query::query(name.clone(), record_type), options)
            .first_answer()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error) => error,
        };

        // hickory reports an empty answer — NXDOMAIN or NODATA — as a
        // `NoRecordsFound` error rather than a response. Rebuild it into a real
        // negative response so the pipeline relays the true response code and
        // can cache it, instead of turning every "no such name / no such record"
        // into SERVFAIL. Genuine failures (SERVFAIL, REFUSED, transport errors)
        // reach here too but keep their error, so racing still prefers a working
        // upstream over a failing one.
        if let NetError::Dns(DnsError::NoRecordsFound(no_records)) = &error {
            if matches!(
                no_records.response_code,
                ResponseCode::NXDomain | ResponseCode::NoError
            ) {
                return negative_response(
                    name,
                    record_type,
                    no_records.response_code,
                    &no_records.authorities,
                );
            }
        }

        // Upstream transport failures are hickory-net's own error type; fold
        // them into the crate's `ProtoError` boundary so the `Resolve` trait
        // stays transport-agnostic. The pipeline only ever maps an error to
        // SERVFAIL, so the message text is all that need survive the conversion.
        Err(ProtoError::from(error.to_string()))
    }
}

/// Build a negative response (no answers) carrying `response_code` and the
/// authority section, so the caller relays NXDOMAIN / NODATA correctly and the
/// negative cache can read the SOA minimum TTL from the authority records.
fn negative_response(
    name: Name,
    record_type: RecordType,
    response_code: ResponseCode,
    authorities: &Option<Arc<[Record]>>,
) -> Result<DnsResponse, ProtoError> {
    let mut message = Message::query();
    message.metadata.message_type = MessageType::Response;
    message.metadata.response_code = response_code;
    message.add_query(Query::query(name, record_type));
    if let Some(authorities) = authorities {
        message.add_authorities(authorities.iter().cloned());
    }
    DnsResponse::from_message(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use hickory_proto::op::{Message, MessageType, ResponseCode};
    use hickory_proto::rr::rdata::{A, SOA};
    use hickory_proto::rr::{RData, Record};
    use tokio::net::UdpSocket;

    use crate::upstream::Upstream;

    const ANSWER: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);

    /// A one-shot UDP responder: answers the first query with a fixed A record
    /// echoing the request id, then returns its bound address.
    async fn spawn_mock_upstream() -> SocketAddr {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = socket.local_addr().unwrap();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 512];
            let (len, src) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();

            let mut response = Message::query();
            response.metadata.id = request.id;
            response.metadata.message_type = MessageType::Response;
            response.metadata.response_code = ResponseCode::NoError;
            response.metadata.recursion_desired = true;
            response.metadata.recursion_available = true;
            if let Some(query) = request.queries.first() {
                response.add_query(query.clone());
                response.add_answer(Record::from_rdata(
                    query.name().clone(),
                    60,
                    RData::A(A(ANSWER)),
                ));
            }

            socket
                .send_to(&response.to_vec().unwrap(), src)
                .await
                .unwrap();
        });

        addr
    }

    /// A one-shot UDP responder that answers with `code`, no answer records, and
    /// optionally an SOA in the authority section — i.e. a negative answer.
    async fn spawn_mock_negative(code: ResponseCode, with_soa: bool) -> SocketAddr {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = socket.local_addr().unwrap();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 512];
            let (len, src) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();

            let mut response = Message::query();
            response.metadata.id = request.id;
            response.metadata.message_type = MessageType::Response;
            response.metadata.response_code = code;
            response.metadata.recursion_desired = true;
            response.metadata.recursion_available = true;
            if let Some(query) = request.queries.first() {
                response.add_query(query.clone());
                if with_soa {
                    response.add_authority(Record::from_rdata(
                        query.name().clone(),
                        30,
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
                }
            }

            socket
                .send_to(&response.to_vec().unwrap(), src)
                .await
                .unwrap();
        });

        addr
    }

    async fn client_for(addr: SocketAddr) -> UpstreamClient {
        let mut upstream = Upstream::plain(IpAddr::V4(Ipv4Addr::LOCALHOST));
        upstream.port = addr.port();
        UpstreamClient::new(&upstream)
    }

    #[tokio::test]
    async fn forwards_query_and_returns_answer() {
        let client = client_for(spawn_mock_upstream().await).await;

        let response = client
            .query(Name::from_ascii("example.com.").unwrap(), RecordType::A)
            .await
            .expect("query failed");

        assert_eq!(response.response_code, ResponseCode::NoError);
        assert_eq!(response.answers[0].data, RData::A(A(ANSWER)));
    }

    #[tokio::test]
    async fn nxdomain_is_a_response_not_an_error() {
        let client = client_for(spawn_mock_negative(ResponseCode::NXDomain, true).await).await;

        let response = client
            .query(Name::from_ascii("nope.example.").unwrap(), RecordType::A)
            .await
            .expect("NXDOMAIN must come back as a response, not an error");

        assert_eq!(response.response_code, ResponseCode::NXDomain);
        assert!(response.answers.is_empty());
        assert!(
            response
                .authorities
                .iter()
                .any(|record| matches!(&record.data, RData::SOA(_))),
            "the SOA authority should be preserved for negative caching"
        );
    }

    #[tokio::test]
    async fn nodata_is_a_response_not_an_error() {
        let client = client_for(spawn_mock_negative(ResponseCode::NoError, true).await).await;

        // A name that exists but has no HTTPS record: NOERROR with no answers.
        let response = client
            .query(Name::from_ascii("example.com.").unwrap(), RecordType::HTTPS)
            .await
            .expect("NODATA must come back as a response, not an error");

        assert_eq!(response.response_code, ResponseCode::NoError);
        assert!(response.answers.is_empty());
    }
}
