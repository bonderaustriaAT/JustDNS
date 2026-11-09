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

Or publish it to the registry. The easiest way is the **`CI base image`
workflow** (`.gitea/workflows/ci-base.yml`): run it from the Actions tab
(`workflow_dispatch`), and it also runs automatically when this `Dockerfile`
changes on `main`. It logs in with the same `REGISTRY_TOKEN` secret as the
release-image workflow and pushes `ci-base:latest`. To push by hand instead
(needs a one-time `docker login` to the registry):

```sh
docker login 192.168.20.143:3000
docker push 192.168.20.143:3000/bonderosa/ci-base:latest
```

Rebuild and reload (or repush) whenever the toolchain in the `Dockerfile`
changes — day to day it stays put and the runners reuse it.

A registry copy is a safety net, not a replacement for the local image: the
runners pull with `force_pull` off, so they keep using a local `ci-base:latest`
when it exists and only re-pull from the registry when it is missing. For that
re-pull to work the runner must be able to read the image — either the
`bonderosa/ci-base` package is public (anonymous pull), or the runner has a
one-time `docker login` to `192.168.20.143:3000`.
