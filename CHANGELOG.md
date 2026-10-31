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
