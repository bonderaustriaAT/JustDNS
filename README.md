# JustDNS

**Your whole network's DNS — filtered, encrypted and self-hosted.**

*Read this in German: [README.de.md](README.de.md).*

JustDNS is a filtering DNS forwarder you run yourself. Point your router or your
devices at it and every lookup passes through one place: ads, trackers and
malware domains are blocked, your chosen resolvers answer the rest over an
encrypted connection, and a clean web dashboard shows you exactly what is
happening on your network — with no accounts, no cloud, and no data leaving your
box.

> Status: actively developed, pre-1.0. It runs and is useful today; settings and
> details can still change between versions.

## Highlights

- **Blocks ads, trackers and malware** out of the box, using curated
  [Hagezi](https://github.com/hagezi/dns-blocklists) lists. Switch the built-in
  categories on or off, add your own allow/block entries or list subscriptions any
  time, and give any **device or group** its own rules on top.
- **Private by default.** Ships with privacy-friendly EU resolvers (DNS4EU,
  Quad9) over **DNS-over-TLS**, raced for speed. Swap in your own resolvers —
  plaintext, DoT or DoH — or pick a **Privacy / Balanced / Speed** profile.
- **Fast.** A response cache with pre-fetching and serve-stale, plus warm
  encrypted connections, keeps repeat lookups near-instant.
- **Local rewrites.** Answer your own names from fixed addresses (A / AAAA) —
  handy for a NAS, a homelab box or split-horizon setups.
- **Safe Search.** Force Google, YouTube, Bing and DuckDuckGo to their filtered
  endpoints, network-wide.
- **See everything.** A live dashboard and a searchable query log (what was
  asked, by which device, how it was answered, and what it resolved to).
- **Per-device & per-group policies.** Give a single device or a whole group its
  own block lists and allow/block rules. Rules stack — global default → group →
  device — so you only set what differs.
- **Guided setup.** A first-run wizard walks you through the admin account,
  resolvers and log retention. Interface in **German and English**.

## Quick start (Docker)

Build the image and run it:

```bash
docker build -t justdns .
```

```bash
docker run -d --name justdns \
  -p 53:53/udp -p 53:53/tcp \
  -p 8080:8080/tcp \
  -v justdns-data:/data \
  justdns
```

Then open **http://<your-host>:8080** and the setup wizard takes it from there.
Point a device's DNS (or your router's) at the host's IP to start using it.

State — the query log, your configuration, account, lists and caches — lives in
`/data`; the volume keeps it across restarts and updates.

## Configuration

Everything is configurable from the web UI after setup. A few things are set at
startup through environment variables:

| Variable | Default | What it does |
|---|---|---|
| `JUSTDNS_LISTEN` | `0.0.0.0:53` | Address/port the DNS server listens on |
| `JUSTDNS_WEB_LISTEN` | `0.0.0.0:8080` | Address/port for the web UI and API |
| `JUSTDNS_IPV6` | `false` | Answer AAAA (IPv6) queries |
| `JUSTDNS_CACHE_CAPACITY` | `10000` | Max cached responses |
| `JUSTDNS_UPSTREAMS` | *(EU defaults)* | Comma-separated plain resolvers (`ip` or `ip:port`) to use instead of the shipped ones — handy to point at a local resolver |

Persistent state defaults under `/data` in the image (database, `config.json`,
account and lists). The config file is the source of truth once set — the
environment only seeds the first run.

## Building from source

Requires a Rust toolchain (edition 2024, rustc ≥ 1.85) and Node.js for the web
UI. The UI is built first and embedded into the binary:

```bash
npm --prefix crates/justdns-web/ui ci
npm --prefix crates/justdns-web/ui run build
cargo build --release --bin justdns
```

The release binary is self-contained (TLS roots are compiled in). Or just use
the `Dockerfile`, which does both stages for you.

## How it works

Each query runs through a single pipeline: a local **rewrite** wins first, then
the **block** check (a blocked name is refused before any lookup), then the
**cache**, and finally the **upstream** resolvers over your chosen transport.
Every handled query is recorded to a local DuckDB log that feeds the dashboard —
and never leaves the machine.

## Guides

Step-by-step guides for every feature, in plain language (German and English),
live in the **[handbook](docs/README.md)** ([Deutsch](docs/README.de.md)). New to
JustDNS? Start with [first-run setup](docs/guides/setup.md), then
[the overview](docs/guides/overview.md).

## Roadmap & changelog

- Where JustDNS is headed: [ROADMAP.md](ROADMAP.md)
  ([Deutsch](ROADMAP.de.md)).
- What's new in each version: [CHANGELOG.md](CHANGELOG.md)
  ([Deutsch](CHANGELOG.de.md)).

## License

[AGPL-3.0-or-later](LICENSE). Contributions are accepted under the same license;
see [CONTRIBUTING.md](CONTRIBUTING.md).
