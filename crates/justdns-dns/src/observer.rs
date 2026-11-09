// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A hook for recording each handled query. The resolver reports what it did
//! through this trait so it stays independent of where the log is kept; the
//! binary wires an implementation to the query store.

use std::net::IpAddr;
use std::time::Duration;

/// The DNSSEC validation verdict for a query's answer, as the four states of
/// RFC 4035 §4.3. Recorded only when validation was on and the answer came from
/// an upstream; a cache, local, blocked or rewritten answer carries none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnssecStatus {
    /// A chain of trust back to the root was built and the signatures verified.
    Secure,
    /// The zone is provably unsigned, so there is nothing to verify.
    Insecure,
    /// The answer should have validated but did not — a failed or missing
    /// signature. Treated as an attack signal: such answers are refused.
    Bogus,
    /// Validation could not reach a verdict (the upstream returned no DNSSEC
    /// data, or the chain could not be fetched).
    Indeterminate,
}

/// Where a query's answer came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerSource {
    /// Served from the response cache.
    Cache,
    /// Forwarded to an upstream resolver.
    Upstream,
    /// Answered locally without a lookup (for example a suppressed AAAA).
    Local,
    /// Answered from a user-defined local rewrite (name → fixed IPs).
    Rewrite,
    /// Refused by the blocking engine without a lookup.
    Blocked,
}

/// One handled query, as the resolver saw it. Record and response codes are the
/// numeric wire values; naming them is left to whoever records the query.
#[derive(Clone, Debug)]
pub struct QueryRecord {
    /// Address of the client that asked.
    pub client: IpAddr,
    /// The queried name in its textual form (for example `example.com.`).
    pub name: String,
    /// The requested record type, by its numeric wire value.
    pub record_type: u16,
    /// The response code returned, by its numeric wire value.
    pub response_code: u16,
    /// Where the answer came from.
    pub source: AnswerSource,
    /// How long handling took.
    pub duration: Duration,
    /// Number of answer records returned.
    pub answers: u16,
    /// The IPv4/IPv6 addresses the query resolved to, in answer order. Empty for
    /// a block, a NODATA answer, or a non-address record type.
    pub addresses: Vec<IpAddr>,
    /// A short label for the upstream resolver that answered — the DoT/DoH
    /// certificate name or the plain IP — when the answer came from one (source
    /// [`AnswerSource::Upstream`]); `None` for a cache, block, rewrite or other
    /// local answer.
    pub resolver: Option<String>,
    /// The DNSSEC validation verdict, when validation was on and the answer came
    /// from an upstream; `None` for a cache, local, blocked or rewritten answer,
    /// or when validation is off.
    pub dnssec: Option<DnssecStatus>,
}

/// Something that records handled queries. Implementations must not block for
/// long: they are called inline on the query path.
pub trait QueryObserver: Send + Sync {
    /// Record one handled query.
    fn observe(&self, record: QueryRecord);
}
