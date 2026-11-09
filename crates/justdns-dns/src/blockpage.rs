// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The custom block page web server.
//!
//! When the block page is on, a blocked name resolves to this server's IP (see
//! the pipeline's block-page branch) so the browser connects here instead of to
//! a dead address. The server then does one of two things, depending on how it
//! is configured:
//!
//! * **Local mode** (no landing URL): every request — any host, any path — is
//!   answered in place with the self-contained explanation page, on plain HTTP
//!   (port 80) and on HTTPS (port 443). Over HTTPS the browser asked for the
//!   blocked domain, so the served certificate cannot match and a warning is
//!   shown first; that is unavoidable without a man-in-the-middle CA.
//!
//! * **Hosted mode** (a landing URL like `https://justdns.justfuture.at/blockpage`
//!   is set): a request that actually reached a *blocked* domain is answered with
//!   a `302` to the landing URL, carrying the blocked host as `?d=`. A request
//!   whose `Host` is the landing host itself is served the rendered page directly.
//!   Because the operator runs JustDNS on that hostname (via a DNS rewrite) and
//!   selects a certificate that is valid for it, the landing page loads over
//!   HTTPS with **no warning** and can name the domain the visitor tried to reach.
//!
//! * **Behind a reverse proxy** (hosted mode with `trust_forwarded` on): the same
//!   `302`-then-serve flow, but the operator's own proxy (nginx, Caddy, Nginx
//!   Proxy Manager, …) terminates TLS for the landing hostname and forwards here
//!   over plain HTTP. The proxy rewrites `Host`, so the landing-host match reads
//!   the visitor's original host from `X-Forwarded-Host`. JustDNS manages no
//!   certificate of its own in this mode — the proxy presents the valid one — so
//!   the setup needs only the landing URL and this node's address.
//!
//! The `Host` is also read from the HTTP/2 `:authority` pseudo-header, not just a
//! `Host` request header, so the landing-host match works whether a browser speaks
//! HTTP/1.1 or HTTP/2 (an HTTP/2 client sends no `Host` header at all).
//!
//! The served certificate and the page content are held behind [`ArcSwap`]s so
//! the binary's supervisor can swap them live when the settings or the selected
//! certificate change, without rebinding the listeners.

use std::convert::Infallible;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use arc_swap::ArcSwap;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::header::{CONTENT_TYPE, HOST, HeaderMap, HeaderValue, LOCATION};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rustls::ServerConfig;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::debug;

/// The most a `POST /request-access` body may be, in bytes. The form carries only
/// a domain and a short reason, so anything larger is refused unread.
const MAX_REQUEST_BODY: usize = 4096;

/// A sink the block page calls when a visitor asks to have a blocked domain
/// released. The binary implements it: it works out which device/group the client
/// is, confirms the domain really is blocked for that client, looks up *why*, and
/// records the request for the dashboard queue.
///
/// Returns whether a request was actually recorded — `false` when the domain is
/// not blocked for that client (so the page can say so instead of claiming
/// success). It must not block for long; recording is a quick file write.
pub trait AccessRequestSink: Send + Sync {
    /// Record an "unblock this" ask for `domain` from `client`, with an optional
    /// reason. Returns `true` if a request was recorded.
    fn request_access(&self, client: IpAddr, domain: &str, reason: Option<&str>) -> bool;
}

/// The access-request sink shared with the serving tasks, if one is wired. `None`
/// leaves the block page read-only (the request button reports it is unavailable).
pub type SharedSink = Option<Arc<dyn AccessRequestSink>>;

/// The operator-set text and routing for the block page.
#[derive(Clone, Debug)]
pub struct BlockContent {
    /// The heading.
    pub title: String,
    /// The explanatory line under the heading.
    pub message: String,
    /// Where the block page is hosted, when it lives at a real hostname.
    ///
    /// `Some` turns on *hosted mode*: a blocked domain that reached the server is
    /// redirected (`302`) to [`Landing::url`] with the blocked host appended as
    /// `?d=`, while a request to the landing host itself is served the rendered
    /// page. `None` is *local mode* — every request renders the page in place.
    pub landing: Option<Landing>,
    /// Trust the `X-Forwarded-Host` header to carry the host the visitor actually
    /// asked for. Set only when the block page sits **behind the operator's own
    /// reverse proxy** (which terminates TLS and forwards here over HTTP): the
    /// proxy rewrites `Host` to the upstream name, so the landing-host match has to
    /// read the original host from `X-Forwarded-Host` instead. Off by default, so a
    /// client can never spoof the header to steer the redirect when no proxy is in
    /// front.
    pub trust_forwarded: bool,
}

