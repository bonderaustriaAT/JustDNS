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

/// Which kind of phishing signal fired on a query's name. Mirrors the detector's
/// verdict so the store and web layer can show it without depending on the filter
/// crate's own types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhishingKind {
    /// The name imitates a protected brand with lookalike (confusable) characters.
    Lookalike,
    /// The name imitates a protected brand through a Punycode (IDN) label.
    Punycode,
    /// The name is a newly registered domain.
    Nrd,
}

impl PhishingKind {
    /// The lowercase token used in the stored label.
    fn as_str(self) -> &'static str {
        match self {
            PhishingKind::Lookalike => "lookalike",
            PhishingKind::Punycode => "punycode",
            PhishingKind::Nrd => "nrd",
        }
    }

    /// Parse a stored token back into a kind, if it is one we wrote.
    fn parse(token: &str) -> Option<Self> {
        match token {
            "lookalike" => Some(PhishingKind::Lookalike),
            "punycode" => Some(PhishingKind::Punycode),
            "nrd" => Some(PhishingKind::Nrd),
            _ => None,
        }
    }
}

/// A phishing detection recorded on a query: which signal fired, whether the
/// query was blocked (versus only warned), and the imitated brand's display name
/// for a lookalike or Punycode hit. A computed marker on the answer, mirroring
/// [`DnssecStatus`]: present only when the detector fired, absent otherwise. The
/// answer itself is never changed by a warn-mode hit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhishingFlag {
    /// Which kind of signal fired.
    pub kind: PhishingKind,
    /// True when the query was blocked, false when only warned.
    pub blocked: bool,
    /// The imitated brand's display name, for a lookalike or Punycode hit; absent
    /// for a newly-registered-domain hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
}

impl PhishingFlag {
    /// The compact label stored in the query log, `mode:kind[:brand]` — for
    /// example `block:lookalike:PayPal` or `warn:nrd`. The brand is the last
    /// field so a display name containing a colon round-trips intact.
    pub(crate) fn label(&self) -> String {
        let mode = if self.blocked { "block" } else { "warn" };
        match &self.brand {
            Some(brand) => format!("{mode}:{}:{brand}", self.kind.as_str()),
            None => format!("{mode}:{}", self.kind.as_str()),
        }
    }

    /// Parse a stored label back into a flag, if it is one we wrote. An empty or
    /// unrecognised label reads back as `None`.
    pub fn from_label(label: &str) -> Option<Self> {
        let mut parts = label.splitn(3, ':');
        let blocked = match parts.next()? {
            "block" => true,
            "warn" => false,
            _ => return None,
        };
        let kind = PhishingKind::parse(parts.next()?)?;
        let brand = parts.next().map(str::to_owned).filter(|s| !s.is_empty());
        Some(Self {
            kind,
            blocked,
            brand,
        })
    }
}

/// The category that decided a query, for the one-glance reason tag in the log
/// row. Derived from the decisive stage of the [`QueryTrace`] and stored in its
/// own small column, so the live list can colour the tag without reading the
/// full trace back. Blocklist and uncloak are one category (both are name-list
/// blocks); a response-IP hit is its own, as is a phishing warn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCategory {
    /// A name block from a block list (including a CNAME-uncloak block).
    Blocklist,
    /// A phishing or newly-registered-domain block.
    Phishing,
    /// The answer resolved to an IP on a response-IP block list.
    ThreatIp,
    /// A phishing warning: the answer was delivered, only flagged.
    Warning,
}

impl ReasonCategory {
    /// The lowercase label stored in the query log.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ReasonCategory::Blocklist => "blocklist",
            ReasonCategory::Phishing => "phishing",
            ReasonCategory::ThreatIp => "threat_ip",
            ReasonCategory::Warning => "warning",
        }
    }

    /// Parse a stored label back into a category, if it is one we wrote.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "blocklist" => Some(ReasonCategory::Blocklist),
            "phishing" => Some(ReasonCategory::Phishing),
            "threat_ip" => Some(ReasonCategory::ThreatIp),
            "warning" => Some(ReasonCategory::Warning),
            _ => None,
        }
    }
}

