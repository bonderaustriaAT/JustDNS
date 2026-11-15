# syntax=docker/dockerfile:1
# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later

# A small, self-contained sidecar image: it only talks to the JustDNS HTTP API
# and a public resolver, so it needs none of the main image's UI, GeoIP or
# DuckDB machinery. Build just this crate against the workspace — only its lean
# dependency set (ureq/rustls/serde) compiles, not the DuckDB-backed rest.

FROM rust:1-slim-bookworm AS build
# A C toolchain for rustls' `ring` (C/asm crypto); nothing else is needed.
RUN apt-get update && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p justdns-harvester \
    && cp target/release/justdns-harvester /usr/local/bin/justdns-harvester

# Runtime: just the binary. rustls ships its roots (webpki-roots) compiled in, so
# no system trust store is required; ca-certificates is added only as a courtesy
# for operators who point the probes at a native-TLS endpoint.
FROM debian:13.7-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/justdns-harvester /usr/local/bin/justdns-harvester

# Sensible container defaults; everything else is required at runtime (URL, user,
# password) or optional (see README).
ENV HARVEST_STATE=/data/harvester/state.json \
    HARVEST_OUT=/data/harvester/reports \
    HARVEST_INTERVAL_SECS=3600

# The rolling state file and the dated reports live here; mount a volume to keep
# them across restarts.
VOLUME /data

ENTRYPOINT ["/usr/local/bin/justdns-harvester"]
