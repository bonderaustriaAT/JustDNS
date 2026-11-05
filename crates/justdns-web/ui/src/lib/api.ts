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

/** One client and how many queries it made over the last 24 hours, with its
 *  device name resolved when one is set. */
export interface ClientCount {
  client: string;
  count: number;
  /** The device name for this address, when a device matches it. */
  name?: string;
}

/** Last-24h figures for the overview's statistics. The live query rate is
 *  derived on the client from successive `stats` polls, not part of this. */
export interface DayStats {
  total: number;
  blocked: number;
  /** Handling time across every query (cache hits included, so a cache-heavy day
   *  reads fast); median and p95 keep one slow outlier from skewing it. */
  avg_duration_ms: number;
  median_duration_ms: number;
  p95_duration_ms: number;
  top_clients: ClientCount[];
}

export type UpstreamMode = "racing" | "fallback" | "balanced" | "rules";

/// The speed/privacy profile: a single switch that picks the resolvers and how
/// eagerly answers are cached. Matches the Rust `SpeedProfile`.
export type SpeedProfile = "privacy" | "balanced" | "speed";

/// How a user-defined resolver is reached. Matches the Rust `CustomTransport`.
export type CustomTransport = "plain" | "dot" | "doh";

/// What a blocked name is answered with. Matches the Rust `BlockResponse`.
/// "null-ip" hands back 0.0.0.0 / :: (default); "nx-domain" says the name does
/// not exist.
export type BlockResponse = "null-ip" | "nx-domain";

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
  /// A display name for this instance, shown in the header.
  instanceName: string;
  /// What a blocked name is answered with: the null IP (default) or NXDOMAIN.
  blockResponse: BlockResponse;
  /// How many seconds since a client's last query still counts it as "active"
  /// on the devices view.
  activeWindowSecs: number;
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
  /** The device name of the client that asked, when a device matches it. */
  device_name?: string;
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

/** A reusable address object: a short name standing for one or more IP
 *  addresses. A rewrite can point at it instead of repeating the address, and
 *  changing the object updates every rewrite that references it. */
export interface Alias {
  name: string;
  ips: string[];
}

/** A local DNS rewrite: an exact name the resolver answers itself from fixed
 *  IP addresses (v4 → A record, v6 → AAAA) and/or the addresses of the address
 *  objects it references, instead of forwarding it upstream. A disabled rewrite
 *  stays in the list but is not answered locally. */
export interface Rewrite {
  name: string;
  ips: string[];
  /** Names of the address objects this rewrite points at. */
  aliases: string[];
  enabled: boolean;
}

/** Everything the rewrites view shows in one payload. */
export interface RewritesView {
  rewrites: Rewrite[];
  aliases: Alias[];
}

/** The kind of device a name stands for, shown as an icon. Matches the Rust
 *  `DeviceKind`. */
export type DeviceKind =
  | "phone"
  | "laptop"
  | "desktop"
  | "tablet"
  | "tv"
  | "console"
  | "iot"
  | "server"
  | "printer"
  | "router"
  | "other";

/** The filtering delta a scope (group or device) layers on what it inherits
 *  (global → group → device). Every field is additive except `listsDisabled`,
 *  which switches an inherited list off. List references are the ids from the
 *  catalogue (`ListObject.id`): a feed subscription id, or `ads`/`malware`.
 *  Matches the Rust `PolicyDelta`. */
export interface PolicyDelta {
  /** List ids switched on in addition to what is inherited. */
  listsAdded: string[];
  /** Inherited list ids switched off for this scope. */
  listsDisabled: string[];
  /** Domains this scope always allows (an allow wins over every block). */
  allow: string[];
  /** Domains this scope always blocks. */
  block: string[];
}

/** An empty delta, for a fresh scope. */
export function emptyDelta(): PolicyDelta {
  return { listsAdded: [], listsDisabled: [], allow: [], block: [] };
}

/** A device's policy as the client holds it while editing: its group (or null
 *  for ungrouped) and its own delta. Sent to the server flattened, matching the
 *  Rust `DevicePolicyInput`. */
export interface DevicePolicy {
  group: string | null;
  delta: PolicyDelta;
}

/** Flatten a `DevicePolicy` into the wire shape the server expects: the group
 *  alongside the delta's own fields. */
function devicePolicyBody(policy: DevicePolicy): Record<string, unknown> {
  return { group: policy.group, ...policy.delta };
}

/** A named group of devices with its own delta. Devices reference it by `id`;
 *  the delta applies between the global default and the device's own delta.
 *  Matches the Rust `Group`. */
export interface Group {
  id: string;
  name: string;
  policy: PolicyDelta;
}

/** One switchable block object the policy editor offers: the two built-ins
 *  (`ads`, `malware`) and every block feed. `globalOn` is its state in the
 *  global default, so a scope that neither adds nor disables it inherits this. */
export interface ListObject {
  id: string;
  label: string;
  globalOn: boolean;
}

/** A named client (device): the address or CIDR range it matches, a display
 *  name, an optional note, a kind, its group and its own policy delta. Matches
 *  the Rust `Device`. */
export interface Device {
  target: string;
  name: string;
  note?: string;
  kind: DeviceKind;
  group?: string;
  policy: PolicyDelta;
}

