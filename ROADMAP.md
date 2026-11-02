# Roadmap

Where JustDNS is headed, in plain language. A German version of this file is kept
alongside it in [ROADMAP.de.md](ROADMAP.de.md).

JustDNS is built to be self-hosted by anyone (AGPL-3.0), so the direction favours
features that are useful and robust for people running it on their own network.
The order below is our current intent, not a promise — things can move as we
learn. Already-shipped features live in the [changelog](CHANGELOG.md); the latest
release is v0.1.28 (recognise and name your devices).

## Phase 1 — Insight & quick wins

- [x] **Live statistics.** A live query rate (per second or per minute,
  switchable), last-24h figures (total queries, blocked by filters, average
  response time) and the top 5 client IPs with their share of queries — shown on
  the overview rather than a separate page.
- [x] **Configurable block response.** Choose what a blocked name returns — a null
  IP (`0.0.0.0` / `::`, now the default) or NXDOMAIN. A small block page may still
  come later.
- [x] **Backup & restore.** Export the whole configuration — settings, lists,
  subscriptions and rewrites — to a single file and import it back. This also lays
  the groundwork for syncing two servers later (Phase 5).
- [x] **Reusable address objects (aliases).** Define a named object — say
  `reverse-proxy` — that stands for one or more IP addresses, and point DNS
  rewrites at it instead of typing the address each time. Change the object's
  address once and every rewrite that points at it is updated at once, so you
  never edit the same IP in a dozen places. Managed under "Address objects" on
  the Rewrites page.
- [x] **Installable web app (PWA).** A mobile-friendly, installable version of
  the dashboard — add it to your phone's home screen and manage JustDNS on the
  go. Pulled forward as an early win; it wraps the dashboard you already have, so
  full native apps can stay later (Phase 6).

## Phase 2 — Devices & policies

- [x] **Recognise & name clients.** Map a client IP (or a CIDR range) to a
  friendly device name and see which devices are active, so logs and stats read in
  names, not numbers. Managed on the "Devices" page.
- [ ] **Per-client / per-group policies.** Give a device or a group its own
  protection level and lists, the way AdGuard Home does — the biggest single step
  toward real household control.
- [ ] **Deeper statistics.** Average response time per resolver (which upstream
  answered and how fast), and the top domains per client.

## Phase 3 — Time & access

- [ ] **Schedules.** Block chosen services or lists during time windows, per
  device or group — kids' hours, focus time, bedtime.
- [ ] **Users & roles.** More than one administrator, read-only accounts, and a
  basic record of who changed what.

## Phase 4 — Encryption & reach

- [ ] **Encrypted client DNS.** Run JustDNS itself as a DNS-over-TLS and
  DNS-over-HTTPS endpoint, so your own devices connect to it encrypted — not only
  its upstream resolvers.

## Phase 5 — High availability

- [ ] **Two-server operation.** Run a second JustDNS that stays in sync — lists,
  configuration, rewrites and (optionally) statistics — with automatic failover,
  so DNS keeps working while one node is down or being updated.

## Phase 6 — Apps

- [ ] **Native Android & iOS apps.** Dedicated apps on top of the web app, for a
  first-class mobile experience and tighter operating-system integration. The
  installable web app (Phase 1) comes first and covers mobile use until then.
