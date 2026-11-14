# Country and network of answers (GeoIP)

*A German version of this guide is in [geoip.de.md](geoip.de.md).*

Every time JustDNS answers a lookup it already knows the addresses the name resolved to.
With a GeoIP database loaded, it also shows **which country and which network (ASN)** each
of those addresses belongs to — so you can see, at a glance, that your smart TV just
resolved a name into a network in another country. It turns the query log from "what was
asked" into "and where that points".

It is **offline only**: the mapping comes from a local database file on disk, never a
lookup to some online service. That is the whole point for a privacy tool — JustDNS reveals
where your devices' traffic is heading without becoming yet another party that watches it.

## What you see

- **In the query log** (the Protokoll page) each row that resolved to a public address
  carries a small badge with the country flag and the network operator (for example
  `🇺🇸 CLOUDFLARENET · AS13335`). Private destinations (your own LAN) carry none. You can
  also narrow the live feed to answers that land in one country or one ASN.
- **On the Privacy dashboard** a new section, **Where your devices phone home**, groups the
  destinations your whole network reached over the chosen window by country and network,
  busiest first.

Nothing about the DNS service itself changes, and the query log is not enlarged — the
country and network are worked out as the dashboard is read, so turning the feature on or
off, or swapping the database, takes effect immediately and even re-labels older log
entries.

## The database

JustDNS ships with the **DB-IP Lite** country and ASN databases (licensed
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) by
[DB-IP](https://db-ip.com)), so the feature works out of the box in the container image with
nothing to set up. The Lite databases are refreshed monthly; a new image picks up the
current month's copy.

### Bring your own database

If you want MaxMind's GeoLite2 data (or any other MaxMind-format `.mmdb`), point JustDNS at
your own files with two environment variables:

```
JUSTDNS_GEOIP_COUNTRY_DB=/path/to/GeoLite2-Country.mmdb
JUSTDNS_GEOIP_ASN_DB=/path/to/GeoLite2-ASN.mmdb
```

Either file may be given on its own — a country database without an ASN database still shows
flags, and the reverse still shows networks. If neither file can be read, the feature simply
stays off: the log carries no badges and the Privacy section hides itself. (GeoLite2 needs a
free MaxMind account and is governed by MaxMind's own licence, which is why it is not
bundled.)

## For the technically curious

- The databases are read with the standard MaxMind `.mmdb` format, so DB-IP Lite and MaxMind
  GeoLite2 are interchangeable.
- Lookups are cached in memory keyed by address; the same busy destinations (big CDNs, your
  resolvers) are mapped once and then come for free.
- Private, loopback, link-local and carrier-grade-NAT addresses are recognised locally and
  never touch the database — they show as local.
- The database paths are node-local (they come from the environment, not the synced
  settings), so pointing one node at your own files never pushes that path to the rest of a
  high-availability cluster.