/// A parsed landing target: the full redirect URL plus the host that identifies
/// "this request is the landing page itself" rather than a blocked domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    /// The full URL browsers are redirected to, verbatim as configured, e.g.
    /// `https://justdns.justfuture.at/blockpage`.
    pub url: String,
    /// The lowercased host that serves the page (`justdns.justfuture.at`), with
    /// any port dropped — used to recognise requests to the landing page.
    pub host: String,
}

impl Landing {
    /// Parse a configured landing URL into the redirect target and its serving
    /// host. Returns `None` for an empty string. Accepts a URL with or without a
    /// scheme and tolerates a path, query or fragment; the host is lowercased and
    /// any userinfo or port is dropped for matching, while [`url`](Self::url)
    /// keeps the operator's text exactly so the redirect lands where they wrote.
    pub fn parse(raw: &str) -> Option<Self> {
        let url = raw.trim();
        if url.is_empty() {
            return None;
        }
        let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
        let authority_end = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        let authority = &after_scheme[..authority_end];
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host)
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
            .to_ascii_lowercase();
        if host.is_empty() {
            return None;
        }
        Some(Self {
            url: url.to_owned(),
            host,
        })
    }
}

/// A live, swappable block-page text, shared with the serving tasks.
pub type SharedContent = Arc<ArcSwap<BlockContent>>;
/// A live, swappable HTTPS server config, shared with the HTTPS serving task.
pub type SharedTls = Arc<ArcSwap<ServerConfig>>;

/// Serve the block page over plain HTTP on `addr`. Returns only on a fatal
/// listener error (a dropped connection is routine and ignored).
pub async fn serve_http(
    addr: SocketAddr,
    content: SharedContent,
    sink: SharedSink,
) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let builder = Arc::new(auto::Builder::new(TokioExecutor::new()));
    loop {
        let (stream, peer) = listener.accept().await?;
        let content = content.clone();
        let sink = sink.clone();
        let builder = builder.clone();
        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let content = content.clone();
                let sink = sink.clone();
                async move { Ok::<_, Infallible>(handle(req, &content, &sink, peer).await) }
            });
            if let Err(error) = builder
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                debug!(%peer, %error, "block-page HTTP connection ended");
            }
        });
    }
}

/// Serve the block page over HTTPS on `addr`, reading the TLS config per
/// connection so a certificate change takes effect without rebinding.
pub async fn serve_https(
    addr: SocketAddr,
    content: SharedContent,
    tls: SharedTls,
    sink: SharedSink,
) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let builder = Arc::new(auto::Builder::new(TokioExecutor::new()));
    loop {
        let (stream, peer) = listener.accept().await?;
        let acceptor = TlsAcceptor::from(tls.load_full());
        let content = content.clone();
        let sink = sink.clone();
        let builder = builder.clone();
        tokio::spawn(async move {
            let stream = match acceptor.accept(stream).await {
                Ok(stream) => stream,
                Err(error) => {
                    // A client aborting at the certificate warning is routine.
                    debug!(%peer, %error, "block-page TLS handshake failed");
                    return;
                }
            };
            let service = service_fn(move |req| {
                let content = content.clone();
                let sink = sink.clone();
                async move { Ok::<_, Infallible>(handle(req, &content, &sink, peer).await) }
            });
            if let Err(error) = builder
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                debug!(%peer, %error, "block-page HTTPS connection ended");
            }
        });
    }
}

