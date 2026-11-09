import { useCallback, useEffect, useRef, useState } from "react";

import { api, type HistoryCursor, type HistoryFilter, type QueryRow, type QuerySource } from "./api";
import {
  displayName,
  recordTypeDescription,
  recordTypeLabel,
  responseCodeDescription,
  responseCodeLabel,
  sourceLabel,
} from "./dns";
import { useI18n, type Lang } from "./i18n";

/** How many log entries the page shows at once. */
export const LOG_LIMIT = 100;
/** How often to pull fresh entries while live mode is on. */
const LIVE_REFRESH_MS = 2_000;

/** Time and date formatters for the active locale. Recreated per render of the
 *  hook, which only changes when the locale does. */
function formatters(locale: string) {
  return {
    time: new Intl.DateTimeFormat(locale, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    }),
    date: new Intl.DateTimeFormat(locale, {
      day: "2-digit",
      month: "2-digit",
    }),
    dateTime: new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    }),
  };
}

/** How to tint the response-code tag in the list. NXDOMAIN is normal, so it is
 *  "neutral", not an alarm; only a real failure (SERVFAIL) is "danger". A code
 *  that needs no tag at all (a clean NOERROR, or a block that already shows the
 *  "Geblockt" tag) is "none". */
export type RcodeTone = "none" | "neutral" | "danger";

export interface LogRow {
  at_ms: number;
  time: string;
  /** Compact day/month, shown beside the time in the history view where rows
   *  span several days. */
  date: string;
  /** Full date and time for the detail view. */
  fullTime: string;
  client: string;
  /** The device name of the client, when one is set. */
  deviceName?: string;
  name: string;
  type: string;
  typeDesc?: string;
  /** Raw source, kept for styling; `sourceText` is what we print. */
  source: QuerySource;
  sourceText: string;
  isBlocked: boolean;
  responseCode: number;
  rcode: string;
  rcodeDesc?: string;
  rcodeTone: RcodeTone;
  answers: number;
  /** The IPs the query resolved to, in answer order. */
  addresses: string[];
  durationMs: number;
  /** The peer node this row was pulled from, in a cluster; absent for this
   *  node's own queries. */
  origin?: string;
}

function rcodeTone(responseCode: number, isBlocked: boolean): RcodeTone {
  if (isBlocked || responseCode === 0) return "none";
  if (responseCode === 2) return "danger"; // SERVFAIL — a real failure
  return "neutral";
}

export interface LogsState {
  rows: LogRow[] | null;
  error: string | null;
  loading: boolean;
}

function toLogRow(row: QueryRow, lang: Lang, fmt: ReturnType<typeof formatters>): LogRow {
  const isBlocked = row.source === "blocked";
  return {
    at_ms: row.at_ms,
    time: fmt.time.format(row.at_ms),
    date: fmt.date.format(row.at_ms),
    fullTime: fmt.dateTime.format(row.at_ms),
    client: row.client,
    deviceName: row.device_name,
    name: displayName(row.name),
    type: recordTypeLabel(row.record_type),
    typeDesc: recordTypeDescription(row.record_type, lang),
    source: row.source,
    sourceText: sourceLabel(row.source, lang),
    isBlocked,
    responseCode: row.response_code,
    rcode: responseCodeLabel(row.response_code),
    rcodeDesc: responseCodeDescription(row.response_code, lang),
    rcodeTone: rcodeTone(row.response_code, isBlocked),
    answers: row.answers,
    addresses: row.addresses ?? [],
    durationMs: row.duration_ms,
    origin: row.origin,
  };
}

/** Load the newest log entries for `query`, refreshing every couple of seconds
 *  while `live` is on. Entries already on screen stay put during a refetch, so
 *  the list never flashes empty between polls or while a new search resolves.
 *  `enabled` gates the whole hook: the explorer turns it off while a facet
 *  filter is active, so the ring poll stops and the history view takes over. */
