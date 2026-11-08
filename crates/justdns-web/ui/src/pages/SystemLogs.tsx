import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { UpdateCard } from "../components/UpdateCard";
import { api, type DiagInfo, type LogVerbosity, type SystemLogRecord } from "../lib/api";
import { useI18n } from "../lib/i18n";
import styles from "./SystemLogs.module.css";

/** How many diagnostic events the stream pulls at once. */
const LOG_LIMIT = 500;
/** How often status and log are refreshed. */
const REFRESH_MS = 2_000;

/** The verbosity presets the toolbar offers, in order. */
const VERBOSITY: LogVerbosity[] = ["normal", "debug", "trace"];

/** The category a log target belongs to: its crate prefix, e.g. `justdns_dns`
 *  for `justdns_dns::client`. Used to group the stream by subsystem. */
function categoryOf(target: string): string {
  const head = target.split("::", 1)[0] ?? target;
  return head;
}

/** A friendly, language-aware name for a subsystem category. Unknown crates
 *  (and future ones) fall back to the raw prefix, so the filter never hides a
 *  category just because it has no label yet. */
const CATEGORY_LABELS: Record<string, [de: string, en: string]> = {
  justdns: ["Kern", "Core"],
  justdns_dns: ["DNS & Upstreams", "DNS & upstreams"],
  justdns_lists: ["Listen", "Lists"],
  justdns_web: ["Web & HA", "Web & HA"],
  justdns_store: ["Speicher", "Storage"],
  justdns_schedule: ["Zeitpläne", "Schedules"],
};
function categoryLabel(cat: string, lang: "de" | "en"): string {
  const pair = CATEGORY_LABELS[cat];
  if (!pair) return cat;
  return lang === "de" ? pair[0] : pair[1];
}

/** The level filter the toolbar offers: everything, warnings and above, or only
 *  errors. `null` means no server-side level filter. */
type LevelFilter = null | "warn" | "error";

/** Load the runtime status and the recent diagnostic log, filtered to `level`
 *  when set, refreshing every couple of seconds. What is already on screen stays
 *  put across a refetch so neither the tiles nor the stream flash empty. */
function useSystemData(level: LevelFilter) {
  const { t } = useI18n();
  const [diag, setDiag] = useState<DiagInfo | null>(null);
  const [logs, setLogs] = useState<SystemLogRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  // Bumped to force an out-of-band refetch — e.g. right after the verbosity
  // changes, so the status tiles and stream reflect it without waiting for the
  // next poll tick.
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((n) => n + 1), []);

  useEffect(() => {
    let active = true;
    const controller = new AbortController();

    async function load() {
      try {
        const [nextDiag, nextLogs] = await Promise.all([
          api.diag(controller.signal),
          api.logsSystem(level ?? undefined, LOG_LIMIT, controller.signal),
        ]);
        if (!active) return;
        setDiag(nextDiag);
        setLogs(nextLogs);
        setError(null);
        setLoading(false);
      } catch (err) {
        if (!active || controller.signal.aborted) return;
        setError(err instanceof Error ? err.message : t("common.unknownError"));
        setLoading(false);
      }
    }

    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => {
      active = false;
      controller.abort();
      clearInterval(timer);
    };
  }, [level, t, nonce]);

  return { diag, logs, error, loading, reload };
}

/** Seconds since start as a short "2 T 3 Std"-style string, dropping leading
 *  zero units and keeping at most the two most significant. */
function useUptime() {
  const { t } = useI18n();
  return (secs: number): string => {
    const d = Math.floor(secs / 86_400);
    const h = Math.floor((secs % 86_400) / 3_600);
    const m = Math.floor((secs % 3_600) / 60);
    const s = secs % 60;
    const parts: string[] = [];
    if (d) parts.push(`${d} ${t("diag.unit.d")}`);
    if (h) parts.push(`${h} ${t("diag.unit.h")}`);
    if (!d && m) parts.push(`${m} ${t("diag.unit.m")}`);
    if (!d && !h && s) parts.push(`${s} ${t("diag.unit.s")}`);
    if (parts.length === 0) parts.push(`0 ${t("diag.unit.s")}`);
    return parts.slice(0, 2).join(" ");
  };
}

