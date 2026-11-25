// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bridge the resolver's query observer to the persistent query store: turn
//! each [`QueryRecord`] the pipeline reports into a stored [`QueryEvent`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use justdns_dns::observer::{
    AnswerSource, BlockStage, DnssecStatus as DnsDnssecStatus, QueryObserver, QueryRecord,
};
use justdns_filter::{PhishingHit, PhishingKind as FilterPhishingKind, PhishingMode};
use justdns_store::{
    DnssecStatus, PhishingFlag, PhishingKind, QueryEvent, QuerySource, QueryTrace, ReasonCategory,
    Store, TraceOutcome, TraceStage, TraceStep,
};

pub struct StoreObserver {
    store: Store,
    /// Live "record queries" switch from the config store. When off, an answered
    /// query is dropped instead of logged, so the query log and its statistics
    /// stop gaining new rows without a restart.
    query_log: Arc<AtomicBool>,
}

impl StoreObserver {
    pub fn new(store: Store, query_log: Arc<AtomicBool>) -> Self {
        Self { store, query_log }
    }
}

impl QueryObserver for StoreObserver {
    fn observe(&self, record: QueryRecord) {
        if !self.query_log.load(Ordering::Relaxed) {
            return;
        }
        // Build the decision trace and reason tag from the raw signals before the
        // fields are moved into the event.
        let (reason, trace) = build_trace(&record);
        self.store.record(QueryEvent {
            at_ms: now_ms(),
            client: record.client,
            name: record.name,
            record_type: record.record_type,
            response_code: record.response_code,
            source: map_source(record.source),
            duration_ms: u32::try_from(record.duration.as_millis()).unwrap_or(u32::MAX),
            answers: record.answers,
            addresses: record.addresses,
            resolver: record.resolver,
            dnssec: record.dnssec.map(map_dnssec),
            phishing: record.phishing.map(map_phishing),
            block_reason: record.block_reason.map(str::to_owned),
            reason,
            trace: Some(trace),
        });
    }
}

/// One trace step.
fn step(stage: TraceStage, outcome: TraceOutcome, detail: Option<String>) -> TraceStep {
    TraceStep {
        stage,
        outcome,
        detail,
    }
}

/// The response code as its wire name, for the answer step's detail.
fn rcode_label(code: u16) -> Option<String> {
    Some(
        match code {
            0 => "NOERROR",
            1 => "FORMERR",
            2 => "SERVFAIL",
            3 => "NXDOMAIN",
            5 => "REFUSED",
            other => return Some(other.to_string()),
        }
        .to_owned(),
    )
}

/// A short detail for a phishing step: the imitated brand, or `NRD` for a
/// newly registered domain.
fn phishing_label(hit: &PhishingHit) -> Option<String> {
    match &hit.kind {
        FilterPhishingKind::Lookalike { brand } | FilterPhishingKind::Punycode { brand } => {
            Some(brand.clone())
        }
        FilterPhishingKind::Nrd => Some("NRD".to_owned()),
    }
}

/// The DNSSEC step's outcome and label from the verdict.
fn dnssec_step(status: DnsDnssecStatus) -> TraceStep {
    let (outcome, label) = match status {
        DnsDnssecStatus::Secure => (TraceOutcome::Pass, "Secure"),
        DnsDnssecStatus::Insecure => (TraceOutcome::Info, "Insecure"),
        DnsDnssecStatus::Bogus => (TraceOutcome::Hit, "Bogus"),
        DnsDnssecStatus::Indeterminate => (TraceOutcome::Info, "Indeterminate"),
    };
    step(TraceStage::Dnssec, outcome, Some(label.to_owned()))
}