/// Handle any request. A `POST /request-access` is the visitor asking to have a
/// blocked domain released (routed to the access sink); every other request
/// resolves the host it was sent to and the query string and dispatches through
/// [`build_response`].
async fn handle(
    req: Request<hyper::body::Incoming>,
    content: &SharedContent,
    sink: &SharedSink,
    peer: SocketAddr,
) -> Response<Full<Bytes>> {
    let content = content.load();
    if req.method() == Method::POST && req.uri().path() == "/request-access" {
        return handle_request_access(req, &content, sink, peer).await;
    }
    let host = effective_host(req.headers(), req.uri(), content.trust_forwarded);
    let query = req.uri().query().unwrap_or("").to_owned();
    build_response(&host, &query, &content)
}

/// Handle `POST /request-access`: read the small form body, work out the client
/// address (the forwarded one behind a trusted proxy, else the TCP peer), and ask
/// the sink to record the request. Replies with a tiny JSON object the page's
/// inline script reads: `{"ok":true}` when a request was recorded, `{"ok":false}`
/// when the domain is not actually blocked for this client or no sink is wired.
async fn handle_request_access(
    req: Request<hyper::body::Incoming>,
    content: &BlockContent,
    sink: &SharedSink,
    peer: SocketAddr,
) -> Response<Full<Bytes>> {
    let Some(sink) = sink else {
        // No sink wired (the block page is serving read-only); report unavailable.
        return json_ok(false);
    };
    let client = forwarded_client_ip(req.headers(), peer, content.trust_forwarded);
    let body = match Limited::new(req.into_body(), MAX_REQUEST_BODY)
        .collect()
        .await
    {
        Ok(collected) => collected.to_bytes(),
        // Too large or a dropped body: nothing to record.
        Err(_) => return json_ok(false),
    };
    let (domain, reason) = parse_access_form(&body);
    if domain.is_empty() {
        return json_ok(false);
    }
    let created = sink.request_access(client, &domain, reason.as_deref());
    json_ok(created)
}

/// The client address a request came from: the first `X-Forwarded-For` entry when
/// the block page trusts its reverse proxy (the proxy is the TCP peer, the visitor
/// is in the header), otherwise the TCP peer. An unparsable forwarded value falls
/// back to the peer, so a spoofed header can never erase the real source.
fn forwarded_client_ip(headers: &HeaderMap, peer: SocketAddr, trust_forwarded: bool) -> IpAddr {
    if trust_forwarded {
        if let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = forwarded.split(',').next() {
                if let Ok(ip) = first.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }
    peer.ip()
}

/// Pull `domain` and the optional `reason` out of an `application/x-www-form-
/// urlencoded` body. Unknown fields are ignored; the domain is required by the
/// caller. Values are form-decoded (`+` → space, then `%XX`).
fn parse_access_form(body: &[u8]) -> (String, Option<String>) {
    let text = String::from_utf8_lossy(body);
    let mut domain = String::new();
    let mut reason: Option<String> = None;
    for pair in text.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "domain" => domain = form_decode(value),
            "reason" => {
                let decoded = form_decode(value);
                if !decoded.trim().is_empty() {
                    reason = Some(decoded);
                }
            }
            _ => {}
        }
    }
    (domain, reason)
}

/// Decode one `application/x-www-form-urlencoded` value: `+` becomes a space, then
/// `%XX` escapes are decoded.
fn form_decode(value: &str) -> String {
    percent_decode(&value.replace('+', " "))
}

/// A tiny JSON reply for the request-access endpoint, carrying only `ok`.
fn json_ok(ok: bool) -> Response<Full<Bytes>> {
    let body = if ok {
        "{\"ok\":true}"
    } else {
        "{\"ok\":false}"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| error_response())
}

