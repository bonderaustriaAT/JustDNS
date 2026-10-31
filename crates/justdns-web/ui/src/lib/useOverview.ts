import { useEffect, useState } from "react";

import { api, type QueryRow, type QuerySource } from "./api";

const HOUR_MS = 3_600_000;
const ACTIVITY_HOURS = 24;
const REFRESH_MS = 5_000;
const RECENT_LIMIT = 8;

/** Numeric DNS record types we expect to show; anything else falls back to its
 *  numeric form so an unusual query is still legible. */
const RECORD_TYPES: Record<number, string> = {
  1: "A",
  2: "NS",
  5: "CNAME",
  6: "SOA",
  12: "PTR",
  15: "MX",
  16: "TXT",
  28: "AAAA",
  33: "SRV",
  64: "SVCB",
  65: "HTTPS",
};

const SOURCE_LABELS: Record<QuerySource, string> = {
  cache: "Cache",
  upstream: "Upstream",
  local: "Lokal",
};

export interface RecentQuery {
  name: string;
  client: string;
  type: string;
  source: string;
  secondsAgo: number;
}

export interface OverviewData {
  total: number;
  cacheRate: number;
  activeClients: number;
  /** Queries per hour over the last 24 hours, oldest first. */
  activity: number[];
  recent: RecentQuery[];
}

export interface OverviewState {
  data: OverviewData | null;
  error: string | null;
  loading: boolean;
}

function typeLabel(code: number): string {
  return RECORD_TYPES[code] ?? `TYPE${code}`;
}

/** Spread the active-hour buckets across a dense, 24-slot series ending at the
 *  current hour, filling quiet hours with zero. Hours are floored on the epoch
 *  to match how the store groups them, sidestepping any timezone skew. */
function denseActivity(buckets: { hour_ms: number; count: number }[]): number[] {
  const currentHour = Math.floor(Date.now() / HOUR_MS) * HOUR_MS;
  const counts = new Map(buckets.map((b) => [b.hour_ms, b.count]));
  const series: number[] = [];
  for (let i = ACTIVITY_HOURS - 1; i >= 0; i -= 1) {
    series.push(counts.get(currentHour - i * HOUR_MS) ?? 0);
  }
  return series;
}

function toRecent(rows: QueryRow[]): RecentQuery[] {
  const now = Date.now();
  return rows.slice(0, RECENT_LIMIT).map((row) => ({
    name: row.name.replace(/\.$/, ""),
    client: row.client,
    type: typeLabel(row.record_type),
    source: SOURCE_LABELS[row.source] ?? row.source,
    secondsAgo: Math.max(0, Math.round((now - row.at_ms) / 1000)),
  }));
}

export function useOverview(): OverviewState {
  const [state, setState] = useState<OverviewState>({
    data: null,
    error: null,
    loading: true,
  });

  useEffect(() => {
    let active = true;
    const controller = new AbortController();

    async function load() {
      try {
        const [stats, activity, queries] = await Promise.all([
          api.stats(controller.signal),
          api.activity(controller.signal),
          api.queries(RECENT_LIMIT, controller.signal),
        ]);
        if (!active) return;
        setState({
          data: {
            total: stats.total,
            cacheRate: stats.total === 0 ? 0 : Math.round((stats.cache_hits / stats.total) * 100),
            activeClients: stats.active_clients,
            activity: denseActivity(activity),
            recent: toRecent(queries),
          },
          error: null,
          loading: false,
        });
      } catch (error) {
        if (!active || controller.signal.aborted) return;
        setState((prev) => ({
          data: prev.data,
          error: error instanceof Error ? error.message : "Unbekannter Fehler",
          loading: false,
        }));
      }
    }

    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => {
      active = false;
      controller.abort();
      clearInterval(timer);
    };
  }, []);

  return state;
}
