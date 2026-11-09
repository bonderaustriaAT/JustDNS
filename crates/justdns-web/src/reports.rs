// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduled summary reports: a daily and/or weekly digest of the DNS activity,
//! generated in the background, stored on the data volume, and viewed or
//! downloaded in the dashboard. No external delivery — nothing leaves the box.
//!
//! A report aggregates the **whole cluster's** log over its window (the durable
//! DuckDB log is unioned across HA nodes), matching what the Query-log explorer
//! shows. Generation reads the aggregate from the store ([`period_report`]) and
//! freezes it, resolving device names at that moment so a later rename never
//! rewrites history.
//!
//! [`period_report`]: justdns_store::Store::period_report

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use chrono::{Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use justdns_clients::ClientResolver;
use justdns_config::ReportSettings;
use justdns_store::PeriodReport;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// How many entries each top-list in a report holds.
pub const REPORT_TOP_N: usize = 10;
/// How many daily reports to keep; older ones are pruned as new ones arrive.
const KEEP_DAILY: usize = 30;
/// How many weekly reports to keep.
const KEEP_WEEKLY: usize = 26;

/// Which cadence a report belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportPeriod {
    Daily,
    Weekly,
}

impl ReportPeriod {
    fn label(self) -> &'static str {
        match self {
            ReportPeriod::Daily => "daily",
            ReportPeriod::Weekly => "weekly",
        }
    }
}

/// One `(name, count)` entry of a top-domains list.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopName {
    pub name: String,
    pub count: u64,
}

/// One `(client, count)` entry of the top-clients list, with the device name
/// resolved at generation time when one matched.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopClient {
    pub client: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    pub count: u64,
}

/// The frozen summary a report carries: headline totals, the source breakdown,
/// DNSSEC verdicts and the three top-lists. Serialised camelCase for the UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub total: u64,
    pub blocked: u64,
    pub cache: u64,
    pub upstream: u64,
    pub local: u64,
    pub rewrite: u64,
    /// Mean handling time in ms over the window (0 when there was no traffic).
    pub avg_duration_ms: u64,
    pub dnssec_secure: u64,
    pub dnssec_insecure: u64,
    pub dnssec_bogus: u64,
    pub dnssec_indeterminate: u64,
    pub top_domains: Vec<TopName>,
    pub top_blocked: Vec<TopName>,
    pub top_clients: Vec<TopClient>,
}

impl ReportSummary {
    /// Build a summary from a store aggregate, resolving each top client's device
    /// name through `resolver` so the stored report keeps the name as it was.
    pub fn from_period(period: PeriodReport, resolver: &ClientResolver) -> Self {
        let avg_duration_ms = period
            .duration_sum_ms
            .checked_div(period.total)
            .unwrap_or(0);
        let map_names = |rows: Vec<(String, u64)>| {
            rows.into_iter()
                .map(|(name, count)| TopName { name, count })
                .collect::<Vec<_>>()
        };
        let top_clients = period
            .top_clients
            .into_iter()
            .map(|(client, count)| {
                let device_name = client
                    .parse::<IpAddr>()
                    .ok()
                    .and_then(|ip| resolver.name_for(ip).map(str::to_owned));
                TopClient {
                    client,
                    device_name,
                    count,
                }
            })
            .collect();
        Self {
            total: period.total,
            blocked: period.blocked,
            cache: period.cache,
            upstream: period.upstream,
            local: period.local,
            rewrite: period.rewrite,
            avg_duration_ms,
            dnssec_secure: period.dnssec_secure,
            dnssec_insecure: period.dnssec_insecure,
            dnssec_bogus: period.dnssec_bogus,
            dnssec_indeterminate: period.dnssec_indeterminate,
            top_domains: map_names(period.top_domains),
            top_blocked: map_names(period.top_blocked),
            top_clients,
        }
    }

    /// Blocked share of total, as a percentage rounded to one decimal.
    fn block_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.blocked as f64) * 100.0 / (self.total as f64)
        }
    }
}

/// A stored, frozen report: its cadence, the window it covers `[from_ms, to_ms)`,
/// when it was generated, and the summary. The `id` is stable and dedup-friendly
/// (`"<period>-<from_ms>"`), so the scheduler never writes the same window twice.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub period: ReportPeriod,
    pub from_ms: i64,
    pub to_ms: i64,
    pub generated_at_ms: i64,
    pub summary: ReportSummary,
}