/// Resolve the host a request was aimed at, coping with HTTP/2 and a front proxy:
///
/// * When `trust_forwarded` is set (the block page is behind the operator's own
///   reverse proxy), the first `X-Forwarded-Host` value wins — the proxy puts the
///   visitor's original host there after rewriting `Host` to the upstream name.
/// * Otherwise the `Host` request header (HTTP/1.1), then the URI authority
///   (HTTP/2, where there is no `Host` header and the `:authority` pseudo-header
///   lands in the URI). Empty when nothing names a host.
fn effective_host(headers: &HeaderMap, uri: &Uri, trust_forwarded: bool) -> String {
    if trust_forwarded {
        if let Some(forwarded) = headers
            .get("x-forwarded-host")
            .and_then(|value| value.to_str().ok())
        {
            // A proxy chain may send a comma-separated list; the first is the
            // host the original visitor asked for.
            let first = forwarded.split(',').next().unwrap_or("").trim();
            if !first.is_empty() {
                return first.to_owned();
            }
        }
    }
    if let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) {
        if !host.is_empty() {
            return host.to_owned();
        }
    }
    uri.host().unwrap_or("").to_owned()
}

/// Decide what a request to `host` (with query string `query`) is answered with:
///
/// * Hosted mode, request **to the landing host** → render the page (`200`),
///   naming the blocked domain taken from `?d=` when present.
/// * Hosted mode, request **to any other host** (a blocked domain that was
///   pointed here) → `302` to the landing URL with the blocked host as `?d=`.
/// * Local mode (no landing) → render the page in place (`451`).
fn build_response(host: &str, query: &str, content: &BlockContent) -> Response<Full<Bytes>> {
    if let Some(landing) = &content.landing {
        if same_host(host, &landing.host) {
            // This IS the landing page, served on its own valid-certificate host.
            let domain = domain_from_query(query);
            return page_response(&domain, content, StatusCode::OK);
        }
        // A blocked domain reached us: send the browser on to the landing page,
        // carrying the domain it tried to reach so the page can name it.
        let location = redirect_location(&landing.url, host);
        if let Ok(value) = HeaderValue::from_str(&location) {
            return Response::builder()
                .status(StatusCode::FOUND)
                .header(LOCATION, value)
                .header("cache-control", "no-store")
                .body(Full::new(Bytes::new()))
                .unwrap_or_else(|_| error_response());
        }
        // An un-encodable location is not expected; fall back to the local page.
    }
    page_response(host, content, StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS)
}

/// Render the page for `domain` with the given status and the no-store headers.
fn page_response(
    domain: &str,
    content: &BlockContent,
    status: StatusCode,
) -> Response<Full<Bytes>> {
    let html = render_page(domain, &content.title, &content.message);
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(html)))
        .unwrap_or_else(|_| error_response())
}

/// A bare `500` used only if building a normal response somehow fails.
fn error_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Full::new(Bytes::new()))
        .expect("static status response builds")
}

/// Whether a request `Host` header names the landing host, ignoring any port and
/// ASCII case. `landing_host` is already lowercased and port-free.
fn same_host(host_header: &str, landing_host: &str) -> bool {
    let host = host_header
        .rsplit_once(':')
        .map_or(host_header, |(host, _)| host);
    host.eq_ignore_ascii_case(landing_host)
}

/// Append the blocked host to the landing URL as `?d=` (or `&d=` if the URL
/// already carries a query), percent-encoding it so the `Location` is always a
/// valid header value.
fn redirect_location(landing_url: &str, blocked_host: &str) -> String {
    let separator = if landing_url.contains('?') { '&' } else { '?' };
    format!("{landing_url}{separator}d={}", percent_encode(blocked_host))
}

/// Pull the `d` query parameter (the blocked domain) out of a query string and
/// percent-decode it for display. Empty when absent.
fn domain_from_query(query: &str) -> String {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("d="))
        .map(percent_decode)
        .unwrap_or_default()
}

