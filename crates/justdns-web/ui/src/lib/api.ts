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

/** One domain a client looked up and how often, over the last 24 hours. The
 *  `domain` is fully qualified as the query log stores it (trailing dot); the UI
 *  trims it for display. */
export interface DomainCount {
  domain: string;
  count: number;
}

/** One upstream resolver's performance over the last 24 hours: how many answers
 *  it gave and its handling-time figures. Only forwarded queries count — cache
 *  hits, blocks and local answers never reach a resolver. */
export interface ResolverStat {
  /** The resolver's label — its DoT/DoH certificate name, or plain IP. */
  resolver: string;
  count: number;
  avg_duration_ms: number;
  median_duration_ms: number;
  p95_duration_ms: number;
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
  /// The IANA time zone schedules are evaluated in (e.g. "Europe/Vienna"), so a
  /// window like "22:00–07:00" means local time across daylight saving.
  timezone: string;
  /// When on, every account must enrol a second factor — the global counterpart
  /// to a user's own `mfaRequired` flag. Enforced server-side as the OR of the
  /// two, so turning this on requires two-factor for everyone at once.
  requireMfa: boolean;
  /// Scheduled-report cadence. Both off by default; see `ReportSettings`.
  reports: ReportSettings;
  /// The custom block page. Off by default; see `BlockPageSettings`.
  blockPage: BlockPageSettings;
  /// Active notifications (webhook alerts). Off by default; see
  /// `NotificationSettings`.
  notifications: NotificationSettings;
  /// Bearer token that guards the Prometheus `GET /metrics` endpoint. Empty leaves
  /// the endpoint off. A write-only secret: blanked for non-admin readers.
  metricsToken: string;
}

/// When the daily and weekly activity reports are generated. Both cadences are
/// off by default and configured on the Reports page; the timezone comes from
/// `timezone`. Mirrors the Rust `ReportSettings` (camelCase on the wire). */
export interface ReportSettings {
  dailyEnabled: boolean;
  /** Local hour (0–23) the daily report runs at, covering the previous day. */
  dailyHour: number;
  weeklyEnabled: boolean;
  /** Weekday the weekly report runs on, 1 = Monday … 7 = Sunday. */
  weeklyWeekday: number;
  /** Local hour (0–23) the weekly report runs at, covering the previous week. */
  weeklyHour: number;
}

/** Default report cadence, mirroring the Rust `ReportSettings::default`: both
 *  off, at 06:00, weekly on Monday. Used to backfill an older server that does
 *  not yet send the field. */
export const DEFAULT_REPORT_SETTINGS: ReportSettings = {
  dailyEnabled: false,
  dailyHour: 6,
  weeklyEnabled: false,
  weeklyWeekday: 1,
  weeklyHour: 6,
};

/** Which cadence a stored report belongs to. */
export type ReportPeriod = "daily" | "weekly";

/** One `(name, count)` row of a top-domains / top-blocked list. */
export interface TopName {
  name: string;
  count: number;
}

/** One row of the top-clients list, with the device name resolved when the
 *  report was generated. */
export interface TopClient {
  client: string;
  deviceName?: string;
  count: number;
}

/** The frozen summary a report carries: headline totals, the source breakdown,
 *  DNSSEC verdicts and the three top-lists. */
export interface ReportSummary {
  total: number;
  blocked: number;
  cache: number;
  upstream: number;
  local: number;
  rewrite: number;
  avgDurationMs: number;
  dnssecSecure: number;
  dnssecInsecure: number;
  dnssecBogus: number;
  dnssecIndeterminate: number;
  topDomains: TopName[];
  topBlocked: TopName[];
  topClients: TopClient[];
}

/** A full stored report: its cadence, the window it covers `[fromMs, toMs)`,
 *  when it was generated, and the summary. */
export interface Report {
  id: string;
  period: ReportPeriod;
  fromMs: number;
  toMs: number;
  generatedAtMs: number;
  summary: ReportSummary;
}

/** One compact row of the report index, without the full top-tables. */
export interface ReportListItem {
  id: string;
  period: ReportPeriod;
  fromMs: number;
  toMs: number;
  generatedAtMs: number;
  total: number;
  blocked: number;
}

/** The window the privacy dashboard attributes over. */
export type TrackerWindow = "day" | "week" | "month";

/** One device in a company's drill-down: how much it reached that company. */
export interface CompanyDevice {
  client: string;
  name?: string;
  contacts: number;
  blocked: number;
  allowed: number;
}

/** One name in a company's drill-down: how often that exact domain was looked up. */
export interface CompanyDomain {
  name: string;
  contacts: number;
  blocked: number;
  allowed: number;
}

/** One company a device (or the whole network) reached, with how many lookups
 *  went to it and how many of those the filter blocked versus let through. For a
 *  network-wide company, `devices` and `domains` carry its drill-down (which
 *  devices reached it, which names under it were looked up); for a single
 *  device's company they are omitted and default to empty. */
export interface CompanyContact {
  org: string;
  category: string;
  contacts: number;
  blocked: number;
  allowed: number;
  devices?: CompanyDevice[];
  domains?: CompanyDomain[];
}

/** One device's tracker contacts over the window, with its company breakdown.
 *  `name` is the device's friendly name when one is known. */
export interface DeviceTrackers {
  client: string;
  name?: string;
  contacts: number;
  blocked: number;
  allowed: number;
  companies: CompanyContact[];
}

