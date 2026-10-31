// Typed client for the JustDNS JSON API. The Rust web crate serves these
// under the same origin as the UI, so the paths are relative.

export interface Stats {
  total: number;
  cache_hits: number;
  upstream: number;
  local: number;
  nxdomain: number;
  servfail: number;
  blocked: number;
  active_clients: number;
}

export interface ActivityBucket {
  hour_ms: number;
  count: number;
}

export type UpstreamMode = "racing" | "fallback" | "balanced" | "rules";

/// The settings the server persists. Field names match the Rust `Settings`
/// type (camelCase on the wire), so this is sent and received as-is.
export interface ServerSettings {
  blockAdsTrackers: boolean;
  safeSearch: boolean;
  blockMalware: boolean;
  upstreamMode: UpstreamMode;
  encryptUpstream: boolean;
  ipv6: boolean;
  queryLog: boolean;
  logRetentionDays: number;
  cacheServeStale: boolean;
}

export type QuerySource = "cache" | "upstream" | "local";

export interface QueryRow {
  at_ms: number;
  client: string;
  name: string;
  record_type: number;
  response_code: number;
  source: QuerySource;
  duration_ms: number;
  answers: number;
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(path, { signal });
  if (!response.ok) {
    throw new Error(`${path} antwortete mit ${response.status}`);
  }
  return response.json() as Promise<T>;
}

async function putJson<T>(path: string, body: T, signal?: AbortSignal): Promise<T> {
  const response = await fetch(path, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal,
  });
  if (!response.ok) {
    throw new Error(`${path} antwortete mit ${response.status}`);
  }
  return response.json() as Promise<T>;
}

export const api = {
  stats: (signal?: AbortSignal) => getJson<Stats>("/api/stats", signal),
  activity: (signal?: AbortSignal) => getJson<ActivityBucket[]>("/api/activity", signal),
  queries: (limit = 50, query = "", signal?: AbortSignal) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (query.trim()) params.set("q", query.trim());
    return getJson<QueryRow[]>(`/api/queries?${params}`, signal);
  },
  settings: (signal?: AbortSignal) => getJson<ServerSettings>("/api/settings", signal),
  saveSettings: (settings: ServerSettings, signal?: AbortSignal) =>
    putJson<ServerSettings>("/api/settings", settings, signal),
};