impl Report {
    pub fn new(period: ReportPeriod, from_ms: i64, to_ms: i64, summary: ReportSummary) -> Self {
        Self {
            id: format!("{}-{}", period.label(), from_ms),
            period,
            from_ms,
            to_ms,
            generated_at_ms: now_ms(),
            summary,
        }
    }
}

/// The persisted list of generated reports, held under one lock and rewritten
/// atomically on each change — the same shape as the other JSON stores on the
/// data volume. Capped per cadence so it never grows without bound.
#[derive(Clone)]
pub struct ReportStore {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    reports: RwLock<Vec<Report>>,
}

impl ReportStore {
    /// Load the stored reports, or start empty when the file is absent or
    /// unreadable (a report is derived data, so a lost file is not fatal).
    pub fn load(path: &Path) -> Self {
        let reports = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Report>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            inner: Arc::new(Inner {
                path: path.to_path_buf(),
                reports: RwLock::new(reports),
            }),
        }
    }

    /// All reports, newest window first.
    pub fn list(&self) -> Vec<Report> {
        let mut reports = self.inner.reports.read().expect("reports lock").clone();
        reports.sort_by_key(|report| std::cmp::Reverse(report.from_ms));
        reports
    }

    /// One report by id, if present.
    pub fn get(&self, id: &str) -> Option<Report> {
        self.inner
            .reports
            .read()
            .expect("reports lock")
            .iter()
            .find(|report| report.id == id)
            .cloned()
    }

    /// The newest stored window-start for a cadence, for the scheduler's dedup.
    pub fn latest_from(&self, period: ReportPeriod) -> Option<i64> {
        self.inner
            .reports
            .read()
            .expect("reports lock")
            .iter()
            .filter(|report| report.period == period)
            .map(|report| report.from_ms)
            .max()
    }

    /// Add a report (replacing any with the same id), prune each cadence to its
    /// cap, and persist. Returns the stored report.
    pub fn add(&self, report: Report) -> Report {
        {
            let mut reports = self.inner.reports.write().expect("reports lock");
            reports.retain(|existing| existing.id != report.id);
            reports.push(report.clone());
            prune(&mut reports, ReportPeriod::Daily, KEEP_DAILY);
            prune(&mut reports, ReportPeriod::Weekly, KEEP_WEEKLY);
        }
        self.save();
        report
    }

    fn save(&self) {
        let reports = self.inner.reports.read().expect("reports lock").clone();
        if let Err(error) = write_reports(&self.inner.path, &reports) {
            warn!(%error, "could not persist reports");
        }
    }
}

/// Keep only the newest `keep` reports of `period`, dropping the oldest.
fn prune(reports: &mut Vec<Report>, period: ReportPeriod, keep: usize) {
    let mut of_period: Vec<i64> = reports
        .iter()
        .filter(|report| report.period == period)
        .map(|report| report.from_ms)
        .collect();
    if of_period.len() <= keep {
        return;
    }
    of_period.sort_unstable_by(|a, b| b.cmp(a));
    let cutoff = of_period[keep - 1];
    reports.retain(|report| report.period != period || report.from_ms >= cutoff);
}

