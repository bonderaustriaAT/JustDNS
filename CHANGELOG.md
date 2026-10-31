# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial Cargo workspace: `justdns-dns`, `justdns-filter`, `justdns-store`,
  `justdns-web` and the `justdns` binary.
- Response cache (`justdns-dns`): per-entry TTL expiry via `moka`, record TTLs
  counted down on read, and negative-answer caching.
- Upstream configuration (`justdns-dns`): plain/DoT/DoH transports, selection
  modes (racing, fallback, load-balanced, rule-based), suffix-based domain
  routing, and the EU defaults (DNS4EU Protective and Quad9 over DoT).
- Upstream network client (`justdns-dns`): forwards a query to a single
  upstream over plain DNS, DoT or DoH, with UDP-to-TCP retry for plain
  upstreams, returning the raw response for the cache and response path.
- Upstream router (`justdns-dns`): racing, fallback, load-balanced and
  rule-based selection across the upstream pool, with domain rules routed by
  longest matching suffix.
- Query pipeline (`justdns-dns`): turns a request into a response via cache
  lookup, upstream forwarding and cache store, with negative answers cached for
  the SOA minimum TTL (capped) and upstream errors returned as SERVFAIL.
- IPv6 toggle (`justdns-dns`): off by default, answering AAAA queries with an
  empty NODATA response so clients fall back to IPv4; enable it to forward AAAA
  upstream.
- UDP and TCP listeners on port 53 (`justdns-dns`): oversized UDP replies are
  truncated so clients retry over TCP; TCP messages use the two-byte length
  prefix.
- `justdns` binary: reads listen address, IPv6 toggle, cache size and upstreams
  from the environment, then serves. Stops cleanly on SIGTERM or Ctrl-C.
- Container image: multi-stage `Dockerfile`, `.dockerignore` and `docs/docker.md`
  with build, run and `dig` instructions.
- Web UI scaffold (React/Vite/TypeScript): overview with protection status and
  blocked-query activity, and settings with an expert toggle that reveals the
  technical options.
- Settings: a Network category with an IPv6 toggle, off by default, matching the
  DNS core's handling of AAAA queries.
- Embedded web UI (`justdns-web`): the built front-end is embedded with
  `rust-embed` and served over HTTP (SPA fallback to `index.html`), listening on
  `0.0.0.0:8080` by default (`JUSTDNS_WEB_LISTEN`). The binary runs it alongside
  the DNS server, and the Docker build bundles the UI via a Node build stage.
- Query log (`justdns-store`): records each handled query — client, name, type,
  response code, source and timing — into a bounded in-memory view for the
  dashboard and a batched, persistent DuckDB log that survives restarts.
- The resolver now feeds the query log: the pipeline reports every handled query
  through an observer hook, and the binary records it to the store at
  `JUSTDNS_DB_PATH` (default `queries.duckdb`; the container keeps it on a
  `/data` volume).
- JSON API (`justdns-web`): `/api/stats` (totals, sources and active clients),
  `/api/activity` (queries per hour over the last 24 hours) and `/api/queries`
  (the most recent queries) read the query log so the dashboard can show live
  figures. Blocked counts stay zero until filtering exists.
- The overview now shows live data from the API instead of placeholders:
  resolved-query total, cache share, active devices, the 24-hour activity trace
  and the most recent queries, refreshed every few seconds.
- Image publishing: a CI workflow builds the container image on a release tag
  and pushes it to the registry, and a `docker-compose.yml` runs that published
  image — so a deployment pulls a ready-made image instead of compiling the
  bundled DuckDB sources locally.
- Query filter (`justdns-web`): `/api/queries` takes an optional `q` that keeps
  only entries whose queried name or client address contains it (case-
  insensitive), so the UI can search the recent log.
- DNS log page: a `Protokoll` view lists the 100 newest queries — time, client,
  domain, record type, source and timing — with a search box that filters by
  domain or client address and a live toggle that streams new entries in as
  they are handled.

### Fixed

- Negative answers no longer become SERVFAIL (`justdns-dns`): the upstream
  client rebuilds the NXDOMAIN and NODATA responses that hickory reports as a
  `NoRecordsFound` error back into real negative responses, so a non-existent
  name returns NXDOMAIN and a missing record type (for example a browser's
  HTTPS/SVCB probe) returns NOERROR with no answers — not SERVFAIL. The
  authority SOA is preserved so negative caching keeps working. Genuine upstream
  failures (SERVFAIL, REFUSED, transport errors) stay errors.
