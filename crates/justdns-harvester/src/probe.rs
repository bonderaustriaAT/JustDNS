// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Network probes used by the selection test, behind traits so the logic is
//! testable offline.
//!
//! Two questions a candidate must answer, and neither can be read from the log:
//! *is it a live host or a parked/dead one?* (a DNS resolve against a public
//! resolver, deliberately **not** the local JustDNS instance, whose answer would
//! be filtered) and *does blocking it break a site or only remove tracking?* (an
//! HTTPS fetch of the root that classifies the response as a human website or a
//! bare beacon endpoint). Both are heuristics that feed the report; a human makes
//! the final call.

use std::io::Read;
use std::time::Duration;

use serde::Deserialize;
use tracing::debug;

/// What a DNS resolve told us about a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// Resolves to at least one routable address (listed).
    Live(Vec<String>),
    /// Resolves only to a null/sinkhole address (`0.0.0.0`, `127.0.0.1`, `::`) —
    /// a parked or deliberately-nulled host, not an active tracker.
    Parked,
    /// No usable answer: NXDOMAIN, SERVFAIL or an empty/AAAA-only answer.
    Dead,
}

/// What an HTTPS fetch of the root told us about a candidate's function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteOutcome {
    /// Serves a human-facing website (HTML with navigation/content) — likely a
    /// company's own site, so blocking the apex would remove function.
    HumanSite,
    /// Serves only a tracking payload: a tiny/empty body, a 204, a pixel image,
    /// or a bare script/JSON endpoint. Safe to block.
    Beacon,
    /// Did not answer over HTTPS at all (refused, timed out, TLS error). Leans
    /// beacon but is reported as its own, less-certain verdict.
    Unreachable,
    /// Answered, but the response did not clearly look like either.
    Unknown,
}

/// Resolves a domain against a public resolver.
pub trait Resolver {
    fn resolve(&self, domain: &str) -> ResolveOutcome;
}

/// Probes what a domain serves over HTTPS.
pub trait SiteProbe {
    fn probe(&self, domain: &str) -> SiteOutcome;
}

/// The null/sinkhole addresses that mark a parked or nulled host.
fn is_null_address(ip: &str) -> bool {
    matches!(
        ip.trim(),
        "0.0.0.0" | "127.0.0.1" | "::" | "::1" | "0:0:0:0:0:0:0:0"
    )
}

// --- Cloudflare DoH resolver --------------------------------------------------

#[derive(Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: i32,
    #[serde(default, rename = "Answer")]
    answer: Vec<DohAnswer>,
}

#[derive(Deserialize)]
struct DohAnswer {
    #[serde(rename = "type")]
    rtype: u16,
    #[serde(default)]
    data: String,
}

/// A resolver that asks Cloudflare's DNS-over-HTTPS JSON endpoint. Public on
/// purpose: the local instance would return a filtered answer, which is exactly
/// what we are trying to see past.
pub struct DohResolver {
    agent: ureq::Agent,
    endpoint: String,
}

impl DohResolver {
    /// A resolver against the given DoH JSON endpoint (e.g.
    /// `https://cloudflare-dns.com/dns-query`).
    pub fn new(endpoint: impl Into<String>, timeout: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .build();
        Self {
            agent,
            endpoint: endpoint.into(),
        }
    }
}

impl Resolver for DohResolver {
    fn resolve(&self, domain: &str) -> ResolveOutcome {
        let url = format!("{}?name={}&type=A", self.endpoint, domain);
        let body = match self
            .agent
            .get(&url)
            .set("accept", "application/dns-json")
            .call()
        {
            Ok(response) => match response.into_string() {
                Ok(body) => body,
                Err(error) => {
                    debug!(%domain, %error, "DoH response read failed");
                    return ResolveOutcome::Dead;
                }
            },
            Err(error) => {
                debug!(%domain, %error, "DoH resolve failed");
                return ResolveOutcome::Dead;
            }
        };
        let parsed: DohResponse = match serde_json::from_str(&body) {
            Ok(parsed) => parsed,
            Err(error) => {
                debug!(%domain, %error, "DoH response unparseable");
                return ResolveOutcome::Dead;
            }
        };
        if parsed.status != 0 {
            return ResolveOutcome::Dead;
        }
        let addrs: Vec<String> = parsed
            .answer
            .into_iter()
            .filter(|a| a.rtype == 1) // A record
            .map(|a| a.data.trim().to_owned())
            .filter(|d| !d.is_empty())
            .collect();
        if addrs.is_empty() {
            return ResolveOutcome::Dead;
        }
        if addrs.iter().all(|ip| is_null_address(ip)) {
            return ResolveOutcome::Parked;
        }
        ResolveOutcome::Live(addrs)
    }
}

