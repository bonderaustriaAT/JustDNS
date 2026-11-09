# Roadmap

Where JustDNS is headed, in plain language. A German version of this file is kept
alongside it in [ROADMAP.de.md](ROADMAP.de.md).

JustDNS is built to be self-hosted by anyone (AGPL-3.0), so the direction favours
features that are useful and robust for people running it on their own network.
The order below is our current intent, not a promise — things can move as we
learn. Already-shipped features live in the [changelog](CHANGELOG.md); the latest
release is v0.11.0, which turns the query log into a full **explorer** — filter the
whole stored history by source, type, response code, resolver and time range, and page
back past the live tail.

**How to read this:** a ticked box (`[x]`) means the feature has shipped — the
[changelog](CHANGELOG.md) has the details. An open box (`[ ]`) is still planned.
The phases run roughly from near-term to long-term.

---

## Phase 1 — Insight & quick wins _(shipped)_

- [x] **Live statistics.** A live query rate (per second or per minute,
  switchable), last-24h figures (total queries, blocked by filters, average
  response time) and the top 5 client IPs with their share of queries — shown on
  the overview rather than a separate page.
- [x] **Configurable block response.** Choose what a blocked name returns — a null
  IP (`0.0.0.0` / `::`, now the default) or NXDOMAIN. The optional block page
  shipped later, in Phase 4.
- [x] **Backup & restore.** Export the whole configuration — settings, lists,
  subscriptions and rewrites — to a single file and import it back. This also lays
  the groundwork for syncing two servers later (Phase 6).
- [x] **Reusable address objects (aliases).** Define a named object — say
  `reverse-proxy` — that stands for one or more IP addresses, and point DNS
  rewrites at it instead of typing the address each time. Change the object's
  address once and every rewrite that points at it is updated at once, so you
  never edit the same IP in a dozen places. Managed under "Address objects" on
  the Rewrites page.
- [x] **Installable web app (PWA).** A mobile-friendly, installable version of
  the dashboard — add it to your phone's home screen and manage JustDNS on the
  go. Pulled forward as an early win; it wraps the dashboard you already have, so
  full native apps can stay later (Phase 7).

## Phase 2 — Devices & policies _(in progress)_

- [x] **Recognise & name clients.** Map a client IP (or a CIDR range) to a
  friendly device name and see which devices are active, so logs and stats read in
  names, not numbers. Managed on the "Devices" page.
- [x] **Per-client / per-group policies.** Give a device or a group its own
  block lists and allow/block rules on top of the global default, the way AdGuard
  Home does — the biggest single step toward real household control. Rules stack
  (global → group → device); managed on the "Devices" page.
- [x] **Deeper statistics.** Average response time per resolver (which upstream
  answered and how fast), and the top domains per client.
- [x] **Blocked services.** A curated catalogue that blocks a whole product
  (TikTok, YouTube, a game store, …) with one switch by bundling its domains — the
  way AdGuard Home does — settable as the global default and, per device, group or
  schedule, with the same inherit/on/off control as a block list.
- [x] **In-app updates.** Every instance checks GitHub for the latest release and
  shows it in the dashboard; one click takes a backup, pulls the new version and
  restarts, then offers to remove the old image. Driven by an isolated updater
  sidecar in the official Compose file, with a guided command as a fallback.

## Phase 3 — Time & access _(done)_

- [x] **Schedules.** Recurring weekly windows that, for everyone, a group or a
  single device, either pause filtering entirely or switch on extra lists and
  domains — kids' hours, focus time, bedtime. Windows may cross midnight and are
  evaluated in a configurable IANA time zone (DST-correct).
- [x] **Users & roles.** More than one administrator with three roles (admin,
  editor, viewer), two-factor sign-in (TOTP + recovery codes) that can be required
  per user or for everyone, and an audit log of who changed what.

## Phase 4 — Encryption & reach _(done)_

- [x] **Encrypted client DNS.** Run JustDNS itself as a DNS-over-TLS and
  DNS-over-HTTPS endpoint, so your own devices connect to it encrypted — not only
  its upstream resolvers. Each endpoint picks a certificate from the store (below);
  per-device and per-group rules apply to encrypted clients unchanged.
- [x] **Named certificate store.** A central catalogue of TLS certificates under
  **System → Certificates**: generate self-signed ones or upload your own, name
  them, renew them in place after a validity check, and select them for DoT, DoH
  and the block page from a dropdown — no repeat upload.