/** The privacy dashboard payload: network-wide totals, the companies contacted
 *  most, and a per-device breakdown. */
export interface TrackerReport {
  window: TrackerWindow;
  fromMs: number;
  toMs: number;
  totalContacts: number;
  totalBlocked: number;
  totalAllowed: number;
  companies: CompanyContact[];
  devices: DeviceTrackers[];
}

/// How a stored certificate came to be. Matches the Rust `CertKind`: one JustDNS
/// minted itself (self-signed, with a local CA to install), or one the operator
/// uploaded.
export type CertKind = "auto" | "uploaded";

/// A stored certificate's derived facts, matching the Rust `CertInfo`. Every
/// field is always present. The feature selectors (DoT/DoH/block page) reference
/// a certificate by `id`.
export interface CertInfo {
  /** Stable id the feature settings store to select this certificate. */
  id: string;
  /** Human-readable name shown in the catalogue and the selectors. */
  name: string;
  kind: CertKind;
  /** The leaf certificate's subject (common name, best-effort). */
  subject: string;
  /** The subject alternative names the certificate is valid for. */
  sans: string[];
  /** Validity start, Unix milliseconds. */
  notBeforeMs: number;
  /** Validity end, Unix milliseconds. */
  notAfterMs: number;
  /** SHA-256 fingerprint of the leaf, colon-separated upper-case hex. */
  fingerprint: string;
  /** Whether the certificate is outside its validity window right now. */
  expired: boolean;
  /** Whether a downloadable local CA accompanies it (minted certs only). */
  hasCa: boolean;
}

/// A stored certificate plus which features currently select it, matching the
/// Rust `CertListItem`. `usedBy` holds feature ids: `dot`, `doh`, `blockPage`.
export interface CertListItem extends CertInfo {
  usedBy: string[];
}

/// One visitor's "ask to unblock this page", matching the Rust `AccessRequest`
/// (camelCase). Only `id`, `domain`, `clientIp` and `createdMs` are always
/// present; the rest can be absent when the device/group or block reason could
/// not be resolved, so every optional field is defaulted in the UI.
export interface AccessRequest {
  id: string;
  domain: string;
  clientIp: string;
  device?: string;
  group?: string;
  /// `"ads"`, `"malware"` or `"custom"` — the UI turns the slug into a label.
  blockCategory?: string;
  blockList?: string;
  blockRule?: string;
  reason?: string;
  createdMs: number;
}

/// How far a release reaches: just the device that asked, its whole group, or
/// everyone (the global allow list).
export type ReleaseScope = "device" | "group" | "all";

/// State of the encrypted client-DNS endpoints (DoT on 853, DoH on `/dns-query`).
/// Matches the Rust `TlsStatus`; every field is always present so the UI never
/// has to guard a missing value. `available` is false when TLS could not be
/// prepared, in which case the section is shown disabled. `dotCertId`/`dohCertId`
/// are the ids of the stored certificate each endpoint serves; an empty string
/// means the store default.
export interface TlsStatus {
  available: boolean;
  dotEnabled: boolean;
  dohEnabled: boolean;
  hostname: string;
  dotCertId: string;
  dohCertId: string;
  dotPort: number;
  dohPort: number;
}

/// The writable part of the encrypted-DNS settings, sent on a save. A blank
/// cert id selects the store default; the serde default on the server accepts an
/// omitted one.
export interface TlsSettings {
  dotEnabled: boolean;
  dohEnabled: boolean;
  hostname: string;
  dotCertId?: string;
  dohCertId?: string;
}

/// The custom block page's settings, matching the Rust `BlockPage`. When
/// `enabled`, blocked names resolve to `serverIpv4`/`serverIpv6` (empty = fall
/// back to the null IP / NXDOMAIN) and the HTTPS listener presents the stored
/// certificate `certId` selects (`null` = the store default).
export interface BlockPageSettings {
  enabled: boolean;
  serverIpv4: string;
  serverIpv6: string;
  certId: string | null;
  /// The landing URL for *hosted mode* (e.g. `https://justdns.justfuture.at/blockpage`).
  /// When set, a blocked domain that reaches the server is redirected here (HTTP
  /// 302) with the blocked host appended as `?d=`, and a request to this URL's
  /// own host is served the rendered page — so JustDNS hosts the page on a real
  /// hostname with a valid certificate. Empty = local mode (serve the built-in
  /// page in place). Optional + defaulted so an older server is backfilled.
  redirectUrl?: string;
  /// Run the block page behind a standalone reverse proxy (NGINX Proxy Manager,
  /// Traefik, …) that terminates TLS and forwards plain HTTP. When `true`, JustDNS
  /// serves HTTP only (no certificate of its own) and trusts the proxy's
  /// `X-Forwarded-Host` header to identify the landing host. Optional + defaulted
  /// so an older server is backfilled.
  behindReverseProxy?: boolean;
  title: string;
  message: string;
}

/// Default block-page settings, mirroring the Rust `BlockPage::default`: off,
/// no server IPs, store-default certificate, with the shipped German wording.
/// Used to backfill an older server that does not yet send the field.
export const DEFAULT_BLOCK_PAGE: BlockPageSettings = {
  enabled: false,
  serverIpv4: "",
  serverIpv6: "",
  certId: null,
  redirectUrl: "",
  behindReverseProxy: false,
  title: "Diese Seite ist gesperrt",
  message: "Der Zugriff auf diese Domain wurde durch JustDNS blockiert.",
};

