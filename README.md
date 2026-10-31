# JustDNS

Filtering DNS forwarder with rewrites. It forwards queries to an upstream
resolver, applies block/allow policies and local rewrites (A, AAAA, CNAME,
wildcards) in a single pipeline, and serves a web UI for configuration and
query inspection.

Status: pre-alpha. Under active development, nothing is stable yet.

## Scope

- Forwarding resolver over plain DNS (UDP/TCP, port 53)
- Blocking engine: hosts, domain lists, AdBlock Plus syntax, wildcards, Pi-hole regex
- Rewrites (A, AAAA, CNAME, wildcards) in the same pipeline as blocks
- Per-client and per-group policies, schedules, service bundles
- Web UI with REST API and a WebSocket live view

Out of scope: DHCP, authoritative zones, reverse proxy / VPN.

## Building from source

Requires a Rust toolchain (edition 2024, rustc >= 1.85) and
[pnpm](https://pnpm.io/) for the web UI.

    cargo build --release

The web UI lives in `frontend/` (added later) and is embedded into the binary
at build time.

## License

[AGPL-3.0-or-later](LICENSE). Contributions are accepted under the same
license; see [CONTRIBUTING.md](CONTRIBUTING.md).