/** One row of the devices view: a named device or a seen-but-unnamed client,
 *  with its recent footprint. For a named device `target` is its address or
 *  range and `name`/`note`/`kind` describe it; for an unnamed client `target`
 *  is the bare address and `name` is absent. `known` tells the two apart.
 *  `group`/`policy` are present only for named devices. */
export interface ClientRow {
  target: string;
  isRange: boolean;
  name?: string;
  note?: string;
  kind: DeviceKind;
  known: boolean;
  count: number;
  lastSeenMs?: number;
  active: boolean;
  group?: string;
  policy?: PolicyDelta;
}

/** Everything the devices view shows: the device rows, the groups, the list
 *  catalogue the policy editor draws from, and the active-window the rows were
 *  aged by (seconds since last query that still counts as active). */
export interface ClientsView {
  clients: ClientRow[];
  groups: Group[];
  lists: ListObject[];
  activeWindowSecs: number;
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
  statsDay: (signal?: AbortSignal) => getJson<DayStats>("/api/stats/day", signal),
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
  /** Add one or more names that share the same addresses and/or objects. */
  setRewrites: (names: string[], ips: string[], aliases: string[]) =>
    sendJson<RewritesView>("POST", "/api/rewrites", { names, ips, aliases }),
  /** Rename a rewrite and replace its addresses and object references, keeping
   *  its enabled state. */
  editRewrite: (oldName: string, name: string, ips: string[], aliases: string[]) =>
    sendJson<RewritesView>("PUT", "/api/rewrites", { oldName, name, ips, aliases }),
  /** Switch a rewrite on or off. */
  setRewriteEnabled: (name: string, enabled: boolean) =>
    sendJson<RewritesView>("PUT", "/api/rewrites/enabled", { name, enabled }),
  removeRewrite: (name: string) =>
    sendJson<RewritesView>("DELETE", "/api/rewrites", { name }),
  /** Add an address object, or replace the addresses of one that exists. */
  setAlias: (name: string, ips: string[]) =>
    sendJson<RewritesView>("POST", "/api/aliases", { name, ips }),
  /** Rename an address object and replace its addresses; references follow. */
  editAlias: (oldName: string, name: string, ips: string[]) =>
    sendJson<RewritesView>("PUT", "/api/aliases", { oldName, name, ips }),
  /** Remove an address object (refused while a rewrite still references it). */
  removeAlias: (name: string) =>
    sendJson<RewritesView>("DELETE", "/api/aliases", { name }),
  /** The devices view: named devices plus recently seen unnamed clients, the
   *  groups and the list catalogue the policy editor draws from. */
  clients: (signal?: AbortSignal) => getJson<ClientsView>("/api/clients", signal),
  /** Add a device, or replace the name/note/kind of one with the same target.
   *  When `policy` is given, the device's group and delta are applied too. */
  setClient: (
    target: string,
    name: string,
    kind: DeviceKind,
    note?: string,
    policy?: DevicePolicy,
  ) =>
    sendJson<ClientsView>("POST", "/api/clients", {
      target,
      name,
      kind,
      note,
      policy: policy && devicePolicyBody(policy),
    }),
  /** Re-point a device at a new target and replace its name/note/kind. When
   *  `policy` is given, the device's group and delta are applied too. */
  editClient: (
    oldTarget: string,
    target: string,
    name: string,
    kind: DeviceKind,
    note?: string,
    policy?: DevicePolicy,
  ) =>
    sendJson<ClientsView>("PUT", "/api/clients", {
      oldTarget,
      target,
      name,
      kind,
      note,
      policy: policy && devicePolicyBody(policy),
    }),
  /** Remove a device by its target. */
  removeClient: (target: string) =>
    sendJson<ClientsView>("DELETE", "/api/clients", { target }),
  /** Create a group with a name and delta. */
  addGroup: (name: string, policy: PolicyDelta) =>
    sendJson<ClientsView>("POST", "/api/groups", { name, ...policy }),
  /** Rename a group and replace its delta, keyed by its id. */
  editGroup: (id: string, name: string, policy: PolicyDelta) =>
    sendJson<ClientsView>("PUT", "/api/groups", { id, name, ...policy }),
  /** Delete a group; its devices become ungrouped. */
  removeGroup: (id: string) => sendJson<ClientsView>("DELETE", "/api/groups", { id }),
  /** Drop every entry in the DNS response cache. */
  clearCache: () => sendJson<void>("POST", "/api/cache/clear"),
  /** Download the full configuration backup (settings, lists and rewrites) as a
   *  Blob the caller saves to a file. */
  downloadBackup: async (): Promise<Blob> => {
    const response = await fetch("/api/backup");
    if (!response.ok) {
      throw failed("/api/backup", response.status);
    }
    return response.blob();
  },
  /** Restore a backup file's parsed JSON, replacing the current settings, lists
   *  and rewrites. The server refuses a file that is not a JustDNS backup. */
  restoreBackup: (backup: unknown) => sendJson<void>("POST", "/api/backup/restore", backup),
  authState: (signal?: AbortSignal) => getJson<AuthState>("/api/auth/state", signal),
  setup: (username: string, password: string) =>
    postJson<{ username: string }>("/api/auth/setup", { username, password }),
  login: (username: string, password: string) =>
    postJson<{ username: string }>("/api/auth/login", { username, password }),
  logout: () => postJson<void>("/api/auth/logout", {}),
};