fn write_reports(path: &Path, reports: &[Report]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let json = serde_json::to_vec_pretty(reports)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(tmp, path)
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// --- Scheduling: which window (if any) is due, in the instance's timezone ------

/// The report window that is due for the daily cadence given the current time,
/// or `None` when daily reports are off or the due window is already stored.
/// A daily report fires at `daily_hour:00` local and covers the **previous full
/// local day**; `latest_from` is the newest stored daily window-start, for dedup.
pub fn due_daily(
    settings: &ReportSettings,
    tz: Tz,
    now_ms: i64,
    latest_from: Option<i64>,
) -> Option<(i64, i64)> {
    if !settings.daily_enabled {
        return None;
    }
    let today = local_date(tz, now_ms)?;
    let hour = u32::from(settings.daily_hour.min(23));
    // The date whose scheduled run has most recently passed.
    let fired_date = if local_hour(tz, now_ms)? >= hour {
        today
    } else {
        today.pred_opt()?
    };
    let start = local_ms(tz, fired_date.pred_opt()?, 0);
    let end = local_ms(tz, fired_date, 0);
    due(start, end, latest_from)
}

/// The report window that is due for the weekly cadence, or `None` when weekly
/// reports are off or the due window is already stored. Fires on `weekly_weekday`
/// (ISO Mon=1 … Sun=7) at `weekly_hour:00` local and covers the previous seven
/// full local days.
pub fn due_weekly(
    settings: &ReportSettings,
    tz: Tz,
    now_ms: i64,
    latest_from: Option<i64>,
) -> Option<(i64, i64)> {
    if !settings.weekly_enabled {
        return None;
    }
    let today = local_date(tz, now_ms)?;
    let hour = u32::from(settings.weekly_hour.min(23));
    let want = i64::from(settings.weekly_weekday.clamp(1, 7));
    let today_wd = i64::from(today.weekday().number_from_monday());
    let mut back = (today_wd - want).rem_euclid(7);
    // Target day is today but the hour has not come yet → last week's run.
    if back == 0 && local_hour(tz, now_ms)? < hour {
        back = 7;
    }
    let fired_date = today.checked_sub_signed(ChronoDuration::days(back))?;
    let start = local_ms(
        tz,
        fired_date.checked_sub_signed(ChronoDuration::days(7))?,
        0,
    );
    let end = local_ms(tz, fired_date, 0);
    due(start, end, latest_from)
}

/// The window an on-demand "generate now" covers: the last full day or seven
/// days ending at the most recent local midnight, regardless of the cadence
/// setting or configured hour.
pub fn adhoc_window(period: ReportPeriod, tz: Tz, now_ms: i64) -> Option<(i64, i64)> {
    let today = local_date(tz, now_ms)?;
    let span = match period {
        ReportPeriod::Daily => 1,
        ReportPeriod::Weekly => 7,
    };
    let end = local_ms(tz, today, 0);
    let start = local_ms(tz, today.checked_sub_signed(ChronoDuration::days(span))?, 0);
    Some((start, end))
}

/// A window is due unless it is already the newest stored one.
fn due(start: i64, end: i64, latest_from: Option<i64>) -> Option<(i64, i64)> {
    match latest_from {
        Some(from) if from >= start => None,
        _ => Some((start, end)),
    }
}

/// The local calendar date at `now_ms` in `tz`.
fn local_date(tz: Tz, now_ms: i64) -> Option<NaiveDate> {
    Some(
        Utc.timestamp_millis_opt(now_ms)
            .single()?
            .with_timezone(&tz)
            .date_naive(),
    )
}

/// The local hour-of-day (0–23) at `now_ms` in `tz`.
fn local_hour(tz: Tz, now_ms: i64) -> Option<u32> {
    Some(
        Utc.timestamp_millis_opt(now_ms)
            .single()?
            .with_timezone(&tz)
            .hour(),
    )
}

/// Epoch-ms of `date` at `hour:00:00` local in `tz`. On a DST gap/overlap it
/// takes the earliest valid instant; report hours (midnight, early morning) are
/// not normally in a gap. Falls back to `hour 0` if the time is unrepresentable.
fn local_ms(tz: Tz, date: NaiveDate, hour: u32) -> i64 {
    let naive = date
        .and_hms_opt(hour.min(23), 0, 0)
        .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).expect("midnight is valid"));
    tz.from_local_datetime(&naive)
        .earliest()
        .or_else(|| tz.from_local_datetime(&naive).latest())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|| naive.and_utc().timestamp_millis())
}

// --- Rendering -----------------------------------------------------------------

/// The language the download labels are rendered in.
#[derive(Clone, Copy)]
pub enum RenderLang {
    De,
    En,
}

impl RenderLang {
    /// Parse a `lang` query value; anything but `en` is German (the default UI
    /// language).
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some(v) if v.eq_ignore_ascii_case("en") => RenderLang::En,
            _ => RenderLang::De,
        }
    }
}

/// A fixed label set for the rendered report, in both UI languages.
struct Labels {
    title: &'static str,
    daily: &'static str,
    weekly: &'static str,
    period: &'static str,
    generated: &'static str,
    total: &'static str,
    blocked: &'static str,
    block_rate: &'static str,
    avg: &'static str,
    sources: &'static str,
    cache: &'static str,
    upstream: &'static str,
    local: &'static str,
    rewrite: &'static str,
    top_domains: &'static str,
    top_blocked: &'static str,
    top_clients: &'static str,
    domain: &'static str,
    client: &'static str,
    count: &'static str,
    none: &'static str,
}

