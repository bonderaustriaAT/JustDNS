# syntax=docker/dockerfile:1
# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later

# Build the web UI so its bundle can be embedded into the binary.
FROM node:20-slim AS ui
WORKDIR /ui
# The release version (git tag) is baked into the bundle so the UI can show it.
# Defaults to "dev" for a plain `docker build`; the image workflow passes the tag.
ARG APP_VERSION=dev
ENV APP_VERSION=$APP_VERSION
COPY crates/justdns-web/ui/package.json crates/justdns-web/ui/package-lock.json ./
RUN npm ci
COPY crates/justdns-web/ui/ ./
RUN npm run build

# Build the release binary against the workspace, with the UI bundle in place
# so rust-embed can embed it.
FROM rust:1-slim-bookworm AS build
# The query log is compiled from DuckDB's bundled C++ sources.
RUN apt-get update && apt-get install -y g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
COPY --from=ui /ui/dist crates/justdns-web/ui/dist
# Cache the Cargo registry and the target directory across builds so a source
# change recompiles only the crates that actually changed — the bundled DuckDB
# C++ sources (the slow part) stay compiled. target/ lives in the cache mount,
# so the finished binary is copied out to a real path for the runtime stage.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin justdns \
    && cp target/release/justdns /usr/local/bin/justdns

# Runtime: just the binary. TLS roots are compiled in (webpki-roots), so no
# system certificate store is needed for DoT/DoH upstreams.
FROM debian:bookworm-slim AS runtime
COPY --from=build /usr/local/bin/justdns /usr/local/bin/justdns

ENV JUSTDNS_LISTEN=0.0.0.0:53 \
    JUSTDNS_WEB_LISTEN=0.0.0.0:8080 \
    JUSTDNS_IPV6=false \
    JUSTDNS_DB_PATH=/data/queries.duckdb \
    JUSTDNS_CONFIG_PATH=/data/config.json \
    JUSTDNS_AUTH_PATH=/data/auth.json \
    JUSTDNS_LISTS_PATH=/data/lists.json \
    JUSTDNS_LISTS_CACHE=/data/lists-cache \
    JUSTDNS_REWRITES_PATH=/data/rewrites.json \
    JUSTDNS_DOT_LISTEN=0.0.0.0:853 \
    JUSTDNS_DOH_LISTEN=0.0.0.0:8443 \
    JUSTDNS_TLS_DIR=/data/tls

# The query log persists here; mount a volume to keep it across restarts.
VOLUME /data

EXPOSE 53/udp
EXPOSE 53/tcp
EXPOSE 8080/tcp
# Encrypted client DNS: DNS-over-TLS (853) and DNS-over-HTTPS (8443). Both are
# off until enabled in the dashboard, but the ports are declared so a deploy can
# map them up front.
EXPOSE 853/tcp
EXPOSE 8443/tcp

ENTRYPOINT ["/usr/local/bin/justdns"]
