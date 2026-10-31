# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later

# Build the release binary against the workspace.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --bin justdns

# Runtime: just the binary. TLS roots are compiled in (webpki-roots), so no
# system certificate store is needed for DoT/DoH upstreams.
FROM debian:bookworm-slim AS runtime
COPY --from=build /src/target/release/justdns /usr/local/bin/justdns

ENV JUSTDNS_LISTEN=0.0.0.0:53 \
    JUSTDNS_IPV6=false

EXPOSE 53/udp
EXPOSE 53/tcp

ENTRYPOINT ["/usr/local/bin/justdns"]
