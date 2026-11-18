// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The run output: a ranked candidate report in Markdown (for a human to read)
//! and JSON (for tooling), plus a short tally of why the rest were dropped.
//!
//! The harvester never blocks anything or opens a PR on its own — this report is
//! its only product. A human runs the final check on the shortlist and adds the
//! approved domains to the shared list by hand.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::probe::{ResolveOutcome, SiteOutcome};
use crate::select::{Candidate, Exclusion, Recommendation};

/// One candidate as written to JSON.
#[derive(Debug, Serialize)]
pub struct CandidateOut {
    pub root: String,
    pub recommend_kind: &'static str,
    pub recommend: Vec<String>,
    pub hits: u64,
    pub distinct_clients: usize,
    pub days_seen: usize,
    pub name_signal: bool,
    pub geo_dach: bool,
    pub fanout: bool,
    pub resolve: String,
    pub addresses: Vec<String>,
    pub apex_function: String,
    pub score: f64,
}

/// One exclusion as written to JSON.
#[derive(Debug, Serialize)]
pub struct ExclusionOut {
    pub root: String,
    pub reason: String,
}

/// The whole run report.
#[derive(Debug, Serialize)]
pub struct Report {
    pub generated_unix: u64,
    pub candidate_count: usize,
    pub candidates: Vec<CandidateOut>,
    pub excluded_count: usize,
    pub excluded: Vec<ExclusionOut>,
}

fn resolve_label(r: &ResolveOutcome) -> (String, Vec<String>) {
    match r {
        ResolveOutcome::Live(ips) => ("live".to_owned(), ips.clone()),
        ResolveOutcome::Parked => ("parked".to_owned(), Vec::new()),
        ResolveOutcome::Dead => ("dead".to_owned(), Vec::new()),
    }
}

fn site_label(s: &SiteOutcome) -> &'static str {
    match s {
        SiteOutcome::HumanSite => "human-site",
        SiteOutcome::Beacon => "beacon",
        SiteOutcome::Unreachable => "unreachable",
        SiteOutcome::Unknown => "unknown",
    }
}

