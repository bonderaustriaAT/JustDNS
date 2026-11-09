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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use hickory_proto::ProtoError;
use hickory_proto::op::{
    DnsRequest, DnsRequestOptions, DnsResponse, Message, MessageType, Query, ResponseCode,
};
use hickory_proto::rr::{Name, Record, RecordType};
use hickory_resolver::config::{NameServerConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::net::xfer::FirstAnswer;
use hickory_resolver::net::{DnsError, DnsHandle, NetError};
use hickory_resolver::{NameServerPool, PoolContext, TlsConfig};

use crate::observer::DnssecStatus;
use crate::upstream::{Transport, Upstream};

pub struct UpstreamClient {
    pool: NameServerPool<TokioRuntimeProvider>,
    /// When set, each query carries the EDNS DO bit so the upstream resolver
    /// validates DNSSEC and reports its verdict in the response's AD flag, and
    /// an upstream SERVFAIL is probed with a checking-disabled retry to tell a
    /// signature failure (Bogus) apart from a dead upstream. JustDNS does not
    /// re-walk the chain itself — that is unreliable over a forwarder and was
    /// the source of the false-Bogus bug — it trusts the validating upstream.
    /// Shared so the binary can flip DNSSEC validation live.
    validate: Arc<AtomicBool>,
    /// A short label identifying this resolver (its certificate name, or plain
    /// IP), computed once so each answer can be attributed to it in the query
    /// log without re-deriving it per query.
    label: Arc<str>,
    /// Whether this upstream is currently considered reachable. Flips to `false`
    /// after [`UNHEALTHY_AFTER`] consecutive failures and back to `true` on the
    /// next success; each flip logs once, so the diagnostic log shows an upstream
    /// going down or recovering without a line per query. Lives as long as the
    /// client (a router rebuild starts a fresh, healthy client).
    healthy: AtomicBool,
    /// Consecutive failure count feeding the health flip above.
    failures: AtomicUsize,
}

/// How many consecutive failures mark an upstream unhealthy. Small, so a real
/// outage is noticed quickly, but above 1 so a single dropped packet does not
/// flap the state.
const UNHEALTHY_AFTER: usize = 3;

impl UpstreamClient {
    pub fn new(upstream: &Upstream, validate: Arc<AtomicBool>) -> Self {
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
            validate,
            label: Arc::from(upstream.label()),
            healthy: AtomicBool::new(true),
            failures: AtomicUsize::new(0),
        }
    }

    /// The label identifying this resolver, attached to each answer it gives so
    /// the query log can attribute it.
    pub(crate) fn label(&self) -> Arc<str> {
        self.label.clone()
    }

    /// Note that the upstream answered: reset the failure streak and, if it was
    /// marked unhealthy, log the recovery once.
    fn note_reachable(&self) {
        self.failures.store(0, Ordering::Relaxed);
        if !self.healthy.swap(true, Ordering::Relaxed) {
            tracing::info!(upstream = %self.label, "upstream recovered");
        }
    }

    /// Note that the upstream failed to answer: count the failure and, when the
    /// streak crosses the threshold and the upstream was still considered up, log
    /// the transition once.
    fn note_failed(&self, error: &str) {
        let streak = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if streak >= UNHEALTHY_AFTER && self.healthy.swap(false, Ordering::Relaxed) {
            tracing::warn!(
                upstream = %self.label,
                consecutive = streak,
                error,
                "upstream not responding; marked unhealthy"
            );
        }
    }

    /// Forward a single query and return the upstream's raw response (TTLs and
    /// response code intact, for the cache and the response path to use)
    /// together with its DNSSEC verdict.
    ///
    /// The verdict is `None` unless validation is on. When it is, JustDNS leans
    /// on the upstream resolver rather than validating itself:
    ///
    /// * The EDNS DO bit asks the upstream to validate; a NOERROR answer with
    ///   the **AD** flag set is `Secure`, one without it comes from a provably
    ///   unsigned zone and is `Insecure`.
    /// * A validating upstream withholds a signature failure as SERVFAIL. To
    ///   tell that apart from an ordinary upstream failure we retry once with
    ///   **CD** (checking disabled): if the data is there with checking off, the
    ///   SERVFAIL was a validation failure — `Bogus`, refused upstream in the
    ///   pipeline; if it still fails, it is a genuine upstream error.
    pub async fn query(
        &self,
        name: Name,
        record_type: RecordType,
    ) -> Result<(DnsResponse, Option<DnssecStatus>), ProtoError> {
        let validating = self.validate.load(Ordering::Relaxed);

        let mut options = DnsRequestOptions::default();
        options.recursion_desired = true;
        // Set the DO bit only when validating, so the upstream actually checks
        // the signatures and reports the verdict in the AD flag. We trust that
        // verdict instead of re-walking the DNSKEY/DS/RRSIG chain ourselves —
        // which is unreliable over a forwarder — so a provably unsigned domain
        // comes back as a normal (AD-less) answer rather than a false Bogus.
        options.edns_set_dnssec_ok = validating;

        let query = Query::query(name.clone(), record_type);
        let error = match self.pool.lookup(query, options).first_answer().await {
            Ok(response) => {
                // AD set by the validating upstream ⇒ Secure; a plain answer
                // without AD ⇒ the zone is unsigned (Insecure).
                let verdict = validating.then(|| {
                    if response.authentic_data {
                        DnssecStatus::Secure
                    } else {
                        DnssecStatus::Insecure
                    }
                });
                self.note_reachable();
                return Ok((response, verdict));
            }
            Err(error) => error,
        };

        // A validating upstream refuses a signature failure with SERVFAIL. Retry
        // the same query once with checking disabled: if an answer (or a real
        // NXDOMAIN/NODATA) comes back with validation off, the SERVFAIL was a
        // DNSSEC failure, i.e. Bogus. The pipeline turns a Bogus verdict into
        // SERVFAIL and never caches it, so the client still refuses the forged
        // answer — but now the query log records why.
        if validating {
            if let NetError::Dns(DnsError::ResponseCode(ResponseCode::ServFail)) = &error {
                let mut retry = DnsRequestOptions::default();
                retry.recursion_desired = true;
                let mut request =
                    DnsRequest::from_query(Query::query(name.clone(), record_type), retry);
                request.metadata.checking_disabled = true;

                let bogus = match self.pool.send(request).first_answer().await {
                    Ok(_) => true,
                    Err(NetError::Dns(DnsError::NoRecordsFound(no_records))) => matches!(
                        no_records.response_code,
                        ResponseCode::NXDomain | ResponseCode::NoError
                    ),
                    Err(_) => false,
                };
                if bogus {
                    // The upstream answered — it just refused a forged record —
                    // so it counts as reachable. The verdict is security-relevant
                    // and rare, so warn once here (not per CD retry).
                    self.note_reachable();
                    tracing::warn!(
                        name = %name,
                        record = %record_type,
                        upstream = %self.label,
                        "DNSSEC validation failed (bogus); answer refused"
                    );
                    return servfail_response(name, record_type)
                        .map(|response| (response, Some(DnssecStatus::Bogus)));
                }
            }
        }

        // hickory reports an empty answer — NXDOMAIN or NODATA — as a
        // `NoRecordsFound` error rather than a response. Rebuild it into a real
        // negative response so the pipeline relays the true response code and
        // can cache it, instead of turning every "no such name / no such record"
        // into SERVFAIL. Genuine failures (SERVFAIL, REFUSED, transport errors)
        // reach here too but keep their error, so racing still prefers a working
        // upstream over a failing one. A negative answer carries no DNSSEC
        // verdict: the AD flag is unavailable on this error path.
        if let NetError::Dns(DnsError::NoRecordsFound(no_records)) = &error {
            if matches!(
                no_records.response_code,
                ResponseCode::NXDomain | ResponseCode::NoError
            ) {
                self.note_reachable();
                return negative_response(
                    name,
                    record_type,
                    no_records.response_code,
                    &no_records.authorities,
                )
                .map(|response| (response, None));
            }
        }

        // Upstream transport failures are hickory-net's own error type; fold
        // them into the crate's `ProtoError` boundary so the `Resolve` trait
        // stays transport-agnostic. The pipeline only ever maps an error to
        // SERVFAIL, so the message text is all that need survive the conversion.
        let message = error.to_string();
        self.note_failed(&message);
        Err(ProtoError::from(message))
    }
}

