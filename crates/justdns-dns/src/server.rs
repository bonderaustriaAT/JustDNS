// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The UDP and TCP listeners on port 53.
//!
//! Both transports decode the wire format into a [`Message`], run it through
//! the [`Pipeline`] and encode the reply back. UDP replies that outgrow the
//! client's advertised buffer are returned truncated so the client retries
//! over TCP, as DNS requires.

use std::io::{self, ErrorKind};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use hickory_proto::op::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio_rustls::TlsAcceptor;
use tracing::warn;

use crate::pipeline::Pipeline;
use crate::router::Resolve;
use crate::tls::TlsManager;

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

    /// A shared handle to the pipeline, so the encrypted-DNS listeners (DoT/DoH)
    /// can run the same filtering and caching as the plain port-53 server.
    pub fn pipeline(&self) -> Arc<Pipeline<R>> {
        self.pipeline.clone()
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
            if let Some(reply) = handle_datagram(&pipeline, &request, peer.ip()).await {
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
            if let Err(error) = handle_stream(stream, &pipeline, peer.ip()).await {
                warn!(%peer, %error, "TCP connection ended on error");
            }
        });
    }
}

/// Decode one datagram, answer it, and encode a reply sized for the client's
/// buffer. A reply that does not fit comes back truncated. Returns `None` when
/// the request cannot be decoded, since there is no id to answer with.
async fn handle_datagram<R>(
    pipeline: &Pipeline<R>,
    request: &[u8],
    client: IpAddr,
) -> Option<Vec<u8>>
where
    R: Resolve + Send + Sync + 'static,
{
    let request = match Message::from_vec(request) {
        Ok(request) => request,
        Err(error) => {
            warn!(%error, "dropping undecodable UDP request");
            return None;
        }
    };

    let max_payload = usize::from(request.max_payload());
    let response = pipeline.handle(&request, client).await;

    let encoded = response.to_vec().ok()?;
    if encoded.len() <= max_payload {
        return Some(encoded);
    }

    truncated(response).to_vec().ok()
}

/// Serve DNS-over-TLS (RFC 7858) on `addr`: a TLS-wrapped, length-framed TCP
/// stream. The same wire handling as plain TCP runs over the decrypted stream,
/// and the peer address is the real client IP, so per-device policies apply
/// unchanged. The server config is read per connection, so a certificate change
/// takes effect for new connections without rebinding.
pub async fn serve_dot<R>(
    addr: SocketAddr,
    pipeline: Arc<Pipeline<R>>,
    tls: Arc<TlsManager>,
) -> io::Result<()>
where
    R: Resolve + Send + Sync + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, peer) = listener.accept().await?;
        let pipeline = pipeline.clone();
        let acceptor = TlsAcceptor::from(tls.dot_config());
        tokio::spawn(async move {
            let stream = match acceptor.accept(stream).await {
                Ok(stream) => stream,
                Err(error) => {
                    warn!(%peer, %error, "DoT TLS handshake failed");
                    return;
                }
            };
            if let Err(error) = handle_stream(stream, &pipeline, peer.ip()).await {
                warn!(%peer, %error, "DoT connection ended on error");
            }
        });
    }
}

/// A DNS-over-TCP connection: each message is framed by a two-byte length. The
/// stream is generic so the same handler serves plain TCP and the decrypted DoT
/// stream.
async fn handle_stream<R, S>(
    mut stream: S,
    pipeline: &Pipeline<R>,
    client: IpAddr,
) -> io::Result<()>
where
    R: Resolve + Send + Sync + 'static,
    S: AsyncRead + AsyncWrite + Unpin,
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

        let response = pipeline.handle(&request, client).await;
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

    const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

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

    fn pipeline(answers: usize) -> Pipeline<Router<Mock>> {
        let router = Arc::new(Router::new(UpstreamMode::Racing, vec![Mock { answers }]));
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
        let reply = handle_datagram(&pipe, &request_bytes(RecordType::A), CLIENT)
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
        let reply = handle_datagram(&pipe, &request_bytes(RecordType::A), CLIENT)
            .await
            .expect("a reply");
        assert!(reply.len() <= 512);
        let decoded = Message::from_vec(&reply).unwrap();
        assert!(decoded.truncated());
        assert!(decoded.answers().is_empty());
        assert_eq!(decoded.id(), 1234);
    }

    #[tokio::test]
    async fn stream_round_trip_is_length_framed() {
        // The generic handler must serve a length-framed exchange over any
        // async stream (here an in-memory duplex standing in for a TLS stream).
        let pipe = pipeline(1);
        let (mut client, server) = tokio::io::duplex(4096);

        let handler = tokio::spawn(async move { handle_stream(server, &pipe, CLIENT).await });

        let request = request_bytes(RecordType::A);
        let length = u16::try_from(request.len()).unwrap().to_be_bytes();
        client.write_all(&length).await.unwrap();
        client.write_all(&request).await.unwrap();
        client.flush().await.unwrap();

        let mut reply_length = [0u8; 2];
        client.read_exact(&mut reply_length).await.unwrap();
        let mut reply = vec![0u8; usize::from(u16::from_be_bytes(reply_length))];
        client.read_exact(&mut reply).await.unwrap();

        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.id(), 1234);
        assert_eq!(decoded.answers().len(), 1);

        // Closing the client ends the connection cleanly.
        drop(client);
        handler.await.unwrap().expect("clean close");
    }

    #[tokio::test]
    async fn undecodable_datagram_is_dropped() {
        let pipe = pipeline(1);
        assert!(
            handle_datagram(&pipe, &[0xff, 0x00, 0x13], CLIENT)
                .await
                .is_none()
        );
    }
}
