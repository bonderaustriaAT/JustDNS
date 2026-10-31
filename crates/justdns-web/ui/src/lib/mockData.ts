// Placeholder figures until the DNS core and web API are wired up. Shaped like
// the real data the overview will show so the UI need not change later.

export interface OverviewData {
  blockedToday: number;
  queriesToday: number;
  activeClients: number;
  /** Queries per hour over the last 24 hours, oldest first. */
  activity: number[];
  recentBlocks: BlockedQuery[];
}

export interface BlockedQuery {
  domain: string;
  client: string;
  minutesAgo: number;
  list: string;
}

export const overview: OverviewData = {
  blockedToday: 12480,
  queriesToday: 48213,
  activeClients: 9,
  activity: [
    180, 120, 90, 60, 50, 70, 140, 320, 540, 610, 580, 620, 700, 660, 590, 640,
    720, 810, 760, 540, 430, 360, 280, 210,
  ],
  recentBlocks: [
    { domain: "ads.doubleclick.net", client: "wohnzimmer-tv", minutesAgo: 1, list: "Werbung" },
    { domain: "analytics.tiktok.com", client: "pixel-7", minutesAgo: 2, list: "Tracker" },
    { domain: "graph.facebook.com", client: "macbook-air", minutesAgo: 4, list: "Tracker" },
    { domain: "telemetry.microsoft.com", client: "arbeits-pc", minutesAgo: 6, list: "Telemetrie" },
    { domain: "app-measurement.com", client: "pixel-7", minutesAgo: 9, list: "Tracker" },
    { domain: "ssl.google-analytics.com", client: "macbook-air", minutesAgo: 12, list: "Tracker" },
  ],
};

export function blockRate(data: OverviewData): number {
  if (data.queriesToday === 0) return 0;
  return Math.round((data.blockedToday / data.queriesToday) * 100);
}
