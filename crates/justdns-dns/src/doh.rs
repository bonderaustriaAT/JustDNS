// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The DNS-over-HTTPS listener (RFC 8484).
//!
//! JustDNS terminates TLS itself and answers `GET`/`POST /dns-query` with the
//! `application/dns-message` wire format, running each request through the same
//! [`Pipeline`] as the plain and DoT transports. The client IP is taken from the
//! TLS connection's peer address — JustDNS is the terminating server, so there
//! is no proxy header to trust — which keeps per-device and per-group policies
//! working for encrypted clients.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use hickory_proto::op::Message;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use std::io;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, warn};

use crate::pipeline::Pipeline;
use crate::router::Resolve;
use crate::tls::TlsManager;

/// The media type for both the request body and the response (RFC 8484).
const DNS_MESSAGE: &str = "application/dns-message";
/// The only path JustDNS answers; anything else is a 404.
const DNS_QUERY_PATH: &str = "/dns-query";
/// A DoH request body larger than this is refused — DNS messages are small.
const MAX_BODY: usize = 65_535;

/// Serve DNS-over-HTTPS on `addr`. The TLS config is read per connection so a
/// certificate change takes effect for new connections without rebinding.
pub async fn serve_doh<R>(
    addr: SocketAddr,
    pipeline: Arc<Pipeline<R>>,
    tls: Arc<TlsManager>,
) -> io::Result<()>
where
    R: Resolve + Send + Sync + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    let builder = Arc::new(auto::Builder::new(TokioExecutor::new()));
    loop {
        let (stream, peer) = listener.accept().await?;
        let acceptor = TlsAcceptor::from(tls.doh_config());
        let pipeline = pipeline.clone();
        let builder = builder.clone();
        tokio::spawn(async move {
            let stream = match acceptor.accept(stream).await {
                Ok(stream) => stream,
                Err(error) => {
                    warn!(%peer, %error, "DoH TLS handshake failed");
                    return;
                }
            };
            let client = peer.ip();
            let service = service_fn(move |req| {
                let pipeline = pipeline.clone();
                async move { Ok::<_, Infallible>(handle(req, &pipeline, client).await) }
            });
            if let Err(error) = builder
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                // A client that hangs up mid-request is routine, not a fault.
                debug!(%peer, %error, "DoH connection ended");
            }
        });
    }
}

/// Answer a single DoH request. Always returns a response; protocol problems map
/// to the matching HTTP status rather than dropping the connection.
async fn handle<R>(
    req: Request<Incoming>,
    pipeline: &Pipeline<R>,
    client: IpAddr,
) -> Response<Full<Bytes>>
where
    R: Resolve + Send + Sync + 'static,
{
    if req.uri().path() != DNS_QUERY_PATH {
        return status(StatusCode::NOT_FOUND);
    }

    let query = match *req.method() {
        Method::GET => match dns_param(req.uri().query()) {
            Some(bytes) => bytes,
            None => return status(StatusCode::BAD_REQUEST),
        },
        Method::POST => match collect_body(req.into_body()).await {
            Some(bytes) => bytes,
            None => return status(StatusCode::BAD_REQUEST),
        },
        _ => return status(StatusCode::METHOD_NOT_ALLOWED),
    };

    let request = match Message::from_vec(&query) {
        Ok(request) => request,
        Err(_) => return status(StatusCode::BAD_REQUEST),
    };

    let response = pipeline.handle(&request, client).await;
    let max_age = min_ttl(&response);
    let Ok(encoded) = response.to_vec() else {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, DNS_MESSAGE)
        .header(CACHE_CONTROL, format!("max-age={max_age}"))
        .body(Full::new(Bytes::from(encoded)))
        .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Decode the `dns` query parameter (base64url, no padding) from a GET request.
fn dns_param(query: Option<&str>) -> Option<Vec<u8>> {
    let encoded = query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("dns="))?;
    URL_SAFE_NO_PAD.decode(encoded).ok()
}

/// Read a POST body up to [`MAX_BODY`] bytes, rejecting anything larger.
async fn collect_body(body: Incoming) -> Option<Vec<u8>> {
    let collected = body.collect().await.ok()?.to_bytes();
    if collected.len() > MAX_BODY {
        return None;
    }
    Some(collected.to_vec())
}

/// The smallest record TTL in the response, used as the HTTP `max-age`. An
/// answer with no records (NXDOMAIN, errors) caches for zero seconds.
fn min_ttl(response: &Message) -> u32 {
    response
        .answers
        .iter()
        .chain(response.authorities.iter())
        .chain(response.additionals.iter())
        .map(|record| record.ttl)
        .min()
        .unwrap_or(0)
}

/// An empty-bodied response with just a status code.
fn status(code: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(code)
        .body(Full::new(Bytes::new()))
        .expect("static status response builds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64url_dns_param() {
        // "example" encoded base64url without padding.
        let encoded = URL_SAFE_NO_PAD.encode(b"example");
        let query = format!("ct=application/dns-message&dns={encoded}");
        assert_eq!(dns_param(Some(&query)), Some(b"example".to_vec()));
    }

    #[test]
    fn missing_dns_param_is_none() {
        assert_eq!(dns_param(Some("ct=application/dns-message")), None);
        assert_eq!(dns_param(None), None);
    }

    #[test]
    fn rejects_non_base64url_param() {
        assert_eq!(dns_param(Some("dns=!!not base64!!")), None);
    }
}