- [x] **Custom block page.** Serve an explanatory page for a blocked name over HTTP
  and HTTPS (with a stored certificate), instead of the silent null-IP answer. The
  long-promised block page from Phase 1.
- [x] **DNSSEC validation.** Validate the signatures on upstream answers against the
  root trust anchors and refuse a tampered or badly signed reply (SERVFAIL) instead
  of returning forged data. A DNSSEC page shows the per-lookup verdict (secure,
  insecure, bogus, indeterminate). Off by default, switched on in the server
  configuration.

## Phase 5 — Operations & observability _(in progress)_

- [x] **System & diagnostics.** A **System** page (admin only) with the server's
  runtime status — version and uptime, which listeners are up, the last list
  refresh, cache and list sizes, active devices — above a live **diagnostic log**
  (the server's own operational events) you can filter, search and download. The
  log is also written to a rotating file on disk so it survives a restart, and an
  unauthenticated **health endpoint** lets a container or uptime monitor check
  that JustDNS is serving.
- [x] **Prometheus metrics.** A `/metrics` endpoint exposing query, cache and
  listener figures in Prometheus format, so JustDNS plugs into an existing
  monitoring and alerting stack. Token-gated and off until a token is set.
- [x] **Query-log explorer.** The Protokoll page can filter the log by source,
  record type, response code, resolver and a time range, and page back through the
  **whole stored history** (not just the in-memory tail) with "Load older". The live
  tail is unchanged until a filter is set.
- [x] **Scheduled reports.** A **Reports** page (admin only) turns the query log into
  daily or weekly summaries — volume, block rate, average response time, the split by
  source, DNSSEC verdicts and the top domains, blocked domains and devices — generated on
  a schedule (both cadences off by default) or on demand, viewed in-app and downloaded as
  printable HTML or CSV. Numbers are cluster-wide; nothing is sent externally. See
  [the guide](docs/guides/reports.md).
- [x] **Deeper engine logging.** The DNS engine reports its background work to the
  diagnostic log — upstream health transitions, per-feed refresh detail with a
  shrink warning, DNSSEC failures, settings and schedule changes — with a runtime
  **verbosity switch** (Normal/Debug/Trace, temporary) and a **category filter** on
  the System page. See [the guide](docs/guides/system-logs.md).
- [x] **Notifications & alerts.** Active alerts over a **webhook** (generic/ntfy, Slack,
  Discord or Telegram) for four events — a node going down or coming back, a new version
  available, a certificate nearing expiry, and a device blocked an unusual amount (a
  possible malware beacon). Each node sends its own, prefixed with its instance name;
  the cluster-identical "update" alert is sent by just one node. Configured as a section
  in Settings, with a "Send test" button. See [the guide](docs/guides/notifications.md).

## Phase 6 — High availability _(done)_

- [x] **Two-server sync — configuration (Stage A).** Run a second JustDNS that stays
  in sync: settings, lists, rewrites, devices, groups and schedules flow both ways
  between two equal nodes (active/active, last-writer-wins). Off by default; you
  enable it and pair the nodes with a shared key through a guided wizard, and the
  page shows the partner's health. See [the guide](docs/guides/high-availability.md).
- [x] **Users & logins (Stage B).** Accounts, roles and second factors (TOTP and
  recovery codes) sync, so a login — enforced MFA included — works on either node.
  Sessions stay node-local. See [the guide](docs/guides/high-availability.md).
- [x] **Up to four nodes — self-assembling mesh.** The two-node pairing became a full
  mesh of up to four equal nodes: set each node's own address and join a new node to any
  one existing node; it discovers the rest from a gossiped roster and meshes with every
  node automatically. One node down never breaks the sync among the rest. Confirmed MFA
  is reconciled so a sync can never silently switch two-factor off.
- [x] **Statistics & query-log history (Stage C).** The append-only query log is merged
  across the cluster: each node pulls the others' events (incrementally, by a per-peer
  watermark, bounded to the retention window), so the explorer shows the whole cluster's
  history and lifetime figures survive a switch between nodes. Each row is tagged with
  its origin node; the live dashboard stays node-local. The retention setting is now
  enforced and prunes the log on every node.

JustDNS deliberately does **not** automate failover (no node seizes the other's
address). Clients use both resolver IPs, or an external virtual IP, and the
operating-system resolver fallback is the failover.

## Phase 7 — Apps _(planned)_

- [ ] **Native Android & iOS apps.** Dedicated apps on top of the web app, for a
  first-class mobile experience and tighter operating-system integration. The
  installable web app (Phase 1) comes first and covers mobile use until then.
