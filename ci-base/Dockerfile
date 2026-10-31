# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later

# Prebuilt CI image for the Gitea runners. It bakes in the toolchain the CI
# jobs need — Rust (with rustfmt and clippy), g++ for DuckDB's bundled C++
# sources, Node 20 for the web UI, and git — so the runners no longer download
# and install them on every run. Build it and load it onto each runner (or push
# it to the registry); `.gitea/workflows/ci.yml` runs its jobs in this image.
FROM rust:1-bookworm

ENV DEBIAN_FRONTEND=noninteractive

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl gnupg g++ git; \
    mkdir -p /etc/apt/keyrings; \
    curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
        | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg; \
    echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_20.x nodistro main" \
        > /etc/apt/sources.list.d/nodesource.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends nodejs; \
    rm -rf /var/lib/apt/lists/*; \
    rustup component add rustfmt clippy; \
    node --version; npm --version; cargo --version; rustfmt --version; cargo clippy --version