/** One listener dot with its label, tinted by whether it is serving. */
function Listener({ label, up }: { label: string; up: boolean }) {
  const { t } = useI18n();
  return (
    <span className={styles.listener} data-up={up || undefined}>
      <span className={styles.listenerDot} aria-hidden="true" />
      <span className={styles.listenerLabel}>{label}</span>
      <span className={styles.listenerState}>{up ? t("diag.up") : t("diag.down")}</span>
    </span>
  );
}

export function SystemLogs() {
  const { t, locale, lang } = useI18n();
  const [level, setLevel] = useState<LevelFilter>(null);
  const [category, setCategory] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [verbosityBusy, setVerbosityBusy] = useState(false);
  const [verbosityError, setVerbosityError] = useState<string | null>(null);
  // The level the user just picked, shown as active immediately while the engine
  // applies it (optimistic), so the control reacts at once instead of waiting for
  // the next diag poll. Cleared once the poll confirms it (or on error).
  const [pendingLevel, setPendingLevel] = useState<LogVerbosity | null>(null);
  const { diag, logs, error, loading, reload } = useSystemData(level);
  const formatUptime = useUptime();

  // Drop the optimistic override as soon as the server confirms the new level.
  useEffect(() => {
    if (pendingLevel && diag?.logLevel === pendingLevel) setPendingLevel(null);
  }, [pendingLevel, diag]);

  // The level the segmented control shows as active: the pending pick while it is
  // being applied, otherwise whatever the engine currently reports.
  const activeLevel: string = pendingLevel ?? diag?.logLevel ?? "";

  // Raise or lower what the engine emits (not just what is shown). The change is
  // temporary; the diag poll re-reads the active level. Optimistic: the clicked
  // level goes active at once and the log shows a loading indicator meanwhile.
  async function changeVerbosity(next: LogVerbosity) {
    if (verbosityBusy || activeLevel === next) return;
    setPendingLevel(next);
    setVerbosityBusy(true);
    setVerbosityError(null);
    try {
      await api.setLogLevel(next);
      reload();
    } catch (err) {
      setPendingLevel(null);
      setVerbosityError(err instanceof Error ? err.message : t("common.unknownError"));
    } finally {
      setVerbosityBusy(false);
    }
  }

  const timeFmt = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      }),
    [locale],
  );
  const numberFmt = useMemo(() => new Intl.NumberFormat(locale), [locale]);

  // The subsystem categories present in the current stream, for the filter. Kept
  // as the raw crate prefixes; a friendly label is applied at render time.
  const categories = useMemo(() => {
    if (!logs) return [];
    return Array.from(new Set(logs.map((r) => categoryOf(r.target)))).sort();
  }, [logs]);

  // A category that scrolled out of the window stops matching anything; drop the
  // filter back to "all" so the stream never looks mysteriously empty.
  useEffect(() => {
    if (category && !categories.includes(category)) setCategory(null);
  }, [category, categories]);

  // Search and category are applied on the client over message, source and
  // fields so typing stays instant and does not wait on a round trip.
  const needle = search.trim().toLowerCase();
  const rows = useMemo(() => {
    if (!logs) return null;
    return logs.filter((r) => {
      if (category && categoryOf(r.target) !== category) return false;
      if (!needle) return true;
      return (
        r.message.toLowerCase().includes(needle) ||
        r.target.toLowerCase().includes(needle) ||
        (r.fields ?? "").toLowerCase().includes(needle)
      );
    });
  }, [logs, needle, category]);

  const errorCount = rows ? rows.filter((r) => r.level === "error").length : 0;

  // Keep the newest entry in view when already pinned near the top (the stream
  // is newest-first, so "near the top" is where live tailing happens).
  const listRef = useRef<HTMLDivElement>(null);

  function download() {
    if (!rows || rows.length === 0) return;
    const text = rows
      .map((r) => {
        const base = `${r.time}  ${r.level.toUpperCase().padEnd(5)}  ${r.target}  ${r.message}`;
        return r.fields ? `${base}  ${r.fields}` : base;
      })
      .join("\n");
    const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    a.href = url;
    a.download = `justdns-log-${stamp}.txt`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  }

  const levels: { key: LevelFilter; labelKey: "systemlogs.filterAll" | "systemlogs.filterWarn" | "systemlogs.filterError" }[] = [
    { key: null, labelKey: "systemlogs.filterAll" },
    { key: "warn", labelKey: "systemlogs.filterWarn" },
    { key: "error", labelKey: "systemlogs.filterError" },
  ];

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <h1 className={styles.title}>{t("systemlogs.title")}</h1>
        <p className={styles.intro}>{t("systemlogs.intro")}</p>
      </header>

      {/* Update: check GitHub, show a new release, one-click update. */}
      <UpdateCard />

      {/* Runtime status tiles. */}
      <section aria-label={t("diag.sectionTitle")}>
        <h2 className={styles.sectionTitle}>{t("diag.sectionTitle")}</h2>
        {diag === null ? (
          <p className={styles.note}>
            {error ? t("systemlogs.unreachable", { error }) : t("systemlogs.loading")}
          </p>
        ) : (
          <div className={styles.tiles}>
            <div className={styles.tile}>
              <span className={styles.tileLabel}>{t("diag.version")}</span>
              <span className={styles.tileValue}>{diag.version}</span>
              <span className={styles.tileSub}>
                {t("diag.uptime")}: {formatUptime(diag.uptimeSecs)}
              </span>
            </div>

            <div className={styles.tile}>
              <span className={styles.tileLabel}>{t("diag.listeners")}</span>
              <div className={styles.listeners}>
                <Listener label={t("diag.listener.udp")} up={diag.listeners.udp} />
                <Listener label={t("diag.listener.tcp")} up={diag.listeners.tcp} />
                <Listener label={t("diag.listener.dot")} up={diag.listeners.dot} />
                <Listener label={t("diag.listener.doh")} up={diag.listeners.doh} />
              </div>
            </div>

            <div className={styles.tile} data-tone={diag.feeds.lastError ? "danger" : undefined}>
              <span className={styles.tileLabel}>{t("diag.feeds")}</span>
              <span className={styles.tileValue}>
                {diag.feeds.lastRefresh
                  ? timeFmt.format(new Date(diag.feeds.lastRefresh))
                  : t("diag.feedsNever")}
              </span>
              <span className={styles.tileSub}>
                {diag.feeds.lastRefresh
                  ? diag.feeds.lastError
                    ? t("diag.feedsError")
                    : t("diag.feedsOk")
                  : "—"}
              </span>
            </div>

            <div className={styles.tile}>
              <span className={styles.tileLabel}>{t("diag.cache")}</span>
              <span className={styles.tileValue}>{numberFmt.format(diag.cacheEntries)}</span>
            </div>

            <div className={styles.tile}>
              <span className={styles.tileLabel}>{t("diag.activeClients")}</span>
              <span className={styles.tileValue}>{numberFmt.format(diag.activeClients)}</span>
            </div>

            <div className={styles.tile}>
              <span className={styles.tileLabel}>{t("diag.lists")}</span>
              <span className={styles.tileValue}>
                {numberFmt.format(
                  diag.lists.ads + diag.lists.malware + diag.lists.custom + diag.lists.allow,
                )}
              </span>
              <span className={styles.tileSub}>
                {numberFmt.format(diag.lists.ads)} {t("diag.listsAds")} ·{" "}
                {numberFmt.format(diag.lists.malware)} {t("diag.listsMalware")} ·{" "}
                {numberFmt.format(diag.lists.custom)} {t("diag.listsCustom")} ·{" "}
                {numberFmt.format(diag.lists.allow)} {t("diag.listsAllow")}
              </span>
            </div>
          </div>
        )}
      </section>

      {/* Verbosity: raise what the engine emits (not just what the stream
          shows). Temporary — a restart returns to the shipping default. */}
      {diag !== null && (
        <section className={styles.verbosity} aria-label={t("systemlogs.verbosityTitle")}>
          <div className={styles.verbosityText}>
            <span className={styles.verbosityLabel}>{t("systemlogs.verbosityTitle")}</span>
            <span className={styles.verbosityHint}>
              {diag.logLevel === "custom"
                ? t("systemlogs.verbosityCustom")
                : t("systemlogs.verbosityHint")}
            </span>
            {verbosityError && (
              <span className={styles.verbosityError} role="alert">
                {verbosityError}
              </span>
            )}
          </div>
          <div className={styles.segmented} role="group" aria-label={t("systemlogs.verbosityTitle")}>
            {VERBOSITY.map((option) => (
              <button
                key={option}
                type="button"
                className={styles.segment}
                data-active={activeLevel === option || undefined}
                data-pending={(verbosityBusy && pendingLevel === option) || undefined}
                aria-pressed={activeLevel === option}
                onClick={() => changeVerbosity(option)}
              >
                {verbosityBusy && pendingLevel === option && (
                  <span className={styles.segSpinner} aria-hidden="true" />
                )}
                {t(`systemlogs.verbosity.${option}`)}
              </button>
            ))}
          </div>
        </section>
      )}

      {/* Diagnostic log stream. */}
      <section aria-label={t("systemlogs.logTitle")}>
        <div className={styles.logHead}>
          <div className={styles.logTitleWrap}>
            <h2 className={styles.sectionTitle}>{t("systemlogs.logTitle")}</h2>
            {verbosityBusy && (
              <span className={styles.logBusyChip} role="status">
                <span className={styles.segSpinner} aria-hidden="true" />
                {t("systemlogs.verbosityApplying")}
              </span>
            )}
          </div>
          <div className={styles.toolbar}>
            <div className={styles.segmented} role="group" aria-label={t("systemlogs.logTitle")}>
              {levels.map((option) => (
                <button
                  key={option.labelKey}
                  type="button"
                  className={styles.segment}
                  data-active={level === option.key || undefined}
                  aria-pressed={level === option.key}
                  onClick={() => setLevel(option.key)}
                >
                  {t(option.labelKey)}
                </button>
              ))}
            </div>
            {categories.length > 1 && (
              <select
                className={styles.category}
                value={category ?? ""}
                aria-label={t("systemlogs.categoryLabel")}
                onChange={(e) => setCategory(e.target.value || null)}
              >
                <option value="">{t("systemlogs.categoryAll")}</option>
                {categories.map((cat) => (
                  <option key={cat} value={cat}>
                    {categoryLabel(cat, lang)}
                  </option>
                ))}
              </select>
            )}
            <input
              type="search"
              className={styles.search}
              placeholder={t("systemlogs.search")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            <button
              type="button"
              className={styles.download}
              onClick={download}
              disabled={!rows || rows.length === 0}
            >
              {t("systemlogs.download")}
            </button>
          </div>
        </div>

        <div className={styles.logBusyBar} data-on={verbosityBusy || undefined} aria-hidden="true" />

        {errorCount > 0 && (
          <div className={styles.alert} role="alert">
            <svg
              className={styles.alertIcon}
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d="M12 3l9 16H3l9-16z" />
              <path d="M12 10v4M12 17.5v.01" />
            </svg>
            <div className={styles.alertText}>
              <strong className={styles.alertTitle}>
                {errorCount === 1
                  ? t("systemlogs.errorBannerOne")
                  : t("systemlogs.errorBanner", { n: errorCount })}
              </strong>
              <span className={styles.alertHint}>{t("systemlogs.errorBannerHint")}</span>
            </div>
          </div>
        )}

        {rows === null ? (
          <p className={styles.note}>
            {error ? t("systemlogs.unreachable", { error }) : loading ? t("systemlogs.loading") : t("common.noData")}
          </p>
        ) : rows.length === 0 ? (
          <p className={styles.note}>
            {level || needle || category ? t("systemlogs.emptyFiltered") : t("systemlogs.empty")}
          </p>
        ) : (
          <div className={styles.list} ref={listRef}>
            <div className={styles.columns} aria-hidden="true">
              <span>{t("systemlogs.colTime")}</span>
              <span>{t("systemlogs.colLevel")}</span>
              <span>{t("systemlogs.colSource")}</span>
              <span>{t("systemlogs.colMessage")}</span>
            </div>
            {rows.map((row, index) => (
              <div
                key={`${row.time}-${index}`}
                className={styles.row}
                data-level={row.level}
              >
                <span className={styles.time} title={row.time}>
                  {timeFmt.format(new Date(row.time))}
                </span>
                <span className={styles.levelCell}>
                  <span className={styles.badge} data-level={row.level}>
                    {t(`systemlogs.level.${row.level}`)}
                  </span>
                </span>
                <span className={styles.source} title={row.target}>
                  {row.target}
                </span>
                <span className={styles.message}>
                  {row.message}
                  {row.fields && <span className={styles.fields}> {row.fields}</span>}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