impl Report {
    /// Build a report from the run's candidates and exclusions. Candidates are
    /// sorted by score, highest first.
    pub fn build(
        generated_unix: u64,
        mut candidates: Vec<Candidate>,
        excluded: Vec<Exclusion>,
    ) -> Self {
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.root.cmp(&b.root))
        });
        let candidates: Vec<CandidateOut> = candidates
            .into_iter()
            .map(|c| {
                let (resolve, addresses) = resolve_label(&c.resolve);
                let (recommend_kind, recommend) = match c.recommendation {
                    Recommendation::Root(root) => ("root", vec![root]),
                    Recommendation::Hosts(hosts) => ("hosts", hosts),
                };
                CandidateOut {
                    root: c.root,
                    recommend_kind,
                    recommend,
                    hits: c.hits,
                    distinct_clients: c.distinct_clients,
                    days_seen: c.days_seen,
                    name_signal: c.signals.name_hit,
                    geo_dach: c.signals.geo_dach,
                    fanout: c.signals.fanout,
                    resolve,
                    addresses,
                    apex_function: site_label(&c.apex_site).to_owned(),
                    score: c.score,
                }
            })
            .collect();
        let excluded: Vec<ExclusionOut> = excluded
            .into_iter()
            .map(|e| ExclusionOut {
                root: e.root,
                reason: e.reason.label().to_owned(),
            })
            .collect();
        Self {
            generated_unix,
            candidate_count: candidates.len(),
            excluded_count: excluded.len(),
            candidates,
            excluded,
        }
    }

    /// Render the human-readable Markdown report.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# JustDNS-Harvester — Kandidatenbericht\n\n");
        md.push_str(&format!(
            "Erzeugt (Unix): {}  ·  Kandidaten: {}  ·  Ausgeschlossen: {}\n\n",
            self.generated_unix, self.candidate_count, self.excluded_count
        ));
        md.push_str(
            "> Reiner Report. Nichts wird automatisch geblockt. Jeden Kandidaten vor der \
             Aufnahme in die geteilte Liste noch manuell prüfen (reiner Mess-/Ad-Host ohne \
             Seitenfunktion, DACH-spezifisch, nicht schon international abgedeckt).\n\n",
        );

        md.push_str("## Kandidaten (nach Score)\n\n");
        if self.candidates.is_empty() {
            md.push_str("_Keine neuen Kandidaten in diesem Lauf._\n\n");
        } else {
            md.push_str(
                "| # | Score | Vorschlag | Geräte | Hits | Tage | Signale | Resolve | Apex |\n",
            );
            md.push_str(
                "|---|------:|-----------|------:|-----:|-----:|---------|---------|------|\n",
            );
            for (i, c) in self.candidates.iter().enumerate() {
                let suggestion = match c.recommend_kind {
                    "root" => format!("`{}`", c.recommend.join("`, `")),
                    _ => format!("Subhosts: `{}`", c.recommend.join("`, `")),
                };
                let mut signals = Vec::new();
                if c.name_signal {
                    signals.push("Name");
                }
                if c.geo_dach {
                    signals.push("Geo");
                }
                if c.fanout {
                    signals.push("Fan-out");
                }
                md.push_str(&format!(
                    "| {} | {:.2} | {} | {} | {} | {} | {} | {} | {} |\n",
                    i + 1,
                    c.score,
                    suggestion,
                    c.distinct_clients,
                    c.hits,
                    c.days_seen,
                    signals.join(", "),
                    c.resolve,
                    c.apex_function,
                ));
            }
            md.push('\n');
        }

        if !self.excluded.is_empty() {
            md.push_str("## Ausgeschlossen (Zusammenfassung)\n\n");
            // Tally reasons rather than list every root.
            let mut tally: std::collections::BTreeMap<&str, usize> =
                std::collections::BTreeMap::new();
            for e in &self.excluded {
                *tally.entry(e.reason.as_str()).or_default() += 1;
            }
            md.push_str("| Grund | Anzahl |\n|-------|-------:|\n");
            for (reason, count) in tally {
                md.push_str(&format!("| {reason} | {count} |\n"));
            }
            md.push('\n');
        }
        md
    }

    /// Write `candidates-<unix>.{md,json}` plus stable `latest.{md,json}` into
    /// `out_dir`, returning the two timestamped paths.
    pub fn write(&self, out_dir: &Path) -> io::Result<(PathBuf, PathBuf)> {
        std::fs::create_dir_all(out_dir)?;
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let md = self.to_markdown();

        let json_path = out_dir.join(format!("candidates-{}.json", self.generated_unix));
        let md_path = out_dir.join(format!("candidates-{}.md", self.generated_unix));
        std::fs::write(&json_path, &json)?;
        std::fs::write(&md_path, &md)?;
        std::fs::write(out_dir.join("latest.json"), &json)?;
        std::fs::write(out_dir.join("latest.md"), &md)?;
        Ok((md_path, json_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::Signals;

    fn candidate(root: &str, score: f64, rec: Recommendation) -> Candidate {
        Candidate {
            root: root.to_owned(),
            recommendation: rec,
            hits: 100,
            distinct_clients: 7,
            days_seen: 4,
            signals: Signals {
                name_hit: true,
                geo_dach: true,
                fanout: true,
            },
            resolve: ResolveOutcome::Live(vec!["1.2.3.4".into()]),
            apex_site: SiteOutcome::Beacon,
            score,
        }
    }

    #[test]
    fn builds_sorted_and_renders() {
        let report = Report::build(
            1_700_000_000,
            vec![
                candidate("low.at", 5.0, Recommendation::Root("low.at".into())),
                candidate("high.de", 12.0, Recommendation::Root("high.de".into())),
            ],
            Vec::new(),
        );
        assert_eq!(report.candidates[0].root, "high.de");
        let md = report.to_markdown();
        assert!(md.contains("high.de"));
        assert!(md.contains("Kandidaten"));
    }

    #[test]
    fn writes_files() {
        let dir = tempfile::tempdir().unwrap();
        let report = Report::build(
            42,
            vec![candidate(
                "x.de",
                9.0,
                Recommendation::Hosts(vec!["a.x.de".into()]),
            )],
            Vec::new(),
        );
        let (md, json) = report.write(dir.path()).unwrap();
        assert!(md.exists() && json.exists());
        assert!(dir.path().join("latest.md").exists());
    }
}
