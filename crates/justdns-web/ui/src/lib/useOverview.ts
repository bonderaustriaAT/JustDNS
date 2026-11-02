import { useEffect, useRef, useState } from "react";

import { api, type ClientCount, type QueryRow } from "./api";
import { displayName, recordTypeLabel, sourceLabel } from "./dns";
import { useI18n, type Lang } from "./i18n";

const HOUR_MS = 3_600_000;
const ACTIVITY_HOURS = 24;
const REFRESH_MS = 5_000;
const RECENT_LIMIT = 8;
/** Weight of the newest sample in the live-rate EMA (the rest carries the
 *  previous value), so the figure steadies instead of jumping each poll. */
const RATE_SMOOTHING = 0.5;

export interface RecentQuery {
  name: string;
  client: string;
  type: string;
  source: string;
  secondsAgo: number;
}

export interface DayFigures {
  total: number;
  blocked: number;
  avgDurationMs: number;
}

export interface OverviewData {
  total: number;
  blocked: number;
  cacheRate: number;
  activeClients: number;
  /** Queries per hour over the last 24 hours, oldest first. */
  activity: number[];
  recent: RecentQuery[];
  /** Smoothed query rate per second, or null until two samples are in. */
  liveRatePerSec: number | null;
  /** Last-24h totals. */
  day: DayFigures;
  /** Busiest clients over the last 24h, most queries first. */
  topClients: ClientCount[];
}

export interface OverviewState {
  data: OverviewData | null;
  error: string | null;
  loading: boolean;
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

function toRecent(rows: QueryRow[], lang: Lang): RecentQuery[] {
  const now = Date.now();
  return rows.slice(0, RECENT_LIMIT).map((row) => ({
    name: displayName(row.name),
    client: row.client,
    type: recordTypeLabel(row.record_type),
    source: sourceLabel(row.source, lang),
    secondsAgo: Math.max(0, Math.round((now - row.at_ms) / 1000)),
  }));
}

export function useOverview(): OverviewState {
  const { lang, t } = useI18n();
  const [state, setState] = useState<OverviewState>({
    data: null,
    error: null,
    loading: true,
  });

  // The live rate is the change in the cumulative total between two polls. The
  // previous sample and the smoothed rate ride in refs so they survive re-renders
  // but reset when the effect re-runs (e.g. on a language switch).
  const sampleRef = useRef<{ total: number; at: number } | null>(null);
  const rateRef = useRef<number | null>(null);

  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    sampleRef.current = null;
    rateRef.current = null;

    async function load() {
      try {
        const [stats, statsDay, activity, queries] = await Promise.all([
          api.stats(controller.signal),
          api.statsDay(controller.signal),
          api.activity(controller.signal),
          api.queries(RECENT_LIMIT, "", controller.signal),
        ]);
        if (!active) return;

        // Derive the per-second rate from the delta since the last sample, then
        // smooth it so the gauge does not jump; null until a second sample is in.
        const now = Date.now();
        const prev = sampleRef.current;
        let liveRatePerSec: number | null = null;
        if (prev && now > prev.at) {
          const instant = Math.max(0, stats.total - prev.total) / ((now - prev.at) / 1000);
          const smoothed =
            rateRef.current == null
              ? instant
              : rateRef.current * (1 - RATE_SMOOTHING) + instant * RATE_SMOOTHING;
          rateRef.current = smoothed;
          liveRatePerSec = smoothed;
        }
        sampleRef.current = { total: stats.total, at: now };

        setState({
          data: {
            total: stats.total,
            blocked: stats.blocked,
            cacheRate: stats.total === 0 ? 0 : Math.round((stats.cache_hits / stats.total) * 100),
            activeClients: stats.active_clients,
            activity: denseActivity(activity),
            recent: toRecent(queries, lang),
            liveRatePerSec,
            day: {
              total: statsDay.total,
              blocked: statsDay.blocked,
              avgDurationMs: statsDay.avg_duration_ms,
            },
            topClients: statsDay.top_clients,
          },
          error: null,
          loading: false,
        });
      } catch (error) {
        if (!active || controller.signal.aborted) return;
        setState((prev) => ({
          data: prev.data,
          error: error instanceof Error ? error.message : t("common.unknownError"),
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
    // `lang` is in the deps so the precomputed source/type labels on the recent
    // rows are rebuilt in the new language when the user switches.
  }, [lang, t]);

  return state;
}
