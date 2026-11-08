# SPDX-FileCopyrightText: 2026 JustDNS contributors
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The JustDNS updater sidecar: a Docker CLI plus compose, and nothing else. It
# watches a request file on the shared data volume and performs the pull/recreate/
# remove that the dashboard asks for. It has the Docker socket; the DNS process
# never does.
FROM docker:27-cli

# jq to read the request file; the compose plugin ships with docker:cli.
RUN apk add --no-cache jq

COPY entrypoint.sh /usr/local/bin/justdns-updater
RUN chmod +x /usr/local/bin/justdns-updater

ENTRYPOINT ["/usr/local/bin/justdns-updater"]
