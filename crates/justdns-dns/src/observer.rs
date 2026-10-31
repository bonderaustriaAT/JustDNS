// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A hook for recording each handled query. The resolver reports what it did
//! through this trait so it stays independent of where the log is kept; the
//! binary wires an implementation to the query store.

use std::net::IpAddr;
use std::time::Duration;

/// Where a query's answer came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerSource {
    /// Served from the response cache.
    Cache,
    /// Forwarded to an upstream resolver.
    Upstream,
    /// Answered locally without a lookup (for example a suppressed AAAA).
    Local,
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
}

/// Something that records handled queries. Implementations must not block for
/// long: they are called inline on the query path.
pub trait QueryObserver: Send + Sync {
    /// Record one handled query.
    fn observe(&self, record: QueryRecord);
}