/// Which shape a notification webhook payload takes, matching the Rust
/// `WebhookFormat` (lowercase on the wire).
export type WebhookFormat = "generic" | "ntfy" | "slack" | "discord" | "telegram";

/// Active-notification settings, matching the Rust `Notifications`. When
/// `enabled`, JustDNS posts a short message to `webhookUrl` (shaped by
/// `webhookFormat`) for each enabled event.
export interface NotificationSettings {
  enabled: boolean;
  /// The webhook target. For most formats the full URL to POST to; for
  /// `telegram` it is the bot token (the chat id is set separately).
  webhookUrl: string;
  webhookFormat: WebhookFormat;
  /// The Telegram chat id to deliver to (only used when `webhookFormat` is
  /// `telegram`).
  telegramChatId: string;
  onNodeDown: boolean;
  onUpdate: boolean;
  onCertExpiry: boolean;
  onDeviceSpike: boolean;
  certExpiryDays: number;
  spikeBlockedThreshold: number;
  spikeWindowMins: number;
}

/// Default notification settings, mirroring the Rust `Notifications::default`:
/// off, generic webhook, deterministic events on and the heuristic off. Used to
/// backfill an older server that does not yet send the field.
export const DEFAULT_NOTIFICATIONS: NotificationSettings = {
  enabled: false,
  webhookUrl: "",
  webhookFormat: "generic",
  telegramChatId: "",
  onNodeDown: true,
  onUpdate: true,
  onCertExpiry: true,
  onDeviceSpike: false,
  certExpiryDays: 14,
  spikeBlockedThreshold: 100,
  spikeWindowMins: 10,
};

export type QuerySource = "cache" | "upstream" | "local" | "rewrite" | "blocked";

/** The DNSSEC verdict recorded for a forwarded query when validation is on:
 *  `secure` (a valid chain of trust), `insecure` (an unsigned zone, correctly
 *  proven), `bogus` (signatures that failed to validate — the answer was refused
 *  with SERVFAIL), or `indeterminate` (no verdict could be reached). Absent when
 *  validation is off or the answer did not come from upstream. Matches the Rust
 *  `DnssecStatus` (snake_case on the wire). */
export type DnssecStatus = "secure" | "insecure" | "bogus" | "indeterminate";

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
  /** The DNSSEC verdict, present only when validation recorded one. */
  dnssec?: DnssecStatus;
  /** The node this row came from in a high-availability cluster: absent for this
   *  node's own queries, a peer's node id for one pulled over the mesh. */
  origin?: string;
}

/** One cluster node as the explorer shows it: the stable id, the host a person
 *  recognises it by (empty when it has not advertised an address), and whether
 *  it is the node serving this UI. */
export interface NodeInfo {
  id: string;
  label: string;
  self: boolean;
}

/** The cluster roster resolved to display labels for the query-log explorer. */
export interface NodesResponse {
  selfId: string;
  nodes: NodeInfo[];
}

/** A keyset-pagination cursor into the query history: the `(at_ms, rowid)` of
 *  the last row of a page, handed back so "load older" can continue past it. */
export interface HistoryCursor {
  atMs: number;
  row: number;
}

/** The facet filters for the query-log explorer. All are optional: an empty
 *  filter (with `q` blank and no facets) returns the newest slice of the whole
 *  history. `sources` is OR-combined; a `null` select means "any". `startMs` /
 *  `endMs` are epoch-millisecond bounds, inclusive. */
export interface HistoryFilter {
  sources: QuerySource[];
  recordType: number | null;
  responseCode: number | null;
  resolver: string | null;
  /** Filter to one named device, by its `target` (address or CIDR range). */
  device: string | null;
  /** Filter to one device group, by its stable `id`. `device` wins if both set. */
  group: string | null;
  startMs: number | null;
  endMs: number | null;
  q: string;
}

/** A page of history results. `nextBefore` is the cursor for the following
 *  page; it is absent (optional on the wire) when this was the last page. */
export interface HistoryResponse {
  rows: QueryRow[];
  nextBefore?: HistoryCursor;
}

/** The DNSSEC view: the lifetime count of queries in each verdict, and the most
 *  recent validated queries (optionally filtered to a single verdict). */
export interface DnssecResponse {
  secure: number;
  insecure: number;
  bogus: number;
  indeterminate: number;
  recent: QueryRow[];
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
  /** Absent when the group's delta is empty — the server omits an empty delta,
   *  so treat a missing policy as {@link emptyDelta}. */
  policy?: PolicyDelta;
}

/** One switchable block object the policy editor offers: the two built-ins
 *  (`ads`, `malware`) and every block feed. `globalOn` is its state in the
 *  global default, so a scope that neither adds nor disables it inherits this. */
export interface ListObject {
  id: string;
  label: string;
  globalOn: boolean;
}

/** One blockable service from the curated catalogue (a social network, a
 *  streaming service, a messenger …). `id` is the bare machine id used by
 *  `PUT /api/services/{id}` for the global default; `listId` (`svc:<id>`) is the
 *  list-object id to switch on or off per scope through a {@link PolicyDelta}.
 *  `globalOn` is its state in the global default. Matches the Rust `ServiceRow`. */
