<!--
SPDX-FileCopyrightText: 2026 JustDNS contributors
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# CI base image

A prebuilt image that carries the toolchain the CI jobs need, so the Gitea
runners stop reinstalling Rust, g++ and Node on every run. It contains:

- Rust (stable) with `rustfmt` and `clippy`
- `g++` — DuckDB's bundled C++ sources are compiled from source
- Node 20 and npm — for the web UI build
- `git` — for `actions/checkout`

`.gitea/workflows/ci.yml` runs both the `test` and `ui` jobs in this image via
`container:`, so only the project's own crates and npm packages are built per
run (the `actions/cache` steps keep `target/` and the npm store warm).

## Building and publishing

The image is tagged `192.168.20.143:3000/bonderosa/ci-base:latest`. It has to be
available to each runner — either loaded into the runner's local Docker store,
or pulled from the registry.

Build and load onto a runner's local store (no registry login needed, since the
runners pull with `force_pull` off):

```sh
docker build -t 192.168.20.143:3000/bonderosa/ci-base:latest ci-base
```

Or push to the registry once (needs a one-time `docker login` to the registry):

```sh
docker login 192.168.20.143:3000
docker push 192.168.20.143:3000/bonderosa/ci-base:latest
```

Rebuild and reload (or repush) whenever the toolchain in the `Dockerfile`
changes — day to day it stays put and the runners reuse it.
