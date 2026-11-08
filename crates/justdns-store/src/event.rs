// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The record of a single handled query, shared by the DNS path that produces
//! it and the web API that reads it back. Record and response codes are kept as
//! their numeric wire values so the store stays independent of the DNS crate;
//! turning them into names is the presentation layer's job.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Where the answer to a query came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySource {
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

impl QuerySource {
    /// The lowercase label stored in the query log.
    pub(crate) fn label(self) -> &'static str {
        match self {
            QuerySource::Cache => "cache",
            QuerySource::Upstream => "upstream",
            QuerySource::Local => "local",
            QuerySource::Rewrite => "rewrite",
            QuerySource::Blocked => "blocked",
        }
    }

    /// Parse a stored label back into a source, if it is one we wrote. Also used
    /// by the web layer to turn a `source=` filter value into a [`QuerySource`].
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "cache" => Some(QuerySource::Cache),
            "upstream" => Some(QuerySource::Upstream),
            "local" => Some(QuerySource::Local),
            "rewrite" => Some(QuerySource::Rewrite),
            "blocked" => Some(QuerySource::Blocked),
            _ => None,
        }
    }
}

/// The DNSSEC validation verdict for a query's answer — the four states of
/// RFC 4035 §4.3. Present only on an upstream answer validated while DNSSEC was
/// on; a cache, local, blocked or rewritten answer, or any answer logged with
/// validation off, carries none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecStatus {
    /// Verified against a chain of trust back to the root.
    Secure,
    /// The zone is provably unsigned; there is nothing to verify.
    Insecure,
    /// Should have validated but did not — refused as a possible attack.
    Bogus,
    /// Validation could not reach a verdict (no DNSSEC data from upstream).
    Indeterminate,
}

impl DnssecStatus {
    /// The lowercase label stored in the query log.
    pub(crate) fn label(self) -> &'static str {
        match self {
            DnssecStatus::Secure => "secure",
            DnssecStatus::Insecure => "insecure",
            DnssecStatus::Bogus => "bogus",
            DnssecStatus::Indeterminate => "indeterminate",
        }
    }

    /// Parse a stored label back into a status, if it is one we wrote.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "secure" => Some(DnssecStatus::Secure),
            "insecure" => Some(DnssecStatus::Insecure),
            "bogus" => Some(DnssecStatus::Bogus),
            "indeterminate" => Some(DnssecStatus::Indeterminate),
            _ => None,
        }
    }
}

/// One handled query and how it was answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryEvent {
    /// Wall-clock time the query finished, in milliseconds since the Unix epoch.
    pub at_ms: i64,
    /// Address of the client that asked.
    pub client: IpAddr,
    /// The queried name in its textual form (for example `example.com.`).
    pub name: String,
    /// The requested record type, by its numeric wire value.
    pub record_type: u16,
    /// The response code returned to the client, by its numeric wire value.
    pub response_code: u16,
    /// Where the answer came from.
    pub source: QuerySource,
    /// How long handling took, in milliseconds.
    pub duration_ms: u32,
    /// Number of answer records returned.
    pub answers: u16,
    /// The IPv4/IPv6 addresses the query resolved to, in answer order. Empty for
    /// a block, a NODATA answer, or a non-address record type.
    #[serde(default)]
    pub addresses: Vec<IpAddr>,
    /// A short label for the upstream resolver that answered — the DoT/DoH
    /// certificate name or the plain IP — when the answer came from one; absent
    /// otherwise. Older logs written before this field read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
    /// The DNSSEC validation verdict, when the answer was validated upstream
    /// (DNSSEC on) and came from a resolver; absent for a cache, local, blocked
    /// or rewritten answer and whenever validation was off. Older logs written
    /// before this field read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dnssec: Option<DnssecStatus>,
}