/// Percent-encode everything outside the URL unreserved set, so a host can be
/// placed safely in a query string. Hostnames are unreserved already, so this is
/// usually a no-op; it only guards against an unusual blocked name.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Reverse [`percent_encode`]: decode `%XX` escapes, leaving everything else as
/// is. Invalid escapes are passed through verbatim.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render the self-contained block page. All dynamic values are HTML-escaped so
/// a crafted host or a configured title/message can never inject markup.
fn render_page(host: &str, title: &str, message: &str) -> String {
    let title = escape(title);
    let message = escape(message);
    let host = escape(host);
    let host_block = if host.is_empty() {
        String::new()
    } else {
        format!("<p class=\"host\">{host}</p>")
    };
    // The "ask to unblock" affordance only makes sense when a specific domain is
    // named; without one there is nothing to request. The domain rides in a
    // data-attribute (already HTML-escaped) and the inline script posts it back.
    let request_block = if host.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"rq\" id=\"rq\" data-domain=\"{host}\">\
<button type=\"button\" class=\"rqbtn\" id=\"rqOpen\">Seite freigeben lassen</button>\
<div class=\"rqpanel\" id=\"rqPanel\" hidden>\
<label class=\"rqlbl\" for=\"rqReason\">Begr\u{00fc}ndung (optional)</label>\
<textarea id=\"rqReason\" class=\"rqta\" rows=\"3\" maxlength=\"280\" \
placeholder=\"Warum brauchst du diese Seite?\"></textarea>\
<button type=\"button\" class=\"rqbtn rqsend\" id=\"rqSend\">Anfrage senden</button>\
<p class=\"rqmsg\" id=\"rqMsg\"></p></div>\
<p class=\"rqdone\" id=\"rqDone\" hidden>Anfrage gesendet — der Administrator entscheidet.</p>\
</div>"
        )
    };
    let script = if host.is_empty() {
        ""
    } else {
        "<script>(function(){var r=document.getElementById('rq');if(!r)return;\
var o=document.getElementById('rqOpen'),p=document.getElementById('rqPanel'),\
s=document.getElementById('rqSend'),t=document.getElementById('rqReason'),\
m=document.getElementById('rqMsg'),dn=document.getElementById('rqDone'),d=r.dataset.domain;\
o.addEventListener('click',function(){p.hidden=false;o.hidden=true;t.focus();});\
s.addEventListener('click',function(){s.disabled=true;m.textContent='';\
var b='domain='+encodeURIComponent(d)+'&reason='+encodeURIComponent(t.value||'');\
fetch('/request-access',{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:b})\
.then(function(x){return x.json().catch(function(){return{ok:false};});})\
.then(function(j){if(j&&j.ok){p.hidden=true;dn.hidden=false;}else{s.disabled=false;\
m.textContent='Nicht geblockt oder nicht sendbar.';}})\
.catch(function(){s.disabled=false;m.textContent='Konnte nicht gesendet werden.';});});})();</script>"
    };
    format!(
        "<!doctype html>\n<html lang=\"de\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title>\
<style>:root{{color-scheme:dark}}\
*{{box-sizing:border-box}}\
body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
padding:28px;font:16px/1.6 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;\
background:#0f1115;color:#e7ece9}}\
main{{max-width:33rem;text-align:center}}\
.dot{{width:13px;height:13px;border-radius:50%;background:#e0564f;display:inline-block;\
position:relative;margin-bottom:22px}}\
.dot::after{{content:\"\";position:absolute;inset:-7px;border-radius:50%;\
border:2px solid #e0564f;opacity:.5;animation:pq 2.4s ease-out infinite}}\
@keyframes pq{{0%{{transform:scale(.6);opacity:.6}}100%{{transform:scale(1.5);opacity:0}}}}\
@media(prefers-reduced-motion:reduce){{.dot::after{{animation:none;opacity:.35}}}}\
h1{{font-size:1.65rem;font-weight:600;margin:0 0 .55rem;letter-spacing:-.015em}}\
p{{margin:0;color:#aab2ad}}\
.host{{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.92rem;\
color:#e7ece9;background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.08);\
border-radius:9px;padding:9px 13px;display:inline-block;margin-top:20px;word-break:break-all}}\
.rq{{margin-top:26px}}\
.rqbtn{{appearance:none;border:1px solid rgba(255,255,255,.14);background:rgba(255,255,255,.04);\
color:#e7ece9;font:inherit;font-size:.9rem;padding:10px 18px;border-radius:9px;cursor:pointer;\
transition:background .15s,border-color .15s}}\
.rqbtn:hover{{background:rgba(255,255,255,.09);border-color:rgba(255,255,255,.22)}}\
.rqbtn:focus-visible{{outline:2px solid #37a97e;outline-offset:2px}}\
.rqpanel{{margin-top:16px;text-align:left;display:inline-block;max-width:24rem;width:100%}}\
.rqlbl{{display:block;font-size:.82rem;color:#aab2ad;margin-bottom:6px}}\
.rqta{{width:100%;box-sizing:border-box;resize:vertical;font:inherit;font-size:.9rem;\
color:#e7ece9;background:rgba(255,255,255,.05);border:1px solid rgba(255,255,255,.12);\
border-radius:9px;padding:9px 11px}}\
.rqta:focus-visible{{outline:2px solid #37a97e;outline-offset:1px}}\
.rqsend{{margin-top:11px;background:#37a97e;border-color:#37a97e;color:#07130d;font-weight:600}}\
.rqsend:hover{{background:#43c492;border-color:#43c492}}\
.rqsend:disabled{{opacity:.55;cursor:default}}\
.rqmsg{{margin-top:9px;font-size:.82rem;color:#e0a44f;min-height:1em}}\
.rqdone{{margin-top:18px;font-size:.92rem;color:#37a97e}}\
.brand{{margin-top:30px;font-size:.72rem;color:#6c756f;text-transform:uppercase;\
letter-spacing:.14em}}\
.brand b{{color:#37a97e;font-weight:700}}</style></head>\
<body><main><span class=\"dot\"></span><h1>{title}</h1><p>{message}</p>{host_block}\
{request_block}\
<p class=\"brand\">gesch\u{00fc}tzt durch <b>JustDNS</b></p></main>{script}</body></html>"
    )
}

/// Minimal HTML-escape for text placed into the page.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(landing: Option<Landing>) -> BlockContent {
        BlockContent {
            title: "Gesperrt".into(),
            message: "Blockiert.".into(),
            landing,
            trust_forwarded: false,
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                hyper::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn renders_host_and_text() {
        let page = render_page("ads.example.com", "Gesperrt", "Blockiert.");
        assert!(page.contains("<h1>Gesperrt</h1>"));
        assert!(page.contains("Blockiert."));
        assert!(page.contains("ads.example.com"));
        assert!(page.starts_with("<!doctype html>"));
    }

    #[test]
    fn escapes_injection_attempts() {
        let page = render_page("<script>evil()</script>", "a&b", "x<y");
        assert!(!page.contains("<script>evil"));
        assert!(page.contains("&lt;script&gt;"));
        assert!(page.contains("a&amp;b"));
        assert!(page.contains("x&lt;y"));
    }

    #[test]
    fn empty_host_omits_the_block() {
        let page = render_page("", "T", "M");
        assert!(!page.contains("class=\"host\""));
    }

    #[test]
    fn local_mode_serves_the_page() {
        let resp = build_response("ads.example.com", "", &content(None));
        assert_eq!(resp.status(), StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS);
        assert!(resp.headers().get(LOCATION).is_none());
    }

    #[test]
    fn hosted_mode_redirects_a_blocked_domain() {
        let landing = Landing::parse("https://justdns.justfuture.at/blockpage").unwrap();
        let resp = build_response("ads.example.com", "", &content(Some(landing)));
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert_eq!(
            resp.headers().get(LOCATION).unwrap(),
            "https://justdns.justfuture.at/blockpage?d=ads.example.com"
        );
    }

    #[test]
    fn hosted_mode_appends_with_ampersand_when_url_has_a_query() {
        let landing = Landing::parse("https://host.example/bp?src=dns").unwrap();
        let resp = build_response("ads.example.com", "", &content(Some(landing)));
        assert_eq!(
            resp.headers().get(LOCATION).unwrap(),
            "https://host.example/bp?src=dns&d=ads.example.com"
        );
    }

    #[test]
    fn hosted_mode_serves_the_page_on_the_landing_host() {
        let landing = Landing::parse("https://justdns.justfuture.at/blockpage").unwrap();
        // The browser, redirected here, asks the landing host with ?d=.
        let resp = build_response(
            "justdns.justfuture.at",
            "d=ads.example.com",
            &content(Some(landing)),
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(LOCATION).is_none());
    }

    #[test]
    fn landing_host_match_ignores_port_and_case() {
        let landing = Landing::parse("https://JustDNS.justfuture.at/blockpage").unwrap();
        assert_eq!(landing.host, "justdns.justfuture.at");
        let resp = build_response(
            "justdns.justfuture.at:443",
            "",
            &content(Some(landing.clone())),
        );
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn landing_parse_without_scheme_or_path() {
        let landing = Landing::parse("justdns.justfuture.at").unwrap();
        assert_eq!(landing.host, "justdns.justfuture.at");
        assert_eq!(landing.url, "justdns.justfuture.at");
        assert!(Landing::parse("   ").is_none());
    }

    #[test]
    fn domain_param_round_trips_through_encode_and_decode() {
        let location = redirect_location("https://host/bp", "a b.example.com");
        assert_eq!(location, "https://host/bp?d=a%20b.example.com");
        assert_eq!(domain_from_query("d=a%20b.example.com"), "a b.example.com");
        assert_eq!(domain_from_query("x=1&d=ads.net"), "ads.net");
        assert_eq!(domain_from_query(""), "");
    }

    #[test]
    fn host_comes_from_the_host_header_on_http1() {
        let h = headers(&[("host", "justdns.justfuture.at")]);
        let uri: Uri = "/blockpage?d=x".parse().unwrap();
        assert_eq!(effective_host(&h, &uri, false), "justdns.justfuture.at");
    }

    #[test]
    fn host_comes_from_the_authority_on_http2() {
        // An HTTP/2 request carries no Host header; the authority is in the URI.
        let h = HeaderMap::new();
        let uri: Uri = "https://justdns.justfuture.at/blockpage?d=x"
            .parse()
            .unwrap();
        assert_eq!(effective_host(&h, &uri, false), "justdns.justfuture.at");
    }

    #[test]
    fn forwarded_host_is_used_only_when_trusted() {
        // Behind the proxy the real host is in X-Forwarded-Host; Host is the upstream.
        let h = headers(&[
            ("host", "justdns.internal:8080"),
            ("x-forwarded-host", "blocked.justfuture.at"),
        ]);
        let uri: Uri = "/blockpage?d=x".parse().unwrap();
        assert_eq!(effective_host(&h, &uri, true), "blocked.justfuture.at");
        // Without trust, the forwarded header is ignored (a client could spoof it).
        assert_eq!(effective_host(&h, &uri, false), "justdns.internal:8080");
    }

    #[test]
    fn forwarded_host_takes_the_first_of_a_list() {
        let h = headers(&[("x-forwarded-host", "blocked.justfuture.at, proxy.internal")]);
        let uri: Uri = "/bp".parse().unwrap();
        assert_eq!(effective_host(&h, &uri, true), "blocked.justfuture.at");
    }

    #[test]
    fn http2_landing_request_is_served_not_looped() {
        // Regression: an HTTP/2 request to the landing host (no Host header) must
        // render the page, not 302 back to itself forever.
        let landing = Landing::parse("https://justdns.justfuture.at/blockpage").unwrap();
        let c = content(Some(landing));
        let h = HeaderMap::new();
        let uri: Uri = "https://justdns.justfuture.at/blockpage?d=ads.example"
            .parse()
            .unwrap();
        let host = effective_host(&h, &uri, c.trust_forwarded);
        let resp = build_response(&host, uri.query().unwrap_or(""), &c);
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(LOCATION).is_none());
    }

    #[test]
    fn proxy_landing_request_is_served_on_forwarded_host() {
        // Behind a proxy: Host is the upstream, X-Forwarded-Host is the landing.
        let landing = Landing::parse("https://blocked.justfuture.at/blockpage").unwrap();
        let mut c = content(Some(landing));
        c.trust_forwarded = true;
        let h = headers(&[
            ("host", "192.168.20.5:80"),
            ("x-forwarded-host", "blocked.justfuture.at"),
        ]);
        let uri: Uri = "/blockpage?d=ads.example".parse().unwrap();
        let host = effective_host(&h, &uri, c.trust_forwarded);
        let resp = build_response(&host, uri.query().unwrap_or(""), &c);
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(LOCATION).is_none());
    }
}