export function useLogs(query: string, live: boolean, enabled = true): LogsState {
  const { lang, t, locale } = useI18n();
  const [state, setState] = useState<LogsState>({
    rows: null,
    error: null,
    loading: true,
  });

  useEffect(() => {
    if (!enabled) {
      // Idle while the history view owns the screen; drop any stale ring rows.
      setState({ rows: null, error: null, loading: false });
      return;
    }
    let active = true;
    const controller = new AbortController();
    const fmt = formatters(locale);

    async function load() {
      try {
        const rows = await api.queries(LOG_LIMIT, query, controller.signal);
        if (!active) return;
        setState({
          rows: rows.map((row) => toLogRow(row, lang, fmt)),
          error: null,
          loading: false,
        });
      } catch (error) {
        if (!active || controller.signal.aborted) return;
        setState((prev) => ({
          rows: prev.rows,
          error: error instanceof Error ? error.message : t("common.unknownError"),
          loading: false,
        }));
      }
    }

    load();
    const timer = live ? setInterval(load, LIVE_REFRESH_MS) : undefined;
    return () => {
      active = false;
      controller.abort();
      if (timer) clearInterval(timer);
    };
    // `lang`/`locale` are in the deps so the labels and the time formatting are
    // rebuilt in the new language when the user switches.
  }, [query, live, enabled, lang, locale, t]);

  return state;
}

/** The history explorer's state: a growing list of pages, whether another page
 *  can be loaded, and the actions to load it. */
export interface HistoryState {
  rows: LogRow[] | null;
  error: string | null;
  /** The first page is loading (the list is empty or being replaced). */
  loading: boolean;
  /** A further page is being appended (the list already has rows). */
  loadingMore: boolean;
  /** Another page can be fetched. */
  hasMore: boolean;
  /** Fetch and append the next page. A no-op while a load is in flight. */
  loadMore: () => void;
}

/** Read the **full durable history** (DuckDB) for `filter`, newest first, a page
 *  at a time. Changing the filter (or the language) resets to the first page;
 *  `loadMore` appends the next, following the keyset cursor so paging stays
 *  correct past the live ring and across equal timestamps. `enabled` turns the
 *  hook off (and clears it) while the live ring view is the one on screen.
 *
 *  `filter` must be referentially stable between renders (memoise it in the
 *  caller), since its identity is what re-triggers the first-page load. */
export function useHistory(filter: HistoryFilter, enabled: boolean): HistoryState {
  const { lang, t, locale } = useI18n();
  const [rows, setRows] = useState<LogRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [cursor, setCursor] = useState<HistoryCursor | null>(null);

  // Bumped on every reset so a slow in-flight fetch from an older filter can
  // tell it has been superseded and drop its result.
  const epochRef = useRef(0);
  // The current locale's formatters, kept fresh for `loadMore` (which runs
  // outside the effect and would otherwise capture a stale copy).
  const fmtRef = useRef(formatters(locale));
  fmtRef.current = formatters(locale);

  useEffect(() => {
    if (!enabled) {
      epochRef.current += 1; // cancel any in-flight load
      setRows(null);
      setError(null);
      setLoading(false);
      setLoadingMore(false);
      setCursor(null);
      return;
    }
    const epoch = (epochRef.current += 1);
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    (async () => {
      try {
        const page = await api.queriesHistory(filter, null, LOG_LIMIT, controller.signal);
        if (epochRef.current !== epoch) return;
        setRows(page.rows.map((row) => toLogRow(row, lang, fmtRef.current)));
        setCursor(page.nextBefore ?? null);
        setLoading(false);
      } catch (err) {
        if (controller.signal.aborted || epochRef.current !== epoch) return;
        setError(err instanceof Error ? err.message : t("common.unknownError"));
        setLoading(false);
      }
    })();
    return () => controller.abort();
  }, [filter, enabled, lang, locale, t]);

  const loadMore = useCallback(() => {
    if (!enabled || loading || loadingMore || !cursor) return;
    const epoch = epochRef.current;
    const controller = new AbortController();
    setLoadingMore(true);
    (async () => {
      try {
        const page = await api.queriesHistory(filter, cursor, LOG_LIMIT, controller.signal);
        if (epochRef.current !== epoch) return;
        setRows((prev) => [
          ...(prev ?? []),
          ...page.rows.map((row) => toLogRow(row, lang, fmtRef.current)),
        ]);
        setCursor(page.nextBefore ?? null);
        setLoadingMore(false);
      } catch (err) {
        if (controller.signal.aborted || epochRef.current !== epoch) return;
        setError(err instanceof Error ? err.message : t("common.unknownError"));
        setLoadingMore(false);
      }
    })();
  }, [enabled, loading, loadingMore, cursor, filter, lang, t]);

  return { rows, error, loading, loadingMore, hasMore: cursor !== null, loadMore };
}
