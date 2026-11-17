<!--
SPDX-FileCopyrightText: 2026 JustDNS contributors
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Running JustDNS in Docker

A two-stage build produces a small Debian image with just the `justdns`
binary. It listens on UDP and TCP port 53 and forwards to the EU resolvers
over DoT unless told otherwise.

## Build

From the repository root:

```sh
docker build -t justdns:dev .
```

## Run

```sh
docker run --rm -p 53:53/udp -p 53:53/tcp -p 8080:8080 justdns:dev
```

The web UI is then at <http://localhost:8080>.

On a Proxmox LXC the container host often already runs a resolver on port 53
(`systemd-resolved`). Either stop it, or map JustDNS to a spare port for
testing:

```sh
docker run --rm -p 5353:53/udp -p 5353:53/tcp -p 8080:8080 justdns:dev
```

## Configuration

Everything is set through environment variables; the defaults give a working
forwarder.

| Variable                | Default         | Meaning                                                            |
| ----------------------- | --------------- | ------------------------------------------------------------------ |
| `JUSTDNS_LISTEN`        | `0.0.0.0:53`    | DNS address and port to listen on.                                 |
| `JUSTDNS_WEB_LISTEN`    | `0.0.0.0:8080`  | Address and port for the web UI.                                   |
| `JUSTDNS_IPV6`          | `false`         | Answer AAAA queries. Off by default, so clients use IPv4.          |
| `JUSTDNS_CACHE_CAPACITY`| `10000`         | Maximum cached entries.                                            |
| `JUSTDNS_UPSTREAMS`     | *(EU defaults)* | Comma-separated plain resolvers (`ip` or `ip:port`) to use instead of the DoT defaults. |
| `RUST_LOG`              | `info`          | Log filter (e.g. `justdns=debug`).                                 |

### Listening interfaces (IPv4)

`JUSTDNS_LISTEN` takes one address and port. The default `0.0.0.0:53` is the
IPv4 wildcard, so the server already accepts queries from every IPv4 interface
at once; there is no separate per-interface setting to turn on. To restrict it
to a single interface, give that interface's IPv4 address, for example
`JUSTDNS_LISTEN=192.168.1.5:53`. Only one bind address is supported, so it is
either the wildcard (all interfaces) or one specific address, not a hand-picked
list of several.

In Docker the address is seen from inside the container, so `0.0.0.0` is
normally what you want: which host interfaces are actually reachable is decided
by the port mapping (`-p`) or the container's network, not by this variable.

Point it at a plain upstream instead of the encrypted defaults:

```sh
docker run --rm -p 5353:53/udp -p 5353:53/tcp \
  -e JUSTDNS_UPSTREAMS=9.9.9.9,1.1.1.1 \
  justdns:dev
```

## Test with dig

Against the mapped port:

```sh
dig @127.0.0.1 -p 5353 example.com A
```

IPv6 is disabled by default, so an AAAA query returns NOERROR with no answer
section:

```sh
dig @127.0.0.1 -p 5353 example.com AAAA
```

Set `-e JUSTDNS_IPV6=true` to have AAAA queries forwarded upstream.

Force TCP to check the length-prefixed path:

```sh
dig +tcp @127.0.0.1 -p 5353 example.com A
```