export interface Service {
  id: string;
  listId: string;
  name: string;
  /** Coarse grouping for the catalogue filter: `social`, `video`, `messaging`,
   *  `gaming` or `music`. */
  category: string;
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
  /** The blockable-services catalogue, so the device and group editors can
   *  render the service switches alongside the list switches. */
  services: Service[];
  activeWindowSecs: number;
}

/** Which scope a schedule applies to. */
export type ScheduleScope = "global" | "group" | "device";

/** The target of a schedule: a scope plus, for a group or device, the id or
 *  client target it points at. The id is absent for the global scope (the server
 *  omits it), so treat it as optional. Matches the Rust `ScheduleTarget`. */
export interface ScheduleTarget {
  scope: ScheduleScope;
  /** Group id or device target. Absent for the global scope. */
  id?: string;
}

/** What a schedule does while its window is open: `pause` blocks everything for
 *  the target (the allow list still wins), `block` layers an extra delta. */
export type ScheduleMode = "pause" | "block";

/** A schedule's effect. `delta` is always present on the wire (empty for a
 *  pause); for a `block` it is the lists/domains to add while the window is open.
 *  Matches the Rust `ScheduleEffect`. */
export interface ScheduleEffect {
  mode: ScheduleMode;
  delta: PolicyDelta;
}

/** A weekday, lowercase, matching the server's JSON. An overnight window lists
 *  the day it *starts* on. */
export type Weekday = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";

/** The seven weekdays in display (Monday-first) order. */
export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** One schedule: a recurring weekly time window that tightens filtering for a
 *  target. `start`/`end` are `"HH:MM"` local times; `end <= start` means the
 *  window crosses midnight. Matches the Rust `Schedule`. */
export interface Schedule {
  id: string;
  name: string;
  enabled: boolean;
  days: Weekday[];
  start: string;
  end: string;
  target: ScheduleTarget;
  effect: ScheduleEffect;
}

/** Everything needed to create a schedule: a {@link Schedule} without its id,
 *  which the server assigns. Matches the Rust `ScheduleDraft`. */
export type ScheduleDraft = Omit<Schedule, "id">;

/** The schedules view: just the stored schedules. The target catalogue (groups
 *  and devices) comes from {@link ClientsView}. */