fn labels(lang: RenderLang) -> Labels {
    match lang {
        RenderLang::De => Labels {
            title: "JustDNS-Bericht",
            daily: "Täglich",
            weekly: "Wöchentlich",
            period: "Zeitraum",
            generated: "Erzeugt",
            total: "Anfragen gesamt",
            blocked: "Geblockt",
            block_rate: "Block-Rate",
            avg: "Ø Antwortzeit",
            sources: "Nach Quelle",
            cache: "Cache",
            upstream: "Upstream",
            local: "Lokal",
            rewrite: "Rewrite",
            top_domains: "Top-Domains",
            top_blocked: "Top geblockte Domains",
            top_clients: "Top-Geräte",
            domain: "Domain",
            client: "Gerät / Client",
            count: "Anfragen",
            none: "Keine Daten in diesem Zeitraum.",
        },
        RenderLang::En => Labels {
            title: "JustDNS report",
            daily: "Daily",
            weekly: "Weekly",
            period: "Period",
            generated: "Generated",
            total: "Total queries",
            blocked: "Blocked",
            block_rate: "Block rate",
            avg: "Avg response time",
            sources: "By source",
            cache: "Cache",
            upstream: "Upstream",
            local: "Local",
            rewrite: "Rewrite",
            top_domains: "Top domains",
            top_blocked: "Top blocked domains",
            top_clients: "Top devices",
            domain: "Domain",
            client: "Device / client",
            count: "Queries",
            none: "No data in this period.",
        },
    }
}

