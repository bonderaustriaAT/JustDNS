import { useEffect, useState } from "react";

import { api, type QueryRow, type QuerySource } from "./api";
import {
  displayName,
  recordTypeDescription,
  recordTypeLabel,
  responseCodeDescription,
  responseCodeLabel,
  sourceLabel,
} from "./dns";

/** How many log entries the page shows at once. */
export const LOG_LIMIT = 100;
/** How often to pull fresh entries while live mode is on. */
const LIVE_REFRESH_MS = 2_000;

const timeFmt = new Intl.DateTimeFormat("de-DE", {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

const dateTimeFmt = new Intl.DateTimeFormat("de-DE", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

/** How to tint the response-code tag in the list. NXDOMAIN is normal, so it is
 *  "neutral", not an alarm; only a real failure (SERVFAIL) is "danger". A code
 *  that needs no tag at all (a clean NOERROR, or a block that already shows the
 *  "Geblockt" tag) is "none". */
export type RcodeTone = "none" | "neutral" | "danger";

export interface LogRow {
  at_ms: number;
  time: string;
  /** Full date and time for the detail view. */
  fullTime: string;
  client: string;
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
  durationMs: number;
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

function toLogRow(row: QueryRow): LogRow {
  const isBlocked = row.source === "blocked";
  return {
    at_ms: row.at_ms,
    time: timeFmt.format(row.at_ms),
    fullTime: dateTimeFmt.format(row.at_ms),
    client: row.client,
    name: displayName(row.name),
    type: recordTypeLabel(row.record_type),
    typeDesc: recordTypeDescription(row.record_type),
    source: row.source,
    sourceText: sourceLabel(row.source),
    isBlocked,
    responseCode: row.response_code,
    rcode: responseCodeLabel(row.response_code),
    rcodeDesc: responseCodeDescription(row.response_code),
    rcodeTone: rcodeTone(row.response_code, isBlocked),
    answers: row.answers,
    durationMs: row.duration_ms,
  };
}

/** Load the newest log entries for `query`, refreshing every couple of seconds
 *  while `live` is on. Entries already on screen stay put during a refetch, so
 *  the list never flashes empty between polls or while a new search resolves. */
export function useLogs(query: string, live: boolean): LogsState {
  const [state, setState] = useState<LogsState>({
    rows: null,
    error: null,
    loading: true,
  });

  useEffect(() => {
    let active = true;
    const controller = new AbortController();

    async function load() {
      try {
        const rows = await api.queries(LOG_LIMIT, query, controller.signal);
        if (!active) return;
        setState({ rows: rows.map(toLogRow), error: null, loading: false });
      } catch (error) {
        if (!active || controller.signal.aborted) return;
        setState((prev) => ({
          rows: prev.rows,
          error: error instanceof Error ? error.message : "Unbekannter Fehler",
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
  }, [query, live]);

  return state;
}
