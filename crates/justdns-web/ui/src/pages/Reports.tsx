// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useMemo, useState } from "react";

import { Toggle } from "../components/Toggle";
import {
  api,
  ApiError,
  type Report,
  type ReportListItem,
  type ReportPeriod,
  type ReportSettings,
  type TopClient,
  type TopName,
} from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useSettings } from "../state/settings";
import styles from "./Reports.module.css";

const HOURS = Array.from({ length: 24 }, (_, h) => h);
const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7];

function hourLabel(h: number): string {
  return `${String(h).padStart(2, "0")}:00`;
}

/** Trigger a browser download for a report file the API handed back as a Blob. */
function saveBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

/** Scheduled reports (administrators only): choose the daily/weekly cadence,
 *  generate one on demand, and view or download the stored summaries. Numbers
 *  are cluster-wide, matching the query-log explorer. */
export function Reports() {
  const { t, lang } = useI18n();
  const { settings, update } = useSettings();
  const reports = settings.reports;

  const [list, setList] = useState<ReportListItem[]>([]);
  const [open, setOpen] = useState<Report | null>(null);
  const [running, setRunning] = useState<ReportPeriod | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      setList(await api.reports(signal));
    } catch {
      /* keep the last-known list on a transient failure */
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal);
    return () => controller.abort();
  }, [refresh]);

  const setReports = (patch: Partial<ReportSettings>) =>
    update("reports", { ...reports, ...patch });

  const weekdayName = useMemo(() => {
    // 2024-01-01 is a Monday, so weekday n (1 = Mon … 7 = Sun) maps to Jan n.
    return (n: number) =>
      new Date(Date.UTC(2024, 0, n)).toLocaleDateString(lang, {
        weekday: "long",
        timeZone: "UTC",
      });
  }, [lang]);

  function windowLabel(fromMs: number, toMs: number, period: ReportPeriod): string {
    const fromDay = new Date(fromMs).toLocaleDateString(lang, { dateStyle: "medium" });
    if (period === "daily") return fromDay;
    // The window end is exclusive (midnight), so the last covered day is the ms before it.
    const toDay = new Date(toMs - 1).toLocaleDateString(lang, { dateStyle: "medium" });
    return t("reports.windowRange", { from: fromDay, to: toDay });
  }

  async function runReport(period: ReportPeriod) {
    setMsg(null);
    setRunning(period);
    try {
      const report = await api.runReport(period);
      setOpen(report);
      await refresh();
    } catch (cause) {
      setMsg(cause instanceof ApiError ? cause.message : t("reports.runFailed"));
    } finally {
      setRunning(null);
    }
  }

  async function view(id: string) {
    setMsg(null);
    try {
      setOpen(await api.report(id));
    } catch (cause) {
      setMsg(cause instanceof ApiError ? cause.message : t("reports.loadFailed"));
    }
  }

  async function download(report: Report, format: "html" | "csv") {
    setMsg(null);
    try {
      const blob = await api.downloadReport(report.id, format, lang);
      saveBlob(blob, `justdns-report-${report.id}.${format}`);
    } catch (cause) {
      setMsg(cause instanceof ApiError ? cause.message : t("reports.loadFailed"));
    }
  }

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t("reports.title")}</h1>
        <p className={styles.lead}>{t("reports.lead")}</p>
      </header>

      {msg && (
        <p className={styles.banner} role="alert">
          {msg}
        </p>
      )}

      {/* Cadence + on-demand generation. */}
      <section className={styles.card}>
        <div className={styles.cardHead}>
          <h2 className={styles.cardTitle}>{t("reports.settingsTitle")}</h2>
          <p className={styles.cardLead}>{t("reports.settingsLead")}</p>
        </div>

        <div className={styles.cadence}>
          <div className={styles.cadenceRow}>
            <div className={styles.cadenceMain}>
              <Toggle
                checked={reports.dailyEnabled}
                onChange={(on) => setReports({ dailyEnabled: on })}
                label={t("reports.daily")}
              />
              <div className={styles.cadenceText}>
                <span className={styles.cadenceName}>{t("reports.daily")}</span>
                <span className={styles.cadenceHint}>{t("reports.dailyHint")}</span>
              </div>
            </div>
            <label className={styles.inlineField} data-dim={!reports.dailyEnabled || undefined}>
              <span className={styles.inlineLabel}>{t("reports.atHour")}</span>
              <select
                className={styles.select}
                value={reports.dailyHour}
                disabled={!reports.dailyEnabled}
                onChange={(e) => setReports({ dailyHour: Number(e.target.value) })}
              >
                {HOURS.map((h) => (
                  <option key={h} value={h}>
                    {hourLabel(h)}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className={styles.cadenceRow}>
            <div className={styles.cadenceMain}>
              <Toggle
                checked={reports.weeklyEnabled}
                onChange={(on) => setReports({ weeklyEnabled: on })}
                label={t("reports.weekly")}
              />
              <div className={styles.cadenceText}>
                <span className={styles.cadenceName}>{t("reports.weekly")}</span>
                <span className={styles.cadenceHint}>{t("reports.weeklyHint")}</span>
              </div>
            </div>
            <div className={styles.inlineGroup}>
              <label className={styles.inlineField} data-dim={!reports.weeklyEnabled || undefined}>
                <span className={styles.inlineLabel}>{t("reports.onWeekday")}</span>
                <select
                  className={styles.select}
                  value={reports.weeklyWeekday}
                  disabled={!reports.weeklyEnabled}
                  onChange={(e) => setReports({ weeklyWeekday: Number(e.target.value) })}
                >
                  {WEEKDAYS.map((n) => (
                    <option key={n} value={n}>
                      {weekdayName(n)}
                    </option>
                  ))}
                </select>
              </label>
              <label className={styles.inlineField} data-dim={!reports.weeklyEnabled || undefined}>
                <span className={styles.inlineLabel}>{t("reports.atHour")}</span>
                <select
                  className={styles.select}
                  value={reports.weeklyHour}
                  disabled={!reports.weeklyEnabled}
                  onChange={(e) => setReports({ weeklyHour: Number(e.target.value) })}
                >
                  {HOURS.map((h) => (
                    <option key={h} value={h}>
                      {hourLabel(h)}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </div>
        </div>

        <div className={styles.cardFoot}>
          <div className={styles.footText}>
            <span className={styles.footTitle}>{t("reports.generateTitle")}</span>
            <span className={styles.footLead}>{t("reports.generateLead")}</span>
          </div>
          <div className={styles.footActions}>
            <button
              type="button"
              className={styles.button}
              disabled={running !== null}
              onClick={() => runReport("daily")}
            >
              {running === "daily" ? t("reports.running") : t("reports.runDaily")}
            </button>
            <button
              type="button"
              className={styles.button}
              disabled={running !== null}
              onClick={() => runReport("weekly")}
            >
              {running === "weekly" ? t("reports.running") : t("reports.runWeekly")}
            </button>
          </div>
        </div>
      </section>

      {open && (
        <ReportView
          report={open}
          windowLabel={windowLabel(open.fromMs, open.toMs, open.period)}
          onClose={() => setOpen(null)}
          onDownload={(format) => download(open, format)}
        />
      )}

      {/* The stored reports. */}
      <section className={styles.card}>
        <div className={styles.cardHead}>
          <h2 className={styles.cardTitle}>{t("reports.listTitle")}</h2>
          <p className={styles.cardLead}>{t("reports.clusterNote")}</p>
        </div>
        {list.length === 0 ? (
          <p className={styles.empty}>{t("reports.empty")}</p>
        ) : (
          <ul className={styles.list}>
            {list.map((item) => (
              <li key={item.id} className={styles.listRow}>
                <span className={styles.period} data-period={item.period}>
                  {t(item.period === "daily" ? "reports.periodDaily" : "reports.periodWeekly")}
                </span>
                <span className={styles.rowWindow}>
                  {windowLabel(item.fromMs, item.toMs, item.period)}
                </span>
                <span className={styles.rowStat}>
                  {item.total.toLocaleString(lang)} · {t("reports.kpiBlocked")}{" "}
                  {item.total > 0
                    ? `${((item.blocked * 100) / item.total).toFixed(1)}%`
                    : "0%"}
                </span>
                <span className={styles.rowGenerated}>
                  {t("reports.generatedAt", {
                    time: new Date(item.generatedAtMs).toLocaleString(lang, {
                      dateStyle: "medium",
                      timeStyle: "short",
                    }),
                  })}
                </span>
                <button
                  type="button"
                  className={styles.buttonGhost}
                  onClick={() => view(item.id)}
                  aria-expanded={open?.id === item.id}
                >
                  {t("reports.view")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/** The formatted view of one report: headline numbers, the source breakdown, the
 *  DNSSEC verdicts (when validation recorded any) and the three top-lists. */
function ReportView({
  report,
  windowLabel,
  onClose,
  onDownload,
}: {
  report: Report;
  windowLabel: string;
  onClose: () => void;
  onDownload: (format: "html" | "csv") => void;
}) {
  const { t, lang } = useI18n();
  const s = report.summary;
  const num = (n: number) => n.toLocaleString(lang);
  const blockRate = s.total > 0 ? ((s.blocked * 100) / s.total).toFixed(1) : "0.0";
  const hasDnssec =
    s.dnssecSecure + s.dnssecInsecure + s.dnssecBogus + s.dnssecIndeterminate > 0;

  const sources: { key: MessageKey; value: number }[] = [
    { key: "reports.sourceCache", value: s.cache },
    { key: "reports.sourceUpstream", value: s.upstream },
    { key: "reports.sourceLocal", value: s.local },
    { key: "reports.sourceRewrite", value: s.rewrite },
    { key: "reports.sourceBlocked", value: s.blocked },
  ];

  return (
    <section className={styles.card}>
      <div className={styles.viewHead}>
        <div>
          <h2 className={styles.cardTitle}>
            {t(report.period === "daily" ? "reports.periodDaily" : "reports.periodWeekly")} ·{" "}
            {windowLabel}
          </h2>
          <p className={styles.cardLead}>{t("reports.clusterNote")}</p>
        </div>
        <div className={styles.viewActions}>
          <button type="button" className={styles.button} onClick={() => onDownload("html")}>
            {t("reports.downloadHtml")}
          </button>
          <button type="button" className={styles.button} onClick={() => onDownload("csv")}>
            {t("reports.downloadCsv")}
          </button>
          <button type="button" className={styles.buttonGhost} onClick={onClose}>
            {t("reports.close")}
          </button>
        </div>
      </div>

      {s.total === 0 ? (
        <p className={styles.empty}>{t("reports.noTraffic")}</p>
      ) : (
        <>
          <div className={styles.kpis}>
            <Kpi label={t("reports.kpiTotal")} value={num(s.total)} />
            <Kpi label={t("reports.kpiBlocked")} value={num(s.blocked)} accent="danger" />
            <Kpi label={t("reports.kpiBlockRate")} value={`${blockRate}%`} accent="danger" />
            <Kpi label={t("reports.kpiAvgDuration")} value={`${num(s.avgDurationMs)} ms`} />
          </div>

          <div className={styles.breakdown}>
            <span className={styles.breakdownTitle}>{t("reports.breakdownTitle")}</span>
            <div className={styles.chips}>
              {sources.map((src) => (
                <span key={src.key} className={styles.chip}>
                  <span className={styles.chipLabel}>{t(src.key)}</span>
                  <span className={styles.chipValue}>{num(src.value)}</span>
                </span>
              ))}
            </div>
          </div>

          {hasDnssec && (
            <div className={styles.breakdown}>
              <span className={styles.breakdownTitle}>{t("reports.dnssecTitle")}</span>
              <div className={styles.chips}>
                <span className={styles.chip}>
                  <span className={styles.chipLabel}>{t("reports.dnssecSecure")}</span>
                  <span className={styles.chipValue}>{num(s.dnssecSecure)}</span>
                </span>
                <span className={styles.chip}>
                  <span className={styles.chipLabel}>{t("reports.dnssecInsecure")}</span>
                  <span className={styles.chipValue}>{num(s.dnssecInsecure)}</span>
                </span>
                <span className={styles.chip}>
                  <span className={styles.chipLabel}>{t("reports.dnssecBogus")}</span>
                  <span className={styles.chipValue}>{num(s.dnssecBogus)}</span>
                </span>
                <span className={styles.chip}>
                  <span className={styles.chipLabel}>{t("reports.dnssecIndeterminate")}</span>
                  <span className={styles.chipValue}>{num(s.dnssecIndeterminate)}</span>
                </span>
              </div>
            </div>
          )}

          <div className={styles.tables}>
            <TopTable
              title={t("reports.topDomains")}
              head={t("reports.colDomain")}
              rows={s.topDomains}
            />
            <TopTable
              title={t("reports.topBlocked")}
              head={t("reports.colDomain")}
              rows={s.topBlocked}
            />
            <TopClientTable title={t("reports.topClients")} head={t("reports.colClient")} rows={s.topClients} />
          </div>
        </>
      )}
    </section>
  );
}

function Kpi({ label, value, accent }: { label: string; value: string; accent?: "danger" }) {
  return (
    <div className={styles.kpi} data-accent={accent}>
      <span className={styles.kpiValue}>{value}</span>
      <span className={styles.kpiLabel}>{label}</span>
    </div>
  );
}

function TopTable({ title, head, rows }: { title: string; head: string; rows: TopName[] }) {
  const { t, lang } = useI18n();
  return (
    <div className={styles.topBlock}>
      <span className={styles.topTitle}>{title}</span>
      {rows.length === 0 ? (
        <p className={styles.topEmpty}>—</p>
      ) : (
        <table className={styles.topTable}>
          <thead>
            <tr>
              <th className={styles.topTh}>{head}</th>
              <th className={styles.topThNum}>{t("reports.colCount")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.name}>
                <td className={styles.topTd} title={row.name}>
                  {row.name}
                </td>
                <td className={styles.topTdNum}>{row.count.toLocaleString(lang)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function TopClientTable({ title, head, rows }: { title: string; head: string; rows: TopClient[] }) {
  const { t, lang } = useI18n();
  return (
    <div className={styles.topBlock}>
      <span className={styles.topTitle}>{title}</span>
      {rows.length === 0 ? (
        <p className={styles.topEmpty}>—</p>
      ) : (
        <table className={styles.topTable}>
          <thead>
            <tr>
              <th className={styles.topTh}>{head}</th>
              <th className={styles.topThNum}>{t("reports.colCount")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.client}>
                <td className={styles.topTd} title={row.client}>
                  {row.deviceName ? (
                    <>
                      <span className={styles.clientName}>{row.deviceName}</span>
                      <span className={styles.clientAddr}>{row.client}</span>
                    </>
                  ) : (
                    row.client
                  )}
                </td>
                <td className={styles.topTdNum}>{row.count.toLocaleString(lang)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