export interface SchedulesView {
  schedules: Schedule[];
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

/** The outcome of refreshing one feed. `previous` and `durationMs` are extra
 *  diagnostics the server may omit, so both are optional. */
export interface RefreshOutcome {
  id: string;
  count?: number;
  previous?: number;
  durationMs?: number;
  error?: string;
}

export interface RefreshResponse {
  outcomes: RefreshOutcome[];
  lists: ListsView;
}

/** An account role, most to least powerful. Matches the Rust `Role`. */
export type Role = "admin" | "editor" | "viewer";

/** Whether an admin exists yet and who, if anyone, the caller is logged in as.
 *  `role` is the signed-in user's role, absent when not signed in. */
export interface AuthState {
  configured: boolean;
  authenticated: boolean;
  username: string | null;
  role?: Role;
}

/** Where a login stands after a step. `authenticated` is done; `mfa` means a
 *  code is needed; `enroll` means MFA must be set up before access. */
export type LoginStatus = "authenticated" | "mfa" | "enroll";

/** What a login step returns. */
export interface LoginResponse {
  username: string;
  status: LoginStatus;
}

/** The one-time material shown while enrolling in MFA: the base32 secret (for
 *  manual entry), the `otpauth://` URI, a ready-to-render QR as SVG, and the
 *  recovery codes. Shown once; only hashes and the secret are kept on the box. */
export interface MfaSetup {
  secret: string;
  otpauthUri: string;
  qrSvg: string;
  recoveryCodes: string[];
}

/** A freshly minted set of recovery codes, returned when regenerating them. */
export interface RecoveryCodes {
  recoveryCodes: string[];
}

/** The signed-in user's own account summary. `mfaRequired` is true when MFA is
 *  required for them (server-wide or individually), so the UI can tell them it
 *  cannot be turned off. */
export interface AccountInfo {
  username: string;
  role: Role;
  mfaEnabled: boolean;
  mfaRequired: boolean;
}

/** One account in the admin's user list — never any secret. Matches the Rust
 *  `UserInfo`. */
export interface UserInfo {
  username: string;
  role: Role;
  mfaEnabled: boolean;
  mfaRequired: boolean;
}

/** Whether an audited action succeeded or was refused. */
export type AuditOutcome = "success" | "failure";

/** One entry in the security audit log. `target` and `detail` are optional (the
 *  server omits them when absent). Matches the Rust `AuditEntry`. */
export interface AuditEntry {
  time: string;
  actor: string;
  action: string;
  target?: string;
  outcome: AuditOutcome;
  detail?: string;
}

/** A diagnostic-log severity, as the server tags each event. */
export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

/** One captured diagnostic (`tracing`) event from the system log. `fields` is
 *  omitted by the server when the event carried no structured fields, so treat
 *  it as optional. Matches the Rust `LogRecord`. */
export interface SystemLogRecord {
  time: string;
  level: LogLevel;
  target: string;
  message: string;
  fields?: string;
}

/** The listener states reported by `/api/diag` — whether each DNS front end is
 *  currently serving. */
export interface DiagListeners {
  udp: boolean;
  tcp: boolean;
  dot: boolean;
  doh: boolean;
}

/** The last subscribed-feed refresh: when it last ran and the error from that
 *  run, if any. Both are omitted by the server before the first refresh / on
 *  success, so both are optional. */
export interface DiagFeeds {
  lastRefresh?: string;
  lastError?: string;
}

/** The sizes of the loaded block/allow sets, by origin. */
export interface DiagLists {
  ads: number;
  malware: number;
  allow: number;
  custom: number;
}

/** The runtime diagnostics snapshot served by `/api/diag` (admin only): the
 *  running version and uptime, which listeners are up, the last feed refresh,
 *  the response-cache size, the loaded list sizes and the active-client count.
 *  Matches the Rust `DiagResponse` (camelCase on the wire). */
export interface DiagInfo {
  version: string;
  uptimeSecs: number;
  listeners: DiagListeners;
  feeds: DiagFeeds;
  cacheEntries: number;
  lists: DiagLists;
  activeClients: number;
  /** The active log verbosity: `normal`, `debug`, `trace`, or `custom` when the
   *  process was started with an explicit `RUST_LOG`. */
  logLevel: string;
}

/** The verbosity presets the System page offers; `custom` is read-only (it only
 *  appears when the operator set `RUST_LOG`). */
export type LogVerbosity = "normal" | "debug" | "trace";

/** The active log verbosity, as reported by `/api/logs/level`. */
export interface LogLevelInfo {
  level: string;
}

/** Progress the updater sidecar reports during a one-click update. Mirrors the
 *  Rust `UpdateProgress`; every field past `phase` is optional on the wire. */
export interface UpdateProgress {
  /** `pulling`, `recreating`, `done` or `failed`. */
  phase: string;
  message?: string;
  targetVersion?: string;
  updatedAtMs?: number;
}

/** What `/api/update` reports (admin only): the running and latest versions,
 *  whether an upgrade exists, the changelog, whether the updater sidecar is
 *  present (one-click vs. guided fallback), a pre-filled persistent backup path,
 *  and any in-flight progress. Matches the Rust `UpdateView`; optional fields are
 *  omitted when unset, so they are typed optional. */
export interface UpdateInfo {
  currentVersion: string;
  latestVersion?: string;
  updateAvailable: boolean;
  releaseNotes?: string;
  releaseUrl?: string;
  checkedAtMs?: number;
  error?: string;
  updaterPresent: boolean;
  defaultBackupPath: string;
  progress?: UpdateProgress;
}

/** The lighter `/api/update/status` poll: the running version (so the UI sees
 *  when the new container is up) and the sidecar's progress. */
export interface UpdateStatus {
  currentVersion: string;
  progress?: UpdateProgress;
}

/** One meshed peer as the HA status reports it. Optional fields are omitted by
 *  the server until they are known, so they are typed optional. */
export interface HaPeer {
  url: string;
  addedAtMs: number;
  peerNodeId?: string;
  peerVersion?: string;
  lastSyncMs?: number;
  lastError?: string;
}

/** What `/api/ha/status` reports (admin only). Never carries the sync key —
 *  only whether one is set and whether the environment pins it. Matches the Rust
 *  `HaStatus`. */
export interface HaStatus {
  nodeId: string;
  enabled: boolean;
  keySet: boolean;
  keyFromEnv: boolean;
  version: string;
  intervalSecs: number;
  /** This node's own advertised address, once set (prefilled by the UI). */
  selfUrl?: string;
  /** True once enabled with at least one peer, so the view switches to the node list. */
  paired: boolean;
  /** The meshed peers; listed alongside this node as the cluster roster. */
  peers: HaPeer[];
  /** Count of live (non-tombstone) tracked entities — a cheap health signal. */
  tracked: number;
}

/** The peer handshake result returned by a successful connect. */
export interface HaHello {
  nodeId: string;
  version: string;
}

/** Which config wins the first join: `local` (ours) or `peer` (adopt the cluster). */
export type HaPrefer = "local" | "peer";

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
  /** The domains a single client looked up most over the last 24 hours, most
   *  lookups first. A never-seen client yields an empty list. */
  topDomains: (client: string, limit = 10, signal?: AbortSignal) => {
    const params = new URLSearchParams({ client, limit: String(limit) });
    return getJson<DomainCount[]>(`/api/stats/top-domains?${params}`, signal);
  },
  /** The upstream resolvers that answered over the last 24 hours, busiest first,
   *  each with its handling-time figures. Empty on a cache-only instance. */
  resolverStats: (signal?: AbortSignal) =>
    getJson<ResolverStat[]>("/api/stats/resolvers", signal),
  /** The privacy dashboard: each device's DNS lookups attributed to the companies
   *  behind the names, over the window (`day`/`week`/`month`), with network-wide
   *  totals, the companies contacted most, and a per-device breakdown. */
  privacyTrackers: (window: TrackerWindow = "day", signal?: AbortSignal) =>
    getJson<TrackerReport>(`/api/privacy/trackers?window=${window}`, signal),
  /** Block a whole company (every domain the catalogue attributes to it) or a
   *  single domain, straight from the privacy dashboard. Editor-gated and
   *  idempotent; returns how many entries were newly added to the block list. */
  privacyBlock: (target: { org: string } | { domain: string }) =>
    sendJson<{ added: number }>("POST", "/api/privacy/block", target),
  queries: (limit = 50, query = "", signal?: AbortSignal) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (query.trim()) params.set("q", query.trim());
    return getJson<QueryRow[]>(`/api/queries?${params}`, signal);
  },
  /** A filtered, paginated page of the **full durable history** (DuckDB), newest
   *  first — the explorer behind `/protokoll`, able to page back past the live
   *  ring. Pass `before` (a previous page's `nextBefore`) to continue. */
  queriesHistory: (
    filter: HistoryFilter,
    before: HistoryCursor | null = null,
    limit = 100,
    signal?: AbortSignal,
  ) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (before) {
      params.set("beforeMs", String(before.atMs));
      params.set("beforeRow", String(before.row));
    }
    if (filter.sources.length > 0) params.set("source", filter.sources.join(","));
    if (filter.recordType != null) params.set("recordType", String(filter.recordType));
    if (filter.responseCode != null) params.set("responseCode", String(filter.responseCode));
    if (filter.resolver) params.set("resolver", filter.resolver);
    if (filter.device) params.set("device", filter.device);
    if (filter.group) params.set("group", filter.group);
    if (filter.startMs != null) params.set("startMs", String(filter.startMs));
    if (filter.endMs != null) params.set("endMs", String(filter.endMs));
    if (filter.q.trim()) params.set("q", filter.q.trim());
    return getJson<HistoryResponse>(`/api/queries/history?${params}`, signal);
  },
  /** The cluster roster resolved to display labels, so the explorer can show
   *  which server answered a query by its hostname instead of a node-id prefix.
   *  Readable by any signed-in user, like `/api/queries` itself. */
  nodes: (signal?: AbortSignal) => getJson<NodesResponse>("/api/nodes", signal),
  /** The DNSSEC verdict counts and the most recent validated queries. Pass a
   *  `status` to filter the recent list to a single verdict. */
  dnssec: (status?: DnssecStatus, limit = 50, signal?: AbortSignal) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (status) params.set("status", status);
    return getJson<DnssecResponse>(`/api/dnssec?${params}`, signal);
  },
  settings: (signal?: AbortSignal) => getJson<ServerSettings>("/api/settings", signal),
  saveSettings: (settings: ServerSettings, signal?: AbortSignal) =>
    putJson<ServerSettings>("/api/settings", settings, signal),
  /** Send a test notification through the saved webhook so the operator can
   *  confirm the channel works. Resolves on 204; the ApiError detail carries
   *  the delivery error (bad URL, HTTP status from the webhook) on failure.
   *  Save the settings first — the server uses the stored config, not the form. */
  notificationsTest: () => sendJson<void>("POST", "/api/notifications/test"),

  // --- Scheduled reports (admin only) ---
  /** The generated reports, newest window first, as compact index rows. */
  reports: (signal?: AbortSignal) => getJson<ReportListItem[]>("/api/reports", signal),
  /** One full report (headline numbers plus the three top-tables). */
  report: (id: string, signal?: AbortSignal) =>
    getJson<Report>(`/api/reports/${encodeURIComponent(id)}`, signal),
  /** Generate a report now for the last full day or week, without waiting for
   *  the schedule. Returns the stored report. */
  runReport: (period: ReportPeriod) =>
    sendJson<Report>("POST", `/api/reports/run?period=${period}`),
  /** Download a report as a printable HTML document or as CSV of its top-lists.
   *  Returns a Blob the caller saves to a file. */
  downloadReport: async (id: string, format: "html" | "csv", lang: string): Promise<Blob> => {
    const url = `/api/reports/${encodeURIComponent(id)}/download?format=${format}&lang=${lang}`;
    const response = await fetch(url);
    if (!response.ok) {
      throw failed(url, response.status);
    }
    return response.blob();
  },

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
  /** The blockable-services catalogue, each entry marked with its global state. */
  services: (signal?: AbortSignal) => getJson<Service[]>("/api/services", signal),
  /** Switch a service into or out of the global default (Admin only). Returns
   *  the fresh catalogue. */
  setService: (id: string, on: boolean) =>
    sendJson<Service[]>("PUT", `/api/services/${encodeURIComponent(id)}`, { on }),
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
  /** Every schedule, in the order the store keeps them. */
  schedules: (signal?: AbortSignal) => getJson<SchedulesView>("/api/schedules", signal),
  /** Create a schedule from a draft; the server assigns its id. */
  addSchedule: (draft: ScheduleDraft) =>
    sendJson<SchedulesView>("POST", "/api/schedules", draft),
  /** Replace a schedule, keyed by its id. */
  editSchedule: (schedule: Schedule) =>
    sendJson<SchedulesView>("PUT", "/api/schedules", schedule),
  /** Delete a schedule by its id. */
  removeSchedule: (id: string) => sendJson<SchedulesView>("DELETE", "/api/schedules", { id }),
  /** Drop every entry in the DNS response cache. */
  clearCache: () => sendJson<void>("POST", "/api/cache/clear"),
  /** Download the full configuration backup (settings, lists and rewrites) as a
   *  Blob the caller saves to a file. The file is encrypted with `password`, so
   *  it is never readable in the clear; the password is mandatory. */
  downloadBackup: async (password: string): Promise<Blob> => {
    const response = await fetch("/api/backup", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ password }),
    });
    if (!response.ok) {
      throw failed("/api/backup", response.status);
    }
    return response.blob();
  },
  /** Restore an encrypted backup file, replacing the current settings, lists and
   *  rewrites. `file` is the raw file bytes base64-encoded; `password` is the one
   *  it was downloaded with (legacy plaintext backups need no password). The
   *  server refuses a file that is not a JustDNS backup or a wrong password. */
  restoreBackup: (file: string, password?: string) =>
    sendJson<void>("POST", "/api/backup/restore", { file, password }),
  /** The encrypted client-DNS status (DoT/DoH flags, hostname, selected certs). */
  tls: (signal?: AbortSignal) => getJson<TlsStatus>("/api/tls", signal),
  /** Set the DoT/DoH flags, hostname and selected certificate ids. Applied live. */
  saveTls: (settings: TlsSettings) => sendJson<TlsStatus>("PUT", "/api/tls", settings),

  // --- Certificate store (admin only) ---
  /** The stored certificates, each with the facts and which features select it. */
  certs: (signal?: AbortSignal) => getJson<CertListItem[]>("/api/certs", signal),
  /** Mint a named self-signed certificate; a blank hostname falls back to the
   *  encrypted-DNS hostname. Returns the refreshed catalogue. */
  generateCert: (name: string, hostname?: string) =>
    sendJson<CertListItem[]>("POST", "/api/certs", { name, hostname }),
  /** Store a named operator-supplied chain and key (both PEM). Returns the
   *  refreshed catalogue. */
  uploadCert: (name: string, cert: string, key: string) =>
    sendJson<CertListItem[]>("POST", "/api/certs", { name, cert, key }),
  /** Replace a stored certificate in place with an uploaded, currently-valid
   *  chain and key. Refused when the replacement is expired or not yet valid. */
  renewCert: (id: string, cert: string, key: string) =>
    sendJson<CertListItem[]>("POST", `/api/certs/${encodeURIComponent(id)}/renew`, { cert, key }),
  /** Remove a stored certificate. Refused while any feature still selects it. */
  deleteCert: (id: string) =>
    sendJson<CertListItem[]>("DELETE", `/api/certs/${encodeURIComponent(id)}`),
  /** Download a minted certificate's local CA (PEM) as a Blob. 404 for an
   *  uploaded certificate, which has no CA. */
  downloadCertCa: async (id: string): Promise<Blob> => {
    const path = `/api/certs/${encodeURIComponent(id)}/ca`;
    const response = await fetch(path);
    if (!response.ok) {
      throw failed(path, response.status);
    }
    return response.blob();
  },
  authState: (signal?: AbortSignal) => getJson<AuthState>("/api/auth/state", signal),
  setup: (username: string, password: string) =>
    postJson<LoginResponse>("/api/auth/setup", { username, password }),
  login: (username: string, password: string) =>
    postJson<LoginResponse>("/api/auth/login", { username, password }),
  /** Second login step for a user with MFA: a TOTP or recovery code. */
  loginMfa: (code: string) => postJson<LoginResponse>("/api/auth/mfa", { code }),
  /** Begin enrolment during a login that requires MFA but has none set up. */
  enrollBegin: () => postJson<MfaSetup>("/api/auth/enroll/begin", {}),
  /** Finish that enrolment with a code; promotes the session. */
  enrollConfirm: (code: string) => postJson<LoginResponse>("/api/auth/enroll/confirm", { code }),
  logout: () => postJson<void>("/api/auth/logout", {}),

  // --- Own account (any signed-in user) ---
  /** The signed-in user's own account summary. */
  account: (signal?: AbortSignal) => getJson<AccountInfo>("/api/account", signal),
  /** Change one's own password after proving the current one. */
  changeOwnPassword: (currentPassword: string, newPassword: string) =>
    sendJson<void>("PUT", "/api/account/password", { currentPassword, newPassword }),
  /** Begin enrolling oneself in MFA; returns the secret/QR/recovery to show once. */
  accountMfaBegin: () => sendJson<MfaSetup>("POST", "/api/account/mfa/begin"),
  /** Confirm one's own enrolment with a code, turning MFA on. */
  accountMfaConfirm: (code: string) =>
    sendJson<void>("POST", "/api/account/mfa/confirm", { code }),
  /** Turn one's own MFA off (refused when MFA is required for the account). */
  accountMfaDisable: (password: string) =>
    sendJson<void>("POST", "/api/account/mfa/disable", { password }),
  /** Replace one's own recovery codes, after proving the password. */
  accountMfaRegenerateRecovery: (password: string) =>
    sendJson<RecoveryCodes>("POST", "/api/account/mfa/recovery", { password }),

  // --- User management (admin only) ---
  /** Every account, for the admin's user-management view. */
  users: (signal?: AbortSignal) => getJson<UserInfo[]>("/api/users", signal),
  /** Create an account with a role and an initial password. */
  addUser: (username: string, password: string, role: Role, mfaRequired: boolean) =>
    sendJson<void>("POST", "/api/users", { username, password, role, mfaRequired }),
  /** Change a user's role (the last admin cannot be demoted). */
  setUserRole: (username: string, role: Role) =>
    sendJson<void>("PUT", `/api/users/${encodeURIComponent(username)}/role`, { role }),
  /** Reset a user's password (admin action). */
  resetUserPassword: (username: string, newPassword: string) =>
    sendJson<void>("PUT", `/api/users/${encodeURIComponent(username)}/password`, { newPassword }),
  /** Require (or stop requiring) MFA for a single user. */
  setUserMfaRequired: (username: string, required: boolean) =>
    sendJson<void>("PUT", `/api/users/${encodeURIComponent(username)}/mfa-required`, { required }),
  /** Clear a user's MFA after a lost device. */
  resetUserMfa: (username: string) =>
    sendJson<void>("DELETE", `/api/users/${encodeURIComponent(username)}/mfa`),
  /** Delete an account (not your own; the last admin cannot be removed). */
  removeUser: (username: string) =>
    sendJson<void>("DELETE", `/api/users/${encodeURIComponent(username)}`),

  // --- Audit log (admin only) ---
  /** The most recent audit entries, newest first. */
  audit: (limit = 200, signal?: AbortSignal) =>
    getJson<AuditEntry[]>(`/api/audit?limit=${limit}`, signal),

  // --- System / diagnostics (admin only) ---
  /** The most recent diagnostic-log events, newest first. Pass `level` to keep
   *  only that severity and above (error > warn > info > debug > trace). */
  logsSystem: (level?: LogLevel, limit = 200, signal?: AbortSignal) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (level) params.set("level", level);
    return getJson<SystemLogRecord[]>(`/api/logs/system?${params}`, signal);
  },
  /** The runtime diagnostics snapshot: version, uptime, listeners, last feed
   *  refresh, cache and list sizes, active clients. */
  diag: (signal?: AbortSignal) => getJson<DiagInfo>("/api/diag", signal),
  /** The active log verbosity. */
  logLevel: (signal?: AbortSignal) => getJson<LogLevelInfo>("/api/logs/level", signal),
  /** Raise or lower the live log verbosity. Temporary — a restart returns to the
   *  shipping default. */
  setLogLevel: (level: LogVerbosity) =>
    sendJson<LogLevelInfo>("PUT", "/api/logs/level", { level }),

  // --- In-app update (admin only) ---
  /** The cached update status (no network call). */
  getUpdate: (signal?: AbortSignal) => getJson<UpdateInfo>("/api/update", signal),
  /** Force a fresh check against GitHub now. */
  checkUpdate: () => sendJson<UpdateInfo>("POST", "/api/update/check"),
  /** Start a one-click update: write the safety backup to `backupPath` (must be
   *  under the data volume), encrypted with `password`, and ask the sidecar to
   *  pull and recreate. The password keeps the on-disk safety backup unreadable. */
  startUpdate: (backupPath: string, password: string) =>
    sendJson<UpdateInfo>("POST", "/api/update", { backupPath, password }),
  /** The light progress poll, used while an update runs and the UI reconnects. */
  updateStatus: (signal?: AbortSignal) => getJson<UpdateStatus>("/api/update/status", signal),

  // --- High availability (admin only) ---
  /** The current HA status (node id, enablement, pairing, last sync). */
  haStatus: (signal?: AbortSignal) => getJson<HaStatus>("/api/ha/status", signal),
  /** Turn HA on or off. Pairing and ledger are kept across a toggle. */
  haEnable: (enabled: boolean) => sendJson<void>("POST", "/api/ha/enable", { enabled }),
  /** Set (or, with an empty string, clear) the shared sync key. Refused while the
   *  key is pinned by an environment variable. */
  haSetKey: (key: string) => sendJson<void>("PUT", "/api/ha/key", { key }),
  /** Set (or, with an empty string, clear) this node's own advertised address, so
   *  peers can mesh back to it. */
  haSetAddress: (url: string) => sendJson<void>("PUT", "/api/ha/address", { url }),
  /** Seed into the cluster: hand over one member's base URL and which config wins
   *  the first join. The rest of the mesh is discovered automatically. */
  haConnect: (url: string, prefer: HaPrefer) =>
    sendJson<HaHello>("POST", "/api/ha/peers", { url, prefer }),
  /** Forget one peer by node id, or — with no id — leave the cluster entirely. */
  haDisconnect: (nodeId?: string) =>
    sendJson<void>(
      "DELETE",
      nodeId ? `/api/ha/peers?node=${encodeURIComponent(nodeId)}` : "/api/ha/peers",
    ),
  /** Run a mesh sync immediately (the "Sync now" button). */
  haSyncNow: () => sendJson<void>("POST", "/api/ha/sync"),
  /** Reveal the shared sync key so an admin can copy it to a new node. Returns
   *  `null` when no key is set or the key is pinned by the environment. */
  haRevealKey: (signal?: AbortSignal) =>
    getJson<{ key: string | null }>("/api/ha/key/reveal", signal),

  // --- Access requests (admin only) ---
  /** The pending "ask to unblock this page" queue, newest first. */
  accessRequests: (signal?: AbortSignal) =>
    getJson<AccessRequest[]>("/api/access-requests", signal),
  /** Release a requested domain at the chosen scope and drop it from the queue. */
  releaseAccessRequest: (id: string, scope: ReleaseScope) =>
    sendJson<void>("POST", `/api/access-requests/${encodeURIComponent(id)}/release`, { scope }),
  /** Dismiss a request without releasing anything. */
  rejectAccessRequest: (id: string) =>
    sendJson<void>("POST", `/api/access-requests/${encodeURIComponent(id)}/reject`),
};