/// Reconstruct the pipeline's decision as an ordered trace plus the reason tag
/// for the query log row, from the raw signals the resolver reported. Pass steps
/// are listed so the drawer can show every gate that ran; the decisive stage of
/// a block carries the `Hit`, and the reason tag mirrors it.
fn build_trace(record: &QueryRecord) -> (Option<ReasonCategory>, QueryTrace) {
    let paused = record.paused;
    let mut steps: Vec<TraceStep> = Vec::new();

    // A local rewrite is trusted and skips every gate.
    if record.source == AnswerSource::Rewrite {
        steps.push(step(
            TraceStage::Answer,
            TraceOutcome::Info,
            rcode_label(record.response_code),
        ));
        return (None, QueryTrace { paused, steps });
    }

    // A blocked answer: the decisive stage carries the hit, then the answer.
    if record.source == AnswerSource::Blocked {
        let reason = match record.block_stage {
            Some(BlockStage::NameBlock) => {
                steps.push(step(TraceStage::NameBlock, TraceOutcome::Hit, None));
                Some(ReasonCategory::Blocklist)
            }
            Some(BlockStage::Phishing) => {
                let detail = record.phishing.as_ref().and_then(phishing_label);
                steps.push(step(TraceStage::Phishing, TraceOutcome::Hit, detail));
                Some(ReasonCategory::Phishing)
            }
            Some(BlockStage::CnameUncloak) => {
                steps.push(step(TraceStage::CnameUncloak, TraceOutcome::Hit, None));
                Some(ReasonCategory::Blocklist)
            }
            Some(BlockStage::ResponseIp) => {
                steps.push(step(TraceStage::ResponseIp, TraceOutcome::Hit, None));
                Some(ReasonCategory::ThreatIp)
            }
            None => Some(ReasonCategory::Blocklist),
        };
        steps.push(step(
            TraceStage::Answer,
            TraceOutcome::Info,
            rcode_label(record.response_code),
        ));
        return (reason, QueryTrace { paused, steps });
    }

    // A delivered answer. The protective gates ran unless blocking was paused.
    let mut reason = None;
    if !paused {
        steps.push(step(TraceStage::NameBlock, TraceOutcome::Pass, None));
        match &record.phishing {
            Some(hit) => {
                reason = Some(ReasonCategory::Warning);
                steps.push(step(
                    TraceStage::Phishing,
                    TraceOutcome::Warn,
                    phishing_label(hit),
                ));
            }
            None => steps.push(step(TraceStage::Phishing, TraceOutcome::Pass, None)),
        }
    }

    let source_detail = match record.source {
        AnswerSource::Cache => Some("Cache".to_owned()),
        AnswerSource::Upstream => record.resolver.clone(),
        AnswerSource::Local => Some("lokal".to_owned()),
        _ => None,
    };
    steps.push(step(TraceStage::Source, TraceOutcome::Info, source_detail));

    if !paused
        && matches!(record.source, AnswerSource::Cache | AnswerSource::Upstream)
        && !record.addresses.is_empty()
    {
        steps.push(step(TraceStage::ResponseIp, TraceOutcome::Pass, None));
    }

    if let Some(status) = record.dnssec {
        steps.push(dnssec_step(status));
    }

    steps.push(step(
        TraceStage::Answer,
        TraceOutcome::Info,
        rcode_label(record.response_code),
    ));
    (reason, QueryTrace { paused, steps })
}

fn map_source(source: AnswerSource) -> QuerySource {
    match source {
        AnswerSource::Cache => QuerySource::Cache,
        AnswerSource::Upstream => QuerySource::Upstream,
        AnswerSource::Local => QuerySource::Local,
        AnswerSource::Rewrite => QuerySource::Rewrite,
        AnswerSource::Blocked => QuerySource::Blocked,
    }
}

fn map_dnssec(status: DnsDnssecStatus) -> DnssecStatus {
    match status {
        DnsDnssecStatus::Secure => DnssecStatus::Secure,
        DnsDnssecStatus::Insecure => DnssecStatus::Insecure,
        DnsDnssecStatus::Bogus => DnssecStatus::Bogus,
        DnsDnssecStatus::Indeterminate => DnssecStatus::Indeterminate,
    }
}

/// Turn the detector's hit into the stored flag: the brand it imitated (for a
/// lookalike or Punycode hit) and whether the query was blocked or only warned.
fn map_phishing(hit: PhishingHit) -> PhishingFlag {
    let blocked = matches!(hit.mode, PhishingMode::Block);
    let (kind, brand) = match hit.kind {
        FilterPhishingKind::Lookalike { brand } => (PhishingKind::Lookalike, Some(brand)),
        FilterPhishingKind::Punycode { brand } => (PhishingKind::Punycode, Some(brand)),
        FilterPhishingKind::Nrd => (PhishingKind::Nrd, None),
    };
    PhishingFlag {
        kind,
        blocked,
        brand,
    }
}