/// Build a synthetic SERVFAIL response (no answers) for a query whose upstream
/// answer was refused as Bogus, so the response carries the right code while the
/// pipeline records the DNSSEC verdict.
fn servfail_response(name: Name, record_type: RecordType) -> Result<DnsResponse, ProtoError> {
    let mut message = Message::query();
    message.metadata.message_type = MessageType::Response;
    message.metadata.response_code = ResponseCode::ServFail;
    message.add_query(Query::query(name, record_type));
    DnsResponse::from_message(message)
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
        // Validation off: these mocks speak plain DNS without RRSIGs, so they
        // exercise the unvalidated path (the DNSSEC path is covered live).
        UpstreamClient::new(&upstream, Arc::new(AtomicBool::new(false)))
    }

    #[test]
    fn health_flips_after_repeated_failures_and_recovers() {
        let upstream = Upstream::plain(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)));
        let client = UpstreamClient::new(&upstream, Arc::new(AtomicBool::new(false)));
        assert!(client.healthy.load(Ordering::Relaxed), "starts healthy");

        // Below the threshold the upstream is still considered up.
        client.note_failed("boom");
        client.note_failed("boom");
        assert!(client.healthy.load(Ordering::Relaxed));

        // The threshold-th consecutive failure flips it to unhealthy.
        client.note_failed("boom");
        assert!(!client.healthy.load(Ordering::Relaxed));

        // A single success both recovers it and clears the streak.
        client.note_reachable();
        assert!(client.healthy.load(Ordering::Relaxed));
        assert_eq!(client.failures.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn forwards_query_and_returns_answer() {
        let client = client_for(spawn_mock_upstream().await).await;

        let (response, verdict) = client
            .query(Name::from_ascii("example.com.").unwrap(), RecordType::A)
            .await
            .expect("query failed");

        assert_eq!(response.response_code, ResponseCode::NoError);
        assert_eq!(response.answers[0].data, RData::A(A(ANSWER)));
        assert_eq!(verdict, None, "validation is off, so there is no verdict");
    }

    #[tokio::test]
    async fn nxdomain_is_a_response_not_an_error() {
        let client = client_for(spawn_mock_negative(ResponseCode::NXDomain, true).await).await;

        let (response, _verdict) = client
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
        let (response, _verdict) = client
            .query(Name::from_ascii("example.com.").unwrap(), RecordType::HTTPS)
            .await
            .expect("NODATA must come back as a response, not an error");

        assert_eq!(response.response_code, ResponseCode::NoError);
        assert!(response.answers.is_empty());
    }
}
