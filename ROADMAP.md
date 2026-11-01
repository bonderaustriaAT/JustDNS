# Roadmap

Where JustDNS is headed, in plain language. A German version of this file is kept
alongside it in [ROADMAP.de.md](ROADMAP.de.md).

JustDNS is built to be self-hosted by anyone (AGPL-3.0), so the direction favours
features that are useful and robust for people running it on their own network.
The order below is our current intent, not a promise — things can move as we
learn. Already-shipped features live in the [changelog](CHANGELOG.md); the latest
release is v0.1.17 (setup wizard, protection strength, instance name).

## Phase 1 — Insight & quick wins

- [ ] **Statistics tab.** A page of its own with: a live query rate (per second or
  per minute, switchable); last-24h figures (total queries, blocked by filters,
  average response time); and the top 5 client IPs with their share of queries.
- [ ] **Configurable block response.** Choose what a blocked name returns — today
  it is NXDOMAIN; options to come are `0.0.0.0` / `::` or a small block page.
- [ ] **Backup & restore.** Export the whole configuration — settings, lists,
  subscriptions and rewrites — to a single file and import it back. This also lays
  the groundwork for syncing two servers later (Phase 5).
- [ ] **Reusable address objects (aliases).** Define a named object — say
  `reverse-proxy` — that stands for an IP address, and use it in DNS rewrites
  instead of typing the address each time. Change the object's address once and
  every rewrite that points at it is updated at once, so you never edit the same
  IP in a dozen places.

## Phase 2 — Devices & policies

- [ ] **Recognise & name clients.** Map a client IP to a friendly device name and
  see which devices are active, so logs and stats read in names, not numbers.
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

- [ ] **Installable web app (PWA) first.** A mobile-friendly, installable version
  of the dashboard — add it to your phone's home screen and manage JustDNS on the
  go, before any native app exists.
- [ ] **Native Android & iOS apps.** Dedicated apps on top of that, for a
  first-class mobile experience and tighter operating-system integration.
