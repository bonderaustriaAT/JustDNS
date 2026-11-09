#!/bin/sh
# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The updater sidecar loop. Every few seconds it writes a heartbeat (so the UI
# knows a one-click update is possible) and, if the dashboard has written a
# request, performs it and reports progress — all through small JSON files on the
# shared data volume. Only a version string ever crosses from the app; the image
# repository is fixed in the environment, so the app cannot steer this at an
# arbitrary image.
set -eu

DATA_DIR="${JUSTDNS_DATA_DIR:-/data}"
UPDATE_DIR="$DATA_DIR/update"
IMAGE="${JUSTDNS_IMAGE:?set JUSTDNS_IMAGE}"
SERVICE="${JUSTDNS_SERVICE:-justdns}"
COMPOSE_FILE="${JUSTDNS_COMPOSE_FILE:-/project/docker-compose.yml}"
ENV_FILE="${JUSTDNS_ENV_FILE:-/project/.env}"
INTERVAL="${JUSTDNS_UPDATE_POLL:-10}"
# An override file we may drop next to the compose file to publish the block
# page's web ports (80/443) without the operator editing their own compose.
# See ensure_blockpage_ports.
OVERRIDE_FILE="$(dirname "$COMPOSE_FILE")/docker-compose.override.yml"

mkdir -p "$UPDATE_DIR"

now_ms() { echo "$(( $(date +%s) * 1000 ))"; }

# Write status.json atomically: phase, message, target version.
write_status() {
  tmp="$UPDATE_DIR/status.json.tmp"
  cat > "$tmp" <<EOF
{"phase":"$1","message":"$2","targetVersion":"$3","updatedAtMs":$(now_ms)}
EOF
  mv "$tmp" "$UPDATE_DIR/status.json"
}

# Accept only a plain release version, so nothing odd reaches docker.
valid_version() { echo "$1" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$'; }

# Pin JUSTDNS_TAG in the env file so the recreate uses the new image.
set_tag() {
  touch "$ENV_FILE"
  if grep -q '^JUSTDNS_TAG=' "$ENV_FILE"; then
    sed -i "s|^JUSTDNS_TAG=.*|JUSTDNS_TAG=$1|" "$ENV_FILE"
  else
    echo "JUSTDNS_TAG=$1" >> "$ENV_FILE"
  fi
}

compose() {
  if [ -f "$OVERRIDE_FILE" ]; then
    docker compose -f "$COMPOSE_FILE" -f "$OVERRIDE_FILE" --env-file "$ENV_FILE" "$@"
  else
    docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" "$@"
  fi
}

# Make sure the block page's web ports (80/443) are published, so a blocked
# domain reaches the page after an update with nothing for the operator to do.
# Host port publishing lives in the compose file, not the image, so this is the
# only place it can happen (the sidecar owns the compose directory). We do it the
# least invasive way: unless the base compose already maps these ports — or uses
# host networking, where published ports are invalid — we drop a small, clearly
# marked override next to it. Compose concatenates `ports`, so the service keeps
# all its other ports. Best-effort: any failure is ignored and the recreate runs.
ensure_blockpage_ports() {
  [ -f "$COMPOSE_FILE" ] || return 0
  # Host networking: ports cannot be published; leave the compose untouched.
  if grep -Eq 'network_mode:[[:space:]]*"?host' "$COMPOSE_FILE" 2>/dev/null; then
    return 0
  fi
  # Already published (base file or a hand-written mapping)? Nothing to do. The
  # anchors keep 8080:8080 / 8443:8443 from counting as 80:80 / 443:443.
  if grep -Eq '(^|[^0-9])(80:80|443:443)([^0-9]|$)' "$COMPOSE_FILE" 2>/dev/null; then
    return 0
  fi
  # Our override already in place? Nothing to do.
  if [ -f "$OVERRIDE_FILE" ] && grep -q 'justdns-blockpage-ports' "$OVERRIDE_FILE" 2>/dev/null; then
    return 0
  fi
  cat > "$OVERRIDE_FILE" <<EOF
# justdns-blockpage-ports: added automatically by justdns-updater so the custom
# block page is reachable on ports 80/443 after an update, with nothing for you
# to do. Delete this file and recreate the service if you do not use the block
# page (or if ports 80/443 are needed by something else on this host).
services:
  $SERVICE:
    ports:
      - "80:80/tcp"
      - "443:443/tcp"
EOF
}

# Keep only the current and the immediately-previous image, removing everything
# older, so the host always holds exactly those two (the update is the current,
# the predecessor stays as an instant rollback). Errors (e.g. an in-use image)
# are ignored.
prune_old() {
  current="$1"
  keep="$2"
  docker images "$IMAGE" --format '{{.Tag}}' | sort -u | while read -r tag; do
    [ -n "$tag" ] || continue
    [ "$tag" = "$current" ] && continue
    [ "$tag" = "$keep" ] && continue
    docker image rm "$IMAGE:$tag" >/dev/null 2>&1 || true
  done
}

process_request() {
  req="$UPDATE_DIR/request.json"
  [ -f "$req" ] || return 0

  version=$(jq -r '.version // empty' "$req" 2>/dev/null || true)
  # Consume the request first, so a bad one never loops forever.
  rm -f "$req"

  if ! valid_version "$version"; then
    write_status failed "invalid version" ""
    return 0
  fi

  # The version running right now becomes the predecessor we keep after the
  # update; everything older than it is pruned automatically.
  prev_ref=$(docker inspect --format '{{.Config.Image}}' "$SERVICE" 2>/dev/null || echo "")
  prev_tag=${prev_ref##*:}

  write_status pulling "pulling $IMAGE:$version" "$version"
  if ! docker pull "$IMAGE:$version"; then
    write_status failed "pull failed" "$version"
    return 0
  fi
  set_tag "$version"
  ensure_blockpage_ports || true
  write_status recreating "recreating $SERVICE" "$version"
  if compose up -d "$SERVICE"; then
    # Keep only the new version and its predecessor; drop anything older.
    prune_old "$version" "$prev_tag"
    write_status done "updated to $version" "$version"
  else
    write_status failed "recreate failed" "$version"
  fi
}

echo "justdns-updater watching $UPDATE_DIR (image $IMAGE, service $SERVICE)"
while true; do
  printf '{"ts":%s}\n' "$(now_ms)" > "$UPDATE_DIR/updater.json" 2>/dev/null || true
  process_request || true
  sleep "$INTERVAL"
done
