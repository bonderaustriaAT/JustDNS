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

export type QuerySource = "cache" | "upstream" | "local" | "blocked";

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

/** Which list a name is on and the rule that matched, or null if on no list. */
export type BlockList = "ads" | "malware";

export interface ClassifyResult {
  list: BlockList;
  rule: string;
}

/** Whether an admin exists yet and who, if anyone, the caller is logged in as. */
export interface AuthState {
  configured: boolean;
  authenticated: boolean;
  username: string | null;
}

/** An API call that failed, carrying the HTTP status so callers can branch. */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

// A session can expire while the dashboard is open. Any guarded call that comes
// back 401 notifies this handler, so the app can drop back to the login screen
// instead of leaving broken widgets on the page.
let onUnauthorized: (() => void) | null = null;
export function setUnauthorizedHandler(handler: (() => void) | null): void {
  onUnauthorized = handler;
}

function failed(path: string, status: number): ApiError {
  if (status === 401) onUnauthorized?.();
  return new ApiError(status, `${path} antwortete mit ${status}`);
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(path, { signal });
  if (!response.ok) {
    throw failed(path, response.status);
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
    throw failed(path, response.status);
  }
  return response.json() as Promise<T>;
}

/** POST JSON without expecting a 401 to bounce the user (used by the auth
 * endpoints themselves, where a 401 means "wrong password", not "logged out"). */
async function postJson<Res>(path: string, body: unknown): Promise<Res> {
  const response = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new ApiError(response.status, `${path} antwortete mit ${response.status}`);
  }
  // 204 No Content (logout) has no body to parse.
  return (response.status === 204 ? undefined : await response.json()) as Res;
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
  classify: (name: string, signal?: AbortSignal) => {
    const params = new URLSearchParams({ name });
    return getJson<ClassifyResult | null>(`/api/classify?${params}`, signal);
  },
  authState: (signal?: AbortSignal) => getJson<AuthState>("/api/auth/state", signal),
  setup: (username: string, password: string) =>
    postJson<{ username: string }>("/api/auth/setup", { username, password }),
  login: (username: string, password: string) =>
    postJson<{ username: string }>("/api/auth/login", { username, password }),
  logout: () => postJson<void>("/api/auth/logout", {}),
};
