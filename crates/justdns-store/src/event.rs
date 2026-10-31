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
            QuerySource::Blocked => "blocked",
        }
    }

    /// Parse a stored label back into a source, if it is one we wrote.
    pub(crate) fn from_label(label: &str) -> Option<Self> {
        match label {
            "cache" => Some(QuerySource::Cache),
            "upstream" => Some(QuerySource::Upstream),
            "local" => Some(QuerySource::Local),
            "blocked" => Some(QuerySource::Blocked),
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
}
