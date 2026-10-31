// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The UDP and TCP listeners on port 53.
//!
//! Both transports decode the wire format into a [`Message`], run it through
//! the [`Pipeline`] and encode the reply back. UDP replies that outgrow the
//! client's advertised buffer are returned truncated so the client retries
//! over TCP, as DNS requires.

use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::sync::Arc;

use hickory_proto::op::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tracing::warn;

use crate::pipeline::Pipeline;
use crate::router::Resolve;

/// A UDP datagram large enough for an EDNS-advertised buffer.
const UDP_RECV_BUFFER: usize = 4096;

pub struct Server<R> {
    pipeline: Arc<Pipeline<R>>,
}

impl<R> Server<R>
where
    R: Resolve + Send + Sync + 'static,
{
    pub fn new(pipeline: Pipeline<R>) -> Self {
        Self {
            pipeline: Arc::new(pipeline),
        }
    }

    /// Bind UDP and TCP on `addr` and serve until one listener fails.
    pub async fn run(self, addr: SocketAddr) -> io::Result<()> {
        let udp = Arc::new(UdpSocket::bind(addr).await?);
        let tcp = TcpListener::bind(addr).await?;

        tokio::select! {
            result = serve_udp(udp, self.pipeline.clone()) => result,
            result = serve_tcp(tcp, self.pipeline.clone()) => result,
        }
    }
}

async fn serve_udp<R>(socket: Arc<UdpSocket>, pipeline: Arc<Pipeline<R>>) -> io::Result<()>
where
    R: Resolve + Send + Sync + 'static,
{
    loop {
        let mut buffer = [0u8; UDP_RECV_BUFFER];
        let (len, peer) = socket.recv_from(&mut buffer).await?;
        let request = buffer[..len].to_vec();
        let socket = socket.clone();
        let pipeline = pipeline.clone();
        tokio::spawn(async move {
            if let Some(reply) = handle_datagram(&pipeline, &request).await {
                if let Err(error) = socket.send_to(&reply, peer).await {
                    warn!(%peer, %error, "failed to send UDP reply");
                }
            }
        });
    }
}

async fn serve_tcp<R>(listener: TcpListener, pipeline: Arc<Pipeline<R>>) -> io::Result<()>
where
    R: Resolve + Send + Sync + 'static,
{
    loop {
        let (stream, peer) = listener.accept().await?;
        let pipeline = pipeline.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_stream(stream, &pipeline).await {
                warn!(%peer, %error, "TCP connection ended on error");
            }
        });
    }
}

/// Decode one datagram, answer it, and encode a reply sized for the client's
/// buffer. A reply that does not fit comes back truncated. Returns `None` when
/// the request cannot be decoded, since there is no id to answer with.
async fn handle_datagram<R>(pipeline: &Pipeline<R>, request: &[u8]) -> Option<Vec<u8>>
where
    R: Resolve + Sync,
{
    let request = match Message::from_vec(request) {
        Ok(request) => request,
        Err(error) => {
            warn!(%error, "dropping undecodable UDP request");
            return None;
        }
    };

    let max_payload = usize::from(request.max_payload());
    let response = pipeline.handle(&request).await;

    let encoded = response.to_vec().ok()?;
    if encoded.len() <= max_payload {
        return Some(encoded);
    }

    truncated(response).to_vec().ok()
}

/// A DNS-over-TCP connection: each message is framed by a two-byte length.
async fn handle_stream<R>(mut stream: TcpStream, pipeline: &Pipeline<R>) -> io::Result<()>
where
    R: Resolve + Sync,
{
    loop {
        let mut length = [0u8; 2];
        match stream.read_exact(&mut length).await {
            Ok(_) => {}
            // A clean close between messages is the normal end of a connection.
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }

        let mut request = vec![0u8; usize::from(u16::from_be_bytes(length))];
        stream.read_exact(&mut request).await?;

        let request = match Message::from_vec(&request) {
            Ok(request) => request,
            Err(error) => {
                warn!(%error, "closing TCP connection after undecodable request");
                return Ok(());
            }
        };

        let response = pipeline.handle(&request).await;
        let Ok(encoded) = response.to_vec() else {
            return Ok(());
        };
        let length = u16::try_from(encoded.len()).unwrap_or(u16::MAX);
        stream.write_all(&length.to_be_bytes()).await?;
        stream.write_all(&encoded).await?;
        stream.flush().await?;
    }
}

/// Strip a response down to its header and question with the TC bit set, so a
/// client retries the query over TCP.
fn truncated(mut response: Message) -> Message {
    response.set_truncated(true);
    response.take_answers();
    response.take_name_servers();
    response.take_additionals();
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::Ipv4Addr;

    use hickory_proto::ProtoError;
    use hickory_proto::op::{MessageType, Query, ResponseCode};
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{Name, RData, Record, RecordType};
    use hickory_proto::xfer::DnsResponse;

    use crate::cache::DnsCache;
    use crate::router::Router;
    use crate::upstream::UpstreamMode;

    struct Mock {
        answers: usize,
    }

    impl Resolve for Mock {
        async fn resolve(
            &self,
            name: Name,
            _record_type: RecordType,
        ) -> Result<DnsResponse, ProtoError> {
            let mut message = Message::new();
            message.set_message_type(MessageType::Response);
            message.set_response_code(ResponseCode::NoError);
            for index in 0..self.answers {
                message.add_answer(Record::from_rdata(
                    name.clone(),
                    120,
                    RData::A(A(Ipv4Addr::new(10, 0, 0, index as u8))),
                ));
            }
            DnsResponse::from_message(message)
        }
    }

    fn pipeline(answers: usize) -> Pipeline<Mock> {
        let router = Router::new(UpstreamMode::Racing, vec![Mock { answers }]);
        Pipeline::new(DnsCache::new(100), router, Default::default())
    }

    fn request_bytes(record_type: RecordType) -> Vec<u8> {
        let mut message = Message::new();
        message.set_id(1234);
        message.set_recursion_desired(true);
        message.add_query(Query::query(
            Name::from_ascii("example.com.").unwrap(),
            record_type,
        ));
        message.to_vec().unwrap()
    }

    #[tokio::test]
    async fn datagram_round_trip_preserves_id() {
        let pipe = pipeline(1);
        let reply = handle_datagram(&pipe, &request_bytes(RecordType::A))
            .await
            .expect("a reply");
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.id(), 1234);
        assert_eq!(decoded.answers().len(), 1);
        assert!(!decoded.truncated());
    }

    #[tokio::test]
    async fn oversized_datagram_is_truncated() {
        // Many answers overflow the 512-byte default UDP buffer.
        let pipe = pipeline(60);
        let reply = handle_datagram(&pipe, &request_bytes(RecordType::A))
            .await
            .expect("a reply");
        assert!(reply.len() <= 512);
        let decoded = Message::from_vec(&reply).unwrap();
        assert!(decoded.truncated());
        assert!(decoded.answers().is_empty());
        assert_eq!(decoded.id(), 1234);
    }

    #[tokio::test]
    async fn undecodable_datagram_is_dropped() {
        let pipe = pipeline(1);
        assert!(handle_datagram(&pipe, &[0xff, 0x00, 0x13]).await.is_none());
    }
}
