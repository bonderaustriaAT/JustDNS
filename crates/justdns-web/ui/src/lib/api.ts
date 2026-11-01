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

/// The speed/privacy profile: a single switch that picks the resolvers and how
/// eagerly answers are cached. Matches the Rust `SpeedProfile`.
export type SpeedProfile = "privacy" | "balanced" | "speed";

/// How hard JustDNS blocks, as one plain choice. Matches the Rust `BlockLevel`.
/// "custom" means the active lists match no level (hand-tuned).
export type BlockLevel = "off" | "minimal" | "standard" | "extreme" | "custom";

/// How a user-defined resolver is reached. Matches the Rust `CustomTransport`.
export type CustomTransport = "plain" | "dot" | "doh";

/// A resolver the user entered by hand. Matches the Rust `CustomUpstream`;
/// stored as typed so a half-finished entry survives editing.
export interface CustomUpstream {
  address: string;
  port: number | null;
  transport: CustomTransport;
  serverName: string | null;
}

/// The settings the server persists. Field names match the Rust `Settings`
/// type (camelCase on the wire), so this is sent and received as-is.
export interface ServerSettings {
  blockAdsTrackers: boolean;
  safeSearch: boolean;
  blockMalware: boolean;
  upstreamMode: UpstreamMode;
  encryptUpstream: boolean;
  speedProfile: SpeedProfile;
  customUpstreamsEnabled: boolean;
  customUpstreams: CustomUpstream[];
  ipv6: boolean;
  queryLog: boolean;
  logRetentionDays: number;
  /// The blocking strength as one plain choice; "custom" when the active lists
  /// match no level. Changed via `setBlockLevel`, not a plain settings save.
  blockLevel: BlockLevel;
  /// A display name for this instance, shown in the header.
  instanceName: string;
}

export type QuerySource = "cache" | "upstream" | "local" | "rewrite" | "blocked";

export interface QueryRow {
  at_ms: number;
  client: string;
  name: string;
  record_type: number;
  response_code: number;
  source: QuerySource;
  duration_ms: number;
  answers: number;
  /** The IPs the query resolved to, in answer order. Empty for a block, a
   *  NODATA answer, or a non-address record type. */
  addresses: string[];
}

/** Which list a name is on and the rule that matched, or null if on no list. */
export type BlockList = "ads" | "malware" | "custom";

export interface ClassifyResult {
  list: BlockList;
  rule: string;
  /** For a custom block, the exact list that matched (own list or a feed). */
  name?: string;
}

/** Whether a list adds names to the allow set or the block set. */
export type ListKind = "allow" | "block";

/** A subscription to an external feed, with the status of its last fetch. */
export interface Subscription {
  id: string;
  name: string;
  url: string;
  kind: ListKind;
  enabled: boolean;
  /** Set when this feed was switched on as a curated preset. */
  preset?: string;
  lastFetchedMs?: number;
  lastCount?: number;
  lastError?: string;
}

/** One curated catalogue entry, with its live on/off state. */
export interface Preset {
  key: string;
  name: string;
  url: string;
  kind: ListKind;
  description: string;
  falsePositive: string;
  /** False-positive risk on a 1 (negligible) to 5 (high) scale. */
  fpLevel: number;
  /** Coarse grouping for the catalogue filter. */
  category: "ads" | "security" | "privacy" | "bypass" | "allow";
  active: boolean;
  enabled: boolean;
}

/** A local DNS rewrite: an exact name the resolver answers itself from fixed
 *  IP addresses (v4 → A record, v6 → AAAA) instead of forwarding it upstream.
 *  A disabled rewrite stays in the list but is not answered locally. */
export interface Rewrite {
  name: string;
  ips: string[];
  enabled: boolean;
}

/** Everything the rewrites view shows in one payload. */
export interface RewritesView {
  rewrites: Rewrite[];
}

/** One of the user's own allow/block entries: a domain, with an optional
 *  free-text note explaining why it is on the list. The note is metadata only —
 *  it never changes what is resolved. */
export interface ListEntry {
  domain: string;
  note?: string;
}

/** Everything the "Listen" view shows in one payload. */
export interface ListsView {
  allow: ListEntry[];
  block: ListEntry[];
  subscriptions: Subscription[];
  presets: Preset[];
  allowActive: number;
  blockActive: number;
}

/** The outcome of refreshing one feed. */
export interface RefreshOutcome {
  id: string;
  count?: number;
  error?: string;
}