/// One gate of the resolver pipeline, in the order a query passes through them.
/// The decision trace lists the gates that ran, so a query carries its own
/// explanation. A gate skipped because blocking was paused is not emitted; the
/// trace's [`paused`](QueryTrace::paused) flag carries that instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStage {
    /// The name was checked against the client's block policy (block and allow
    /// lists).
    NameBlock,
    /// The name was checked by the phishing / newly-registered-domain detector.
    Phishing,
    /// Where the answer came from: the cache or a named upstream resolver.
    Source,
    /// The answer's CNAME chain was inspected for a cloaked tracker.
    CnameUncloak,
    /// The answer's IP addresses were checked against the response-IP block set.
    ResponseIp,
    /// The DNSSEC validation verdict for an upstream answer.
    Dnssec,
    /// The response finally delivered to the client.
    Answer,
}

/// What a [`TraceStage`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    /// Checked and not matched: the query passed this gate.
    Pass,
    /// Matched and blocked here: the decisive gate.
    Hit,
    /// Matched as a warning: flagged, but the answer was still delivered.
    Warn,
    /// A neutral, informational stage (the source, the DNSSEC verdict, the final
    /// answer) that did not itself block.
    Info,
}

/// One stage of a query's decision trace: which gate, what it decided, and a
/// short human-readable detail where one adds something (the imitated brand, the
/// blocked IP, the CNAME target, the serving resolver, the final response).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceStep {
    /// The pipeline gate this step records.
    pub stage: TraceStage,
    /// What the gate decided.
    pub outcome: TraceOutcome,
    /// A short detail for the stage, when one is worth showing; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The full decision trace for one query: the ordered stages that ran, plus
/// whether blocking was paused at the time (which is why the protective gates do
/// not appear). Persisted to the query log on disk only and read back when the
/// log entry's detail is opened, so it never weighs on the in-memory hot view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryTrace {
    /// Whether the troubleshooting pause was in force, so the protective gates
    /// (name block, phishing, CNAME uncloak) were skipped and are not listed.
    #[serde(default)]
    pub paused: bool,
    /// The stages that ran, in pipeline order.
    pub steps: Vec<TraceStep>,
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
    /// The phishing verdict, when the detector fired on this query's name; absent
    /// otherwise. A computed marker like [`Self::dnssec`]: a warn-mode hit leaves
    /// the answer unchanged, a block-mode hit is recorded on the blocked answer.
    /// Older logs written before this field read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phishing: Option<PhishingFlag>,
    /// Why the answer was blocked by a response-level check rather than a name
    /// block, as a short stable label; absent otherwise. Currently only
    /// `"threat-ip"`: the resolved answer carried an IP on an IP block list.
    /// Older logs written before this field read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    /// The category that decided this query, for the one-glance reason tag in the
    /// log row. Carried in the in-memory hot view and the list, so the tag needs
    /// no extra read. `None` for a plain, undecided query. Older logs read back as
    /// `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ReasonCategory>,
    /// The full decision trace: the ordered pipeline gates this query ran through.
    /// Written to the durable log on disk only and never kept in the hot view, so
    /// it does not weigh on memory; read back from disk when the entry's detail is
    /// opened. `None` in the hot view and in list reads; present only when loaded
    /// for one entry. Older logs read back as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<QueryTrace>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phishing_flag_label_round_trips() {
        let cases = [
            PhishingFlag {
                kind: PhishingKind::Lookalike,
                blocked: true,
                brand: Some("PayPal".to_owned()),
            },
            PhishingFlag {
                kind: PhishingKind::Punycode,
                blocked: false,
                brand: Some("Microsoft".to_owned()),
            },
            PhishingFlag {
                kind: PhishingKind::Nrd,
                blocked: false,
                brand: None,
            },
        ];
        for flag in cases {
            let label = flag.label();
            assert_eq!(
                PhishingFlag::from_label(&label),
                Some(flag.clone()),
                "round-trips through {label:?}"
            );
        }
    }

    #[test]
    fn phishing_flag_label_carries_mode_and_brand() {
        let flag = PhishingFlag {
            kind: PhishingKind::Lookalike,
            blocked: true,
            brand: Some("Erste Bank / George".to_owned()),
        };
        // mode:kind:brand, brand last so a colon in the name would survive too.
        assert_eq!(flag.label(), "block:lookalike:Erste Bank / George");
    }

    #[test]
    fn empty_or_unknown_phishing_label_reads_as_none() {
        assert_eq!(PhishingFlag::from_label(""), None);
        assert_eq!(PhishingFlag::from_label("secure"), None);
        assert_eq!(PhishingFlag::from_label("block:unknownkind"), None);
    }
}