// --- HTTPS function probe -----------------------------------------------------

/// How many bytes of a response body we read before deciding. Enough to judge a
/// page's shape without pulling a whole site.
const PROBE_BODY_LIMIT: usize = 64 * 1024;

/// A probe that fetches `https://<domain>/` and classifies the response.
pub struct HttpSiteProbe {
    agent: ureq::Agent,
}

impl HttpSiteProbe {
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .user_agent("justdns-harvester/0.1 (+candidate-review)")
            .build();
        Self { agent }
    }
}

impl SiteProbe for HttpSiteProbe {
    fn probe(&self, domain: &str) -> SiteOutcome {
        let url = format!("https://{domain}/");
        let response = match self.agent.get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(code, _)) => {
                // A 204/404/410 with no site body is beacon-shaped; a 401/403
                // guarding an endpoint likewise is not a human site.
                return match code {
                    204 | 404 | 410 | 401 | 403 => SiteOutcome::Beacon,
                    _ => SiteOutcome::Unknown,
                };
            }
            Err(_) => return SiteOutcome::Unreachable,
        };

        let content_type = response
            .header("content-type")
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut buf = Vec::with_capacity(4096);
        let _ = response
            .into_reader()
            .take(PROBE_BODY_LIMIT as u64)
            .read_to_end(&mut buf);
        let body = String::from_utf8_lossy(&buf);

        classify_response(&content_type, &body)
    }
}

/// Classify an HTTPS response into a function verdict from its content type and
/// body. Split out from the network so it can be unit-tested directly.
pub fn classify_response(content_type: &str, body: &str) -> SiteOutcome {
    // Non-HTML payloads are tracking endpoints, not sites.
    if content_type.starts_with("image/")
        || content_type.contains("javascript")
        || content_type.contains("json")
    {
        return SiteOutcome::Beacon;
    }
    // A tiny body is a beacon even if it claims to be HTML.
    if body.trim().len() < 256 {
        return SiteOutcome::Beacon;
    }
    if content_type.contains("text/html") || body.to_ascii_lowercase().contains("<html") {
        let lower = body.to_ascii_lowercase();
        let anchors = lower.matches("<a ").count();
        let has_nav = lower.contains("<nav") || lower.contains("role=\"navigation\"");
        let has_heading = lower.contains("<h1") || lower.contains("<h2");
        let has_structure = lower.contains("<article")
            || lower.contains("<main")
            || lower.contains("<header")
            || lower.contains("<footer");
        // A real site has navigation and structure; a tag/consent stub does not.
        if (has_nav || anchors >= 5) && (has_heading || has_structure) {
            return SiteOutcome::HumanSite;
        }
        return SiteOutcome::Beacon;
    }
    SiteOutcome::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_addresses_recognised() {
        assert!(is_null_address("0.0.0.0"));
        assert!(is_null_address(" 127.0.0.1 "));
        assert!(!is_null_address("185.54.150.220"));
    }

    #[test]
    fn classify_spots_a_human_site() {
        let body = format!(
            "<html><head><title>Etracker GmbH</title></head><body><nav><a href=\"/\">Home</a>\
             <a href=\"/produkt\">Produkt</a></nav><main><h1>Willkommen</h1><p>{}</p></main>\
             <footer>Impressum</footer></body></html>",
            "Lorem ipsum ".repeat(40)
        );
        assert_eq!(
            classify_response("text/html; charset=utf-8", &body),
            SiteOutcome::HumanSite
        );
    }

    #[test]
    fn classify_spots_beacons() {
        assert_eq!(
            classify_response("image/gif", "GIF89a…"),
            SiteOutcome::Beacon
        );
        assert_eq!(
            classify_response("application/javascript", &"x".repeat(5000)),
            SiteOutcome::Beacon
        );
        assert_eq!(classify_response("text/html", ""), SiteOutcome::Beacon);
        // HTML that is just a tag stub, no nav/structure → beacon.
        let stub = format!("<html><body>{}</body></html>", "ping ".repeat(100));
        assert_eq!(classify_response("text/html", &stub), SiteOutcome::Beacon);
    }
}