export interface RefreshResponse {
  outcomes: RefreshOutcome[];
  lists: ListsView;
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

/** Send a guarded mutation (POST/PUT/DELETE). A 401 bounces to login like the
 * read calls; other failures carry the server's message so the UI can show it. */
async function sendJson<Res>(method: string, path: string, body?: unknown): Promise<Res> {
  const response = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    if (response.status === 401) onUnauthorized?.();
    const detail = await response.text().catch(() => "");
    throw new ApiError(response.status, detail || `${path} antwortete mit ${response.status}`);
  }
  return (response.status === 204 ? undefined : await response.json()) as Res;
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
  /** Apply a block level: the server toggles the matching feeds and switches,
   *  stores the level, and returns the fresh settings and lists. "custom" is not
   *  applicable and is rejected. */
  setBlockLevel: (level: BlockLevel) =>
    sendJson<{ settings: ServerSettings; lists: ListsView }>(
      "PUT",
      "/api/settings/block-level",
      { level },
    ),
  classify: (name: string, signal?: AbortSignal) => {
    const params = new URLSearchParams({ name });
    return getJson<ClassifyResult | null>(`/api/classify?${params}`, signal);
  },
  lists: (signal?: AbortSignal) => getJson<ListsView>("/api/lists", signal),
  /** Add an allow entry, optionally with a note. Re-adding an existing domain
   *  keeps it and only sets the note when one is given. */
  addAllow: (domain: string, note?: string) =>
    sendJson<ListsView>("POST", "/api/lists/allow", { domain, note }),
  removeAllow: (domain: string) => sendJson<ListsView>("DELETE", "/api/lists/allow", { domain }),
  /** Set (or, with an empty string, clear) the note on an existing allow entry. */
  setAllowNote: (domain: string, note: string) =>
    sendJson<ListsView>("PUT", "/api/lists/allow", { domain, note }),
  addBlock: (domain: string, note?: string) =>
    sendJson<ListsView>("POST", "/api/lists/block", { domain, note }),
  removeBlock: (domain: string) => sendJson<ListsView>("DELETE", "/api/lists/block", { domain }),
  /** Set (or, with an empty string, clear) the note on an existing block entry. */
  setBlockNote: (domain: string, note: string) =>
    sendJson<ListsView>("PUT", "/api/lists/block", { domain, note }),
  addSubscription: (name: string, url: string, kind: ListKind) =>
    sendJson<ListsView>("POST", "/api/lists/subscriptions", { name, url, kind }),
  setSubscriptionEnabled: (id: string, enabled: boolean) =>
    sendJson<ListsView>("PUT", `/api/lists/subscriptions/${encodeURIComponent(id)}`, { enabled }),
  removeSubscription: (id: string) =>
    sendJson<ListsView>("DELETE", `/api/lists/subscriptions/${encodeURIComponent(id)}`),
  setPreset: (key: string, on: boolean) =>
    sendJson<ListsView>("PUT", `/api/lists/presets/${encodeURIComponent(key)}`, { on }),
  refreshLists: () => sendJson<RefreshResponse>("POST", "/api/lists/refresh"),
  rewrites: (signal?: AbortSignal) => getJson<RewritesView>("/api/rewrites", signal),
  /** Add one or more names that share the same addresses. */
  setRewrites: (names: string[], ips: string[]) =>
    sendJson<RewritesView>("POST", "/api/rewrites", { names, ips }),
  /** Rename a rewrite and replace its addresses, keeping its enabled state. */
  editRewrite: (oldName: string, name: string, ips: string[]) =>
    sendJson<RewritesView>("PUT", "/api/rewrites", { oldName, name, ips }),
  /** Switch a rewrite on or off. */
  setRewriteEnabled: (name: string, enabled: boolean) =>
    sendJson<RewritesView>("PUT", "/api/rewrites/enabled", { name, enabled }),
  removeRewrite: (name: string) =>
    sendJson<RewritesView>("DELETE", "/api/rewrites", { name }),
  /** Drop every entry in the DNS response cache. */
  clearCache: () => sendJson<void>("POST", "/api/cache/clear"),
  authState: (signal?: AbortSignal) => getJson<AuthState>("/api/auth/state", signal),
  setup: (username: string, password: string) =>
    postJson<{ username: string }>("/api/auth/setup", { username, password }),
  login: (username: string, password: string) =>
    postJson<{ username: string }>("/api/auth/login", { username, password }),
  logout: () => postJson<void>("/api/auth/logout", {}),
};