/// Milliseconds since the Unix epoch, clamped into range on the impossible
/// chance the clock is absurd.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    fn sample() -> QueryRecord {
        QueryRecord {
            client: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            name: "kek.at.".to_owned(),
            record_type: 1,
            response_code: 0,
            source: AnswerSource::Blocked,
            duration: Duration::from_millis(1),
            answers: 0,
            addresses: Vec::new(),
            resolver: None,
            dnssec: None,
            phishing: None,
            block_reason: None,
            block_stage: Some(BlockStage::NameBlock),
            paused: false,
        }
    }

    #[tokio::test]
    async fn records_a_query_when_logging_is_on() {
        let (store, _writer) = Store::in_memory(16).unwrap();
        let flag = Arc::new(AtomicBool::new(true));
        let observer = StoreObserver::new(store.clone(), flag);
        observer.observe(sample());
        assert_eq!(store.recent(16).len(), 1, "a logged query is recorded");
    }

    #[tokio::test]
    async fn drops_a_query_when_logging_is_off() {
        let (store, _writer) = Store::in_memory(16).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let observer = StoreObserver::new(store.clone(), flag.clone());
        observer.observe(sample());
        assert_eq!(store.recent(16).len(), 0, "logging off keeps no record");

        // Flipping it back on takes effect on the next query, no restart.
        flag.store(true, Ordering::Relaxed);
        observer.observe(sample());
        assert_eq!(store.recent(16).len(), 1, "the switch is live");
    }

    fn has(trace: &QueryTrace, stage: TraceStage, outcome: TraceOutcome) -> bool {
        trace
            .steps
            .iter()
            .any(|step| step.stage == stage && step.outcome == outcome)
    }

    #[test]
    fn trace_marks_a_name_block() {
        let record = sample(); // a name block by default
        let (reason, trace) = build_trace(&record);
        assert_eq!(reason, Some(ReasonCategory::Blocklist));
        assert!(has(&trace, TraceStage::NameBlock, TraceOutcome::Hit));
        assert_eq!(trace.steps.last().unwrap().stage, TraceStage::Answer);
    }

    #[test]
    fn trace_marks_a_threat_ip_block() {
        let mut record = sample();
        record.block_stage = Some(BlockStage::ResponseIp);
        record.block_reason = Some("threat-ip");
        let (reason, trace) = build_trace(&record);
        assert_eq!(reason, Some(ReasonCategory::ThreatIp));
        assert!(has(&trace, TraceStage::ResponseIp, TraceOutcome::Hit));
    }

    #[test]
    fn trace_passes_a_clean_answer() {
        let mut record = sample();
        record.source = AnswerSource::Upstream;
        record.block_stage = None;
        record.addresses = vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))];
        record.resolver = Some("dns.example".to_owned());
        let (reason, trace) = build_trace(&record);
        assert_eq!(reason, None);
        assert!(has(&trace, TraceStage::NameBlock, TraceOutcome::Pass));
        assert!(has(&trace, TraceStage::Phishing, TraceOutcome::Pass));
        assert!(has(&trace, TraceStage::Source, TraceOutcome::Info));
        assert!(has(&trace, TraceStage::ResponseIp, TraceOutcome::Pass));
    }

    #[test]
    fn trace_flags_a_phishing_warning() {
        let mut record = sample();
        record.source = AnswerSource::Upstream;
        record.block_stage = None;
        record.phishing = Some(PhishingHit {
            kind: FilterPhishingKind::Nrd,
            mode: PhishingMode::Warn,
        });
        let (reason, trace) = build_trace(&record);
        assert_eq!(reason, Some(ReasonCategory::Warning));
        assert!(has(&trace, TraceStage::Phishing, TraceOutcome::Warn));
    }

    #[test]
    fn trace_notes_a_pause_and_skips_the_gates() {
        let mut record = sample();
        record.source = AnswerSource::Upstream;
        record.block_stage = None;
        record.paused = true;
        let (_reason, trace) = build_trace(&record);
        assert!(trace.paused);
        assert!(
            !trace.steps.iter().any(|step| step.stage == TraceStage::NameBlock),
            "the protective gates are skipped while paused"
        );
        assert_eq!(trace.steps.last().unwrap().stage, TraceStage::Answer);
    }

    #[test]
    fn trace_is_minimal_for_a_rewrite() {
        let mut record = sample();
        record.source = AnswerSource::Rewrite;
        record.block_stage = None;
        let (reason, trace) = build_trace(&record);
        assert_eq!(reason, None);
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].stage, TraceStage::Answer);
    }
}