/// Format an epoch-ms as a local `YYYY-MM-DD HH:MM` string in `tz`.
fn fmt_ts(ms: i64, tz: Tz) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// Minimal HTML escaping for the few user-influenced strings (domain/client
/// names and the instance name) placed into the rendered report.
fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render a report as a self-contained, printable HTML document (print to PDF for
/// a file to keep). Styling is inline so the download needs nothing else.
pub fn to_html(report: &Report, instance_name: &str, tz: Tz, lang: RenderLang) -> String {
    let l = labels(lang);
    let s = &report.summary;
    let period_name = match report.period {
        ReportPeriod::Daily => l.daily,
        ReportPeriod::Weekly => l.weekly,
    };
    let window = format!(
        "{} – {}",
        fmt_ts(report.from_ms, tz),
        fmt_ts(report.to_ms, tz)
    );

    let name_table = |title: &str, rows: &[TopName]| -> String {
        if rows.is_empty() {
            return format!("<h2>{}</h2><p class=\"none\">{}</p>", esc(title), l.none);
        }
        let body: String = rows
            .iter()
            .map(|r| {
                format!(
                    "<tr><td>{}</td><td class=\"n\">{}</td></tr>",
                    esc(&r.name),
                    r.count
                )
            })
            .collect();
        format!(
            "<h2>{}</h2><table><thead><tr><th>{}</th><th class=\"n\">{}</th></tr></thead><tbody>{}</tbody></table>",
            esc(title),
            l.domain,
            l.count,
            body
        )
    };

    let client_table = {
        if s.top_clients.is_empty() {
            format!(
                "<h2>{}</h2><p class=\"none\">{}</p>",
                esc(l.top_clients),
                l.none
            )
        } else {
            let body: String = s
                .top_clients
                .iter()
                .map(|c| {
                    let label = match &c.device_name {
                        Some(name) => format!(
                            "{} <span class=\"muted\">({})</span>",
                            esc(name),
                            esc(&c.client)
                        ),
                        None => esc(&c.client),
                    };
                    format!(
                        "<tr><td>{}</td><td class=\"n\">{}</td></tr>",
                        label, c.count
                    )
                })
                .collect();
            format!(
                "<h2>{}</h2><table><thead><tr><th>{}</th><th class=\"n\">{}</th></tr></thead><tbody>{}</tbody></table>",
                esc(l.top_clients),
                l.client,
                l.count,
                body
            )
        }
    };

    format!(
        "<!doctype html><html lang=\"{lang_attr}\"><head><meta charset=\"utf-8\">\
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
    <title>{title} — {inst}</title><style>\
    :root{{color-scheme:light}}\
    body{{font:15px/1.5 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;color:#1a1a1a;background:#fff;margin:2rem auto;max-width:52rem;padding:0 1rem}}\
    h1{{font-size:1.5rem;margin:0 0 .25rem}}h2{{font-size:1.1rem;margin:2rem 0 .5rem;border-bottom:1px solid #e3e3e3;padding-bottom:.25rem}}\
    .sub{{color:#666;margin:0 0 1.5rem}}\
    .kpis{{display:flex;flex-wrap:wrap;gap:1rem;margin:1rem 0}}\
    .kpi{{flex:1 1 8rem;border:1px solid #e3e3e3;border-radius:.5rem;padding:.75rem 1rem}}\
    .kpi .v{{font-size:1.5rem;font-weight:600}}.kpi .k{{color:#666;font-size:.85rem}}\
    table{{border-collapse:collapse;width:100%}}th,td{{text-align:left;padding:.35rem .5rem;border-bottom:1px solid #eee}}\
    .n{{text-align:right;font-variant-numeric:tabular-nums}}.muted{{color:#888}}.none{{color:#888}}\
    .srcs{{color:#444}}\
    </style></head><body>\
    <h1>{title}</h1><p class=\"sub\">{inst} · {period_label}: {period_name}<br>{period_word}: {window}<br>{gen}: {generated}</p>\
    <div class=\"kpis\">\
    <div class=\"kpi\"><div class=\"v\">{total}</div><div class=\"k\">{total_l}</div></div>\
    <div class=\"kpi\"><div class=\"v\">{blocked}</div><div class=\"k\">{blocked_l}</div></div>\
    <div class=\"kpi\"><div class=\"v\">{rate:.1}%</div><div class=\"k\">{rate_l}</div></div>\
    <div class=\"kpi\"><div class=\"v\">{avg} ms</div><div class=\"k\">{avg_l}</div></div>\
    </div>\
    <p class=\"srcs\"><strong>{sources}:</strong> {cache_l} {cache} · {upstream_l} {upstream} · {local_l} {local} · {rewrite_l} {rewrite}</p>\
    {top_dom}{top_blk}{top_cli}\
    </body></html>",
        lang_attr = match lang { RenderLang::De => "de", RenderLang::En => "en" },
        title = esc(l.title),
        inst = esc(instance_name),
        period_label = l.period,
        period_name = period_name,
        period_word = l.period,
        window = window,
        gen = l.generated,
        generated = fmt_ts(report.generated_at_ms, tz),
        total = s.total,
        total_l = l.total,
        blocked = s.blocked,
        blocked_l = l.blocked,
        rate = s.block_rate(),
        rate_l = l.block_rate,
        avg = s.avg_duration_ms,
        avg_l = l.avg,
        sources = l.sources,
        cache_l = l.cache,
        cache = s.cache,
        upstream_l = l.upstream,
        upstream = s.upstream,
        local_l = l.local,
        local = s.local,
        rewrite_l = l.rewrite,
        rewrite = s.rewrite,
        top_dom = name_table(l.top_domains, &s.top_domains),
        top_blk = name_table(l.top_blocked, &s.top_blocked),
        top_cli = client_table,
    )
}

/// Render a report's top-lists as CSV: a section per list, each a `name,count`
/// (or `client,device,count`) block. A spreadsheet-friendly export.
pub fn to_csv(report: &Report, lang: RenderLang) -> String {
    let l = labels(lang);
    let s = &report.summary;
    let mut out = String::new();
    let csv = |field: &str| -> String {
        if field.contains([',', '"', '\n']) {
            format!("\"{}\"", field.replace('"', "\"\""))
        } else {
            field.to_owned()
        }
    };

    out.push_str(&format!(
        "{}\n{},{}\n",
        csv(l.top_domains),
        l.domain,
        l.count
    ));
    for row in &s.top_domains {
        out.push_str(&format!("{},{}\n", csv(&row.name), row.count));
    }
    out.push_str(&format!(
        "\n{}\n{},{}\n",
        csv(l.top_blocked),
        l.domain,
        l.count
    ));
    for row in &s.top_blocked {
        out.push_str(&format!("{},{}\n", csv(&row.name), row.count));
    }
    out.push_str(&format!(
        "\n{}\n{},{},{}\n",
        csv(l.top_clients),
        l.client,
        "device",
        l.count
    ));
    for row in &s.top_clients {
        out.push_str(&format!(
            "{},{},{}\n",
            csv(&row.client),
            csv(row.device_name.as_deref().unwrap_or("")),
            row.count
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz() -> Tz {
        "Europe/Vienna".parse().unwrap()
    }

    /// 2026-11-10 is a Tuesday. 08:00 local in Vienna (UTC+1) = 07:00 UTC.
    fn at(y: i32, m: u32, d: u32, h: u32) -> i64 {
        tz().with_ymd_and_hms(y, m, d, h, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn daily_due_after_the_hour_covers_yesterday() {
        let settings = ReportSettings {
            daily_enabled: true,
            daily_hour: 6,
            ..ReportSettings::default()
        };
        // Now is 2026-11-10 08:00, past 06:00 → window is all of the 9th.
        let now = at(2026, 11, 10, 8);
        let (start, end) = due_daily(&settings, tz(), now, None).expect("due");
        assert_eq!(start, at(2026, 11, 9, 0));
        assert_eq!(end, at(2026, 11, 10, 0));
        // Already stored → not due again.
        assert!(due_daily(&settings, tz(), now, Some(start)).is_none());
    }

    #[test]
    fn daily_before_the_hour_covers_the_day_before() {
        let settings = ReportSettings {
            daily_enabled: true,
            daily_hour: 6,
            ..ReportSettings::default()
        };
        // Now is 2026-11-10 05:00, before 06:00 → last fire was the 9th 06:00,
        // which covers the 8th.
        let now = at(2026, 11, 10, 5);
        let (start, end) = due_daily(&settings, tz(), now, None).expect("due");
        assert_eq!(start, at(2026, 11, 8, 0));
        assert_eq!(end, at(2026, 11, 9, 0));
    }

    #[test]
    fn daily_off_is_never_due() {
        let settings = ReportSettings::default();
        assert!(due_daily(&settings, tz(), at(2026, 11, 10, 8), None).is_none());
    }

    #[test]
    fn weekly_due_on_its_weekday_covers_prior_week() {
        // Monday = 1. 2026-11-09 is a Monday.
        let settings = ReportSettings {
            weekly_enabled: true,
            weekly_weekday: 1,
            weekly_hour: 6,
            ..ReportSettings::default()
        };
        // Monday 2026-11-09 08:00, past 06:00 → window is the 7 days before.
        let now = at(2026, 11, 9, 8);
        let (start, end) = due_weekly(&settings, tz(), now, None).expect("due");
        assert_eq!(end, at(2026, 11, 9, 0));
        assert_eq!(start, at(2026, 11, 2, 0));
    }

    #[test]
    fn adhoc_daily_is_the_last_full_local_day() {
        let (start, end) = adhoc_window(ReportPeriod::Daily, tz(), at(2026, 11, 10, 15)).unwrap();
        assert_eq!(start, at(2026, 11, 9, 0));
        assert_eq!(end, at(2026, 11, 10, 0));
    }

    #[test]
    fn store_caps_each_cadence_and_dedups_by_window() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reports.json");
        let store = ReportStore::load(&path);
        let summary =
            ReportSummary::from_period(PeriodReport::default(), &ClientResolver::new(vec![]));
        // Same window twice → one entry.
        store.add(Report::new(
            ReportPeriod::Daily,
            1000,
            2000,
            summary.clone(),
        ));
        store.add(Report::new(
            ReportPeriod::Daily,
            1000,
            2000,
            summary.clone(),
        ));
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.latest_from(ReportPeriod::Daily), Some(1000));
        // Over the cap, oldest drops.
        for i in 0..(KEEP_DAILY as i64 + 5) {
            let from = 10_000 + i * 1000;
            store.add(Report::new(
                ReportPeriod::Daily,
                from,
                from + 500,
                summary.clone(),
            ));
        }
        let daily = store
            .list()
            .into_iter()
            .filter(|r| r.period == ReportPeriod::Daily)
            .count();
        assert_eq!(daily, KEEP_DAILY);
        // Reload from disk keeps the capped set.
        let reloaded = ReportStore::load(&path);
        assert_eq!(reloaded.list().len(), KEEP_DAILY);
    }
}
