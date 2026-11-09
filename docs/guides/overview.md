# The overview

*A German version of this guide is in [overview.de.md](overview.de.md).*

The **Overview** is the home screen — a live dashboard of what your DNS is doing
right now and over the last day. You don't configure anything here; it's the pulse of
the system.

## What you see

- **Queries resolved** — the big counter of how many lookups JustDNS has answered.
- **Live rate** — queries per second or per minute, switchable, updating as traffic
  comes in.
- **Queries per hour** — a bar graph of the last 24 hours, so you can see when your
  network is busy.
- **From cache** — the share of answers served instantly from the cache instead of a
  fresh upstream lookup. Higher is faster.
- **Blocked** — how many lookups were filtered.
- **Active clients** — how many devices are querying right now.
- **Last 24 hours** — totals for queries and blocks, plus the **response time**.
- **Top clients** — the busiest devices, by name if you have [named them](devices.md).
- **Resolver response times** — how each upstream resolver you forward to is
  performing (see below).

## Response time

JustDNS shows three numbers so one slow lookup can't mislead you:

- **Average** across every query (cache hits included).
- **Median** — the typical query. On a cache-heavy setup this is near zero, because
  most answers come straight from the cache.
- **95th percentile (p95)** — the slow tail: 95% of queries were at least this fast.

Reading them together tells you both how fast it feels day-to-day (median) and how
bad the occasional slow lookup gets (p95).

## Resolver response times

JustDNS usually forwards to more than one upstream resolver (by default DNS4EU and
Quad9, raced against each other). This section breaks the response time down **per
resolver**, so you can see which one is actually answering and how fast each is:

- Each resolver is listed by name — its encrypted (DoT/DoH) certificate name, such
  as *dns.quad9.net*, or its plain IP address — **busiest first**.
- The headline figure is that resolver's **median** answer time; hover a row to see
  its **95th percentile** too.
- Beneath it is how many answers that resolver gave over the last 24 hours.

Only lookups actually **forwarded** to an upstream count here — answers from the
cache, blocks and local rewrites never reach a resolver, so a cache-heavy or freshly
started instance may show little or nothing. It's a quick way to tell whether one of
your resolvers is consistently slower than the others, or to confirm a resolver you
added by hand is really being used.

---

## For the technically curious

The live rate is derived on the client from successive polls of the stats endpoint,
not stored server-side. The 24-hour figures and the response-time percentiles are
computed over recorded queries in the log store, so they depend on the
[query log](query-log.md) being enabled. The response-time statistics are taken over
**all** queries, cache hits included, which is why a cache-heavy instance reads fast;
median and p95 are used alongside the mean specifically so a single slow upstream
lookup cannot pull the headline number out of proportion.

The per-resolver figures are kept apart: each upstream answer is tagged with the
resolver that gave it as it is recorded, and only queries with source *upstream*
carry one — so the per-resolver percentiles are computed over just the forwarded
lookups, not the cache hits that dominate the headline numbers. They survive a
restart the same way the other figures do, rebuilt from the log on startup.
