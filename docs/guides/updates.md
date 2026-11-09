# Updating JustDNS

*A German version of this guide is in [updates.de.md](updates.de.md).*

JustDNS checks for new releases on its own and, when one exists, shows it on the
**System & diagnostics** page with the changelog and an **Update now** button.
One click takes a backup, pulls the new version and restarts the container — no
SSH, no `docker` commands.

## Seeing that an update is available

Every instance quietly checks GitHub for the latest release tag (no account or
token needed — it only reads public tags). When a newer version exists:

- A **green banner** appears at the top of the dashboard for admins, with a link
  to the update.
- The **System & diagnostics** page shows an **Update** card with the new version,
  what's new (straight from the changelog), and the **Update now** button.

You can force a fresh check any time with **Check again** on that card. Update
checks can be turned off entirely in the settings (for an air-gapped install) —
nothing is ever downloaded or installed without your click.

## Running a one-click update

Press **Update now**. A dialog opens that:

1. **Pre-fills a backup path** on the data volume (e.g.
   `/data/backups/justdns-backup-…json`). It must stay under `/data` so the backup
   survives the container being recreated — that is the whole point.
2. Offers **Download restore file**, so you also keep a copy of your configuration
   on your own computer.

Press **Start update**. JustDNS writes the safety backup, then the updater pulls
the new image and recreates the container. The card shows the progress
(*downloading → recreating → waiting for restart*); the dashboard briefly
reconnects while the new version comes up, then reports success.

### After the update

Nothing to do. The host always keeps exactly two images — the version you just
installed and the one before it — so a manual rollback is always one step away
while older images are cleared automatically. The updater prunes them right after
a successful update; there is no button to press.

## Installs without the updater (manual fallback)

One-click updates need the official install (see below), which bundles a small
**updater** container. On an install that does not have it, the **Update now**
dialog instead shows the exact command to run on your Docker host, and still lets
you download the restore file first. Updating is then one copy-paste.

## The official install (so one-click updates work)

The one-click update is driven by an **updater sidecar**: a tiny container that
has the Docker socket and recreates the JustDNS container for you. The DNS process
itself never gets the socket. Install with the bundled Compose file:

```sh
# from the deploy/ directory of a checkout, or the files copied to /opt/justdns
cp .env.example .env     # adjust JUSTDNS_IMAGE / JUSTDNS_TAG if needed
docker compose up -d
```

That starts two containers: `justdns` (the resolver and dashboard) and
`justdns-updater` (the sidecar, with no ports and no inbound network). From then
on, updates are one click in the dashboard.

The default image comes from the **GitHub Container Registry** (`ghcr.io`), built
by GitHub Actions on each release, so it works anywhere with no login. To self-host
the image, point `JUSTDNS_IMAGE` (and `JUSTDNS_UPDATER_IMAGE`) in `.env` at your own
registry.

---

## For the technically curious

**The process never touches Docker.** Giving the internet-facing DNS/API process
the Docker socket would be a needless risk, and a container cannot recreate itself
or delete its own image anyway. So the web process only ever *reads* GitHub and
*writes* a small request file onto the shared data volume
(`/data/update/request.json`). The isolated updater sidecar watches that file,
runs the pull/recreate and then prunes old images, and writes progress back to
`/data/update/status.json`. A heartbeat file tells the UI the sidecar is alive, so
it can offer one-click vs. the guided command.

**Old images are pruned automatically.** After a successful recreate the sidecar
keeps exactly the new image and its immediate predecessor and removes anything
older, so the host never accumulates stale images yet a one-step rollback is always
available. There is nothing for the admin to decide or click.

**Only a version crosses the boundary.** The request carries just a release
version string (validated as `vX.Y.Z`). The sidecar builds the image name from its
own fixed `JUSTDNS_IMAGE` environment variable, so even a compromised web process
cannot make it pull or delete an arbitrary image. The recreate is a plain
`docker compose up -d justdns` with the new tag pinned in `.env`; the updater keeps
running throughout, which is how it can report that the update finished.

**Source and changelog.** The latest release is the highest `vX.Y.Z` tag from the
GitHub tags API (pre-release and non-version tags are ignored); the changelog shown
is the matching section of `CHANGELOG.md` at that tag. Both are public, so this
works out of the box with no token. Override the repository with
`JUSTDNS_UPDATE_REPO` for a fork.

**All of it is admin-only and audited.** The `/api/update*` endpoints require the
Admin role; starting an update is recorded in the [audit log](audit.md). Pruning
old images is the sidecar's own housekeeping and needs no endpoint.
