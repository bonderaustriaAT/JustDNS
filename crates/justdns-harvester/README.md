<!--
SPDX-FileCopyrightText: 2026 JustDNS contributors
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# justdns-harvester

A **read-only** companion container that mines a running JustDNS instance's query
log for DACH advertising and reach-measurement domains the existing lists miss,
and writes a **ranked candidate report** for a human to review. It never blocks
anything and never opens a pull request — the report is its only output.

It exists because the built-in tracker catalogue is deliberately thin on
German-language ad/measurement companies, so the domains worth adding to the
[JustDNS DACH blocklist](https://github.com/bonderaustriaAT/justdns-blocklist)
are exactly the ones slipping through *resolved, never blocked* in the log.

## Why a sidecar, and why the API

- The query log lives in a **DuckDB file that is single-writer locked** by the
  JustDNS process. A second reader cannot open it, so the harvester does **not**
  touch the file.
- JustDNS's HTTP API is guarded by a **session cookie** (there is no API key for
  the history endpoint). The harvester logs in as a dedicated **Viewer service
  account** — reads need only Viewer, and the account can do nothing else — and
  pages `GET /api/queries/history` incrementally.
- The account must have **MFA disabled** (this flow has no second step). Give it
  the Viewer role and nothing more.

## The selection test — what goes on the list and what doesn't

Three stages, in order. **Hard gates decide in/out; the score only ranks; a human
makes the final call.** This keeps the list's "each domain individually verified"
ethos while automating the drudgery.

1. **Stage 0 — hard exclusions.** Dropped outright when: already blocked on the
   instance (incl. your own manual block list, learned from the `blocked` feed),
   already covered by the DACH list or an active Hagezi feed (downloaded and
   suffix-matched), on the first-party/shared-infra safelist (CDNs, fonts,
   payment, clouds), too little signal (hits/devices under threshold), or — after
   probing — not resolving live (parked/nulled/NXDOMAIN).
2. **Stage 1 — relevance signals.** Must look like DACH measurement/ads: a
   `.de`/`.at`/`.ch` root, a tracking-ish name
   (`track|metri|stat|count|pixel|ivw|analyt|adserv|werbe|reichweite|…`), or a
   high **device fan-out** — a third-party beacon is reached by many devices, a
   first-party API by one. This fan-out signal comes straight from *your* log and
   is the strongest discriminator.
3. **Stage 2 — function test.** An HTTPS fetch of the root, classified: a **human
   website** (HTML with navigation/structure) ⇒ never the apex, only the pure
   measurement **sub-hosts** go on the list (e.g. `code.etracker.com`, not
   `etracker.de`); a **bare beacon** (pixel / 204 / script / tiny body) ⇒ the
   root itself is safe to block.

The **score** (`w·log(devices) + w·log(hits) + w·days + name + geo − human-apex
penalty`) only orders the shortlist. After reviewing the report, run the final
four-point check (DACH-specific? pure beacon without site function? live? not a
company site?) and add the approved domains to the shared list by hand.

## Configuration (environment)

| Variable | Required | Default | Meaning |
|---|---|---|---|
| `JUSTDNS_URL` | yes | — | Base URL of the instance, e.g. `https://dns.example` |
| `JUSTDNS_USER` | yes | — | Viewer service account username |
| `JUSTDNS_PASSWORD` | yes | — | Its password (read from env, never logged) |
| `HARVEST_STATE` | no | `/data/harvester/state.json` | Rolling state file |
| `HARVEST_OUT` | no | `/data/harvester/reports` | Report output directory |
| `HARVEST_INTERVAL_SECS` | no | `3600` | Poll interval; `0` = run once and exit |
| `HARVEST_ONCE` | no | `0` | `1` = run once and exit (for cron) |
| `HARVEST_WINDOW_DAYS` | no | `7` | First-run backfill window |
| `HARVEST_PAGE_LIMIT` | no | `1000` | Rows per history page |
| `HARVEST_MAX_PAGES` | no | `50` | Page cap per feed per run |
| `HARVEST_MAX_PROBES` | no | `60` | Max roots network-probed per run (rest wait) |
| `HARVEST_PROBE_TIMEOUT_SECS` | no | `6` | DNS/HTTPS probe timeout |
| `HARVEST_DOH_ENDPOINT` | no | `https://cloudflare-dns.com/dns-query` | Public resolver for the liveness probe (deliberately not the local instance) |
| `HARVEST_COVERAGE_URLS` | no | DACH list + Hagezi multi/pro/tif | Comma-separated coverage feeds |

Each run writes `candidates-<unix>.{md,json}` plus stable `latest.{md,json}` into
the output directory.

## Run it

```sh
docker build -f crates/justdns-harvester/Dockerfile -t justdns-harvester .
docker run --rm \
  -e JUSTDNS_URL=https://dns.example \
  -e JUSTDNS_USER=harvester \
  -e JUSTDNS_PASSWORD=… \
  -e HARVEST_ONCE=1 \
  -v harvester-data:/data \
  justdns-harvester
```

As a long-running sidecar, drop `HARVEST_ONCE` and set `HARVEST_INTERVAL_SECS`.

## What it contacts

Only: the JustDNS instance (API, read-only), the configured coverage feeds
(GitHub raw), the DoH endpoint (liveness of candidate domains), and an HTTPS
`GET /` of each candidate domain (the function test). Your log data never leaves
the sidecar — only aggregates are written to the local report.
