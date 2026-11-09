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
# so rust-embed can embed it. cargo-chef splits dependency compilation — the slow
# part, including DuckDB's bundled C++ sources — into its own layer that stays
# cached until Cargo.toml/Cargo.lock change. A source change then recompiles only
# the workspace crates. The CI build persists these layers across runs through the
# GitHub Actions cache (see .github/workflows/image.yml), turning the cold ~20 min
# build into a couple of minutes once the cache is warm.
FROM rust:1-slim-bookworm AS chef
# g++ builds DuckDB's bundled C++ during the dependency cook below; mold is a much
# faster linker, which shortens the final link of the large DuckDB-backed binary —
# the slow tail of every warm build, where the deps are already cached.
RUN apt-get update && apt-get install -y g++ mold && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
# Link with mold instead of GNU ld. Set after cargo-chef is installed (so its own,
# already-built binary is unaffected); the planner and build stages inherit it.
ENV RUSTFLAGS="-Clink-arg=-fuse-ld=mold"
WORKDIR /src

# Distil the workspace down to a dependency recipe (just the manifests).
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# Compile only the dependencies from the recipe. This layer is reused as long as
# the dependency set is unchanged, so it does not rebuild on an ordinary code or
# release-version change.
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
# Now the real sources and the prebuilt UI bundle; from here only our own crates
# recompile. (APP_VERSION is a runtime env set in the runtime stage, not needed
# to compile, so it is deliberately absent here and does not invalidate caches.)
COPY . .
COPY --from=ui /ui/dist crates/justdns-web/ui/dist
RUN cargo build --release --locked --bin justdns \
    && cp target/release/justdns /usr/local/bin/justdns

# Runtime: just the binary. TLS roots are compiled in (webpki-roots), so no
# system certificate store is needed for DoT/DoH upstreams. curl is added only
# so the container HEALTHCHECK below can probe the web endpoint.
FROM debian:13.7-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/justdns /usr/local/bin/justdns

# The release version (git tag) so the backend can report it at /api/diag. The
# build arg is the same one the UI stage bakes into the bundle; re-declaring it
# here hands it to the runtime process too. Defaults to "dev" for a plain build.
ARG APP_VERSION=dev
ENV APP_VERSION=$APP_VERSION

ENV JUSTDNS_LISTEN=0.0.0.0:53 \
    JUSTDNS_WEB_LISTEN=0.0.0.0:8080 \
    JUSTDNS_IPV6=false \
    JUSTDNS_DB_PATH=/data/queries.duckdb \
    JUSTDNS_CONFIG_PATH=/data/config.json \
    JUSTDNS_AUTH_PATH=/data/auth.json \
    JUSTDNS_LISTS_PATH=/data/lists.json \
    JUSTDNS_LISTS_CACHE=/data/lists-cache \
    JUSTDNS_REWRITES_PATH=/data/rewrites.json \
    JUSTDNS_ACCESS_PATH=/data/access-requests.json \
    JUSTDNS_DOT_LISTEN=0.0.0.0:853 \
    JUSTDNS_DOH_LISTEN=0.0.0.0:8443 \
    JUSTDNS_TLS_DIR=/data/tls \
    JUSTDNS_CERTS_DIR=/data/certs \
    JUSTDNS_SYSLOG_DIR=/data/logs

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

# Liveness: the unauthenticated health endpoint reports 200 once the web server
# is serving. The DNS listeners come up alongside it, so a healthy web endpoint
# is a good proxy for "the process is up and answering".
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/justdns"]
