# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later

# Build the web UI so its bundle can be embedded into the binary.
FROM node:20-slim AS ui
WORKDIR /ui
COPY crates/justdns-web/ui/package.json crates/justdns-web/ui/package-lock.json ./
RUN npm ci
COPY crates/justdns-web/ui/ ./
RUN npm run build

# Build the release binary against the workspace, with the UI bundle in place
# so rust-embed can embed it.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
COPY --from=ui /ui/dist crates/justdns-web/ui/dist
RUN cargo build --release --locked --bin justdns

# Runtime: just the binary. TLS roots are compiled in (webpki-roots), so no
# system certificate store is needed for DoT/DoH upstreams.
FROM debian:bookworm-slim AS runtime
COPY --from=build /src/target/release/justdns /usr/local/bin/justdns

ENV JUSTDNS_LISTEN=0.0.0.0:53 \
    JUSTDNS_WEB_LISTEN=0.0.0.0:8080 \
    JUSTDNS_IPV6=false

EXPOSE 53/udp
EXPOSE 53/tcp
EXPOSE 8080/tcp

ENTRYPOINT ["/usr/local/bin/justdns"]
