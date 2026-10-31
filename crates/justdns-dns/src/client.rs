// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Network client that forwards a query to a single upstream.
//!
//! Each client wraps a hickory name-server pool configured for one upstream.
//! For a plain upstream the pool holds both a UDP and a TCP name server, so a
//! truncated UDP answer is retried over TCP. Selecting between several
//! upstreams (racing, fallback, rule-based) is handled a layer up; this client
//! only speaks to the one it was built for.

use hickory_proto::ProtoError;
use hickory_proto::op::Query;
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::xfer::{DnsHandle, DnsRequestOptions, DnsResponse, FirstAnswer};
use hickory_resolver::config::{NameServerConfigGroup, ResolverOpts};
use hickory_resolver::name_server::{NameServerPool, TokioConnectionProvider};

use crate::upstream::{Transport, Upstream};

pub struct UpstreamClient {
    pool: NameServerPool<TokioConnectionProvider>,
}

impl UpstreamClient {
    pub fn new(upstream: &Upstream) -> Self {
        // `trust_negative_responses` only matters when a pool spreads a query
        // across several servers; each client holds one upstream, so it has no
        // effect here and we leave it at the group defaults.
        let ips = [upstream.address];
        let servers = match &upstream.transport {
            Transport::Plain => NameServerConfigGroup::from_ips_clear(&ips, upstream.port, true),
            Transport::Dot { server_name } => {
                NameServerConfigGroup::from_ips_tls(&ips, upstream.port, server_name.clone(), true)
            }
            Transport::Doh { server_name } => NameServerConfigGroup::from_ips_https(
                &ips,
                upstream.port,
                server_name.clone(),
                true,
            ),
        };

        let pool = NameServerPool::from_config(
            servers,
            ResolverOpts::default(),
            TokioConnectionProvider::default(),
        );

        Self { pool }
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

        self.pool
            .lookup(Query::query(name, record_type), options)
            .first_answer()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use hickory_proto::op::{Message, MessageType, ResponseCode};
    use hickory_proto::rr::rdata::A;
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

            let mut response = Message::new();
            response.set_id(request.id());
            response.set_message_type(MessageType::Response);
            response.set_response_code(ResponseCode::NoError);
            response.set_recursion_desired(true);
            response.set_recursion_available(true);
            if let Some(query) = request.queries().first() {
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

    #[tokio::test]
    async fn forwards_query_and_returns_answer() {
        let addr = spawn_mock_upstream().await;

        let mut upstream = Upstream::plain(IpAddr::V4(Ipv4Addr::LOCALHOST));
        upstream.port = addr.port();
        let client = UpstreamClient::new(&upstream);

        let response = client
            .query(Name::from_ascii("example.com.").unwrap(), RecordType::A)
            .await
            .expect("query failed");

        assert_eq!(response.response_code(), ResponseCode::NoError);
        assert_eq!(response.answers()[0].data().as_a(), Some(&A(ANSWER)));
    }
}
