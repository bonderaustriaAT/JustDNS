# The query log

*A German version of this guide is in [query-log.de.md](query-log.de.md).*

The **Query log** (the *Protokoll* page) is the detailed record of every DNS lookup:
what was asked, by which device, when, how it was answered, and what it resolved to.
It is how you answer "why is this site not loading?" or "what is this smart TV
talking to?".

## Reading a row

Each entry shows:

- **Time** of the lookup.
- **Device / client** — the device name if you have [named it](devices.md), otherwise
  the IP address.
- **Name** that was looked up.
- **How it was answered** — one of:
  - **Cache** — served from the local cache.
  - **Upstream** — looked up at your resolvers.
  - **Local** — answered by JustDNS itself.
  - **Rewrite** — answered by one of your [rewrites](rewrites.md).
  - **Blocked** — stopped by a filter.
- **Addresses** it resolved to (empty for a block or a non-address record).

## Search

Type in the search box to filter the log. It matches on the **name** and on the
**device** — including the device's friendly name, so searching *"tablet"* finds
everything the device you named "Kids' tablet" asked for.

## Turning it on or off

The log depends on **Query log** being enabled under [Settings](settings.md). There
you also set **how many days** of history to keep; older entries are dropped
automatically.

---

## For the technically curious

Entries are written to a DuckDB store on the `/data` volume. Retention is enforced by
dropping entries older than the configured window. A "blocked" entry records the
category and the specific list or rule that matched (the same precedence the
[policy engine](policies.md) applies: allow first, then malware, your own block list,
group and device ad-hoc blocks, your feeds, then ads — first match wins). Because
blocking and rewriting happen before the cache, those sources are logged distinctly
from cache and upstream answers, so the log reflects exactly which stage produced each
answer. The device shown is resolved from the client's source IP at display time via
your device list, so renaming a device updates past rows too.
