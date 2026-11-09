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
- **Server** — in a [high-availability](high-availability.md) cluster, a small badge shows
  which node answered the query, by its **hostname** (the host part of the node's
  address) rather than a cryptic id. Your own node's rows are dimmed and marked as this
  node. On a single-node instance there is only ever one server, so the badge is hidden.

## Search

Type in the search box to filter the log. It matches on the **name** and on the
**device** — including the device's friendly name, so searching *"tablet"* finds
everything the device you named "Kids' tablet" asked for.

## Filtering and the full history

To the left of the list is a **filter rail**. Use it to narrow the log by:

- **Source** — one or more of blocked, cache, upstream, local or rewrite (tick
  several to see any of them).
- **Record type** — A, AAAA, HTTPS, and so on.
- **Response code** — NOERROR, NXDOMAIN, SERVFAIL, …
- **Resolver** — which upstream answered (only the ones that have answered recently
  appear in the list).
- **Device / group** — a single [named device](devices.md) or a whole
  [group](policies.md). Only named devices are offered; a group matches every device in
  it. A device defined as an address range (CIDR) matches every observed address inside
  it, following the same "most specific device wins" resolution as the device column.
- **From / To** — a time range.

As soon as you set any filter, the page switches from the live tail to the **full
stored history**: it reads straight from the durable log on disk, so you are no longer
limited to the most recent entries kept in memory. Because this view spans several days,
the **Time** column here also shows the **date**. Live updating pauses while you filter
(there is nothing to tail when you are looking at the past). Use **Load older** at the
bottom to page further back through the history, and **Reset filters** to clear
everything and return to the live view.

The search box works alongside the filters, so you can, for example, show every
**blocked** lookup for *"doubleclick"* over a chosen afternoon.

## Turning it on or off

The log depends on **Query log** being enabled under [Settings](settings.md). It is a
real switch on the recording itself: turn it off and this node stops writing any new
rows, so the live tail, the explorer and the dashboard statistics all stop gaining
entries (what is already stored stays until retention drops it). Turn it back on and
recording resumes on the next query — no restart. There you also set **how many days**
of history to keep; older entries are dropped automatically.

## Live view vs. the whole cluster

The **live** tail shows only the queries **this node** answered — it is a fast,
in-memory stream of this node's own traffic. As soon as you **filter**, the page reads
the full stored history instead, and in a [cluster](high-availability.md) that history
spans **every** node: each node pulls its peers' entries, and a **server badge** marks
which node answered each row. So if a device uses a different node for DNS, you will not
see it in the live tail here — apply a filter (or open the log on that node) and its
queries appear, badged with their node.

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

**Two read paths — why.** The live tail serves from a small in-memory ring of the most
recent events, which is what makes it instant and lets it poll every couple of seconds
without touching the disk. The filtered view instead runs a real query against the
DuckDB store, which holds the whole retained history, not just the ring. Those are two
genuinely different sources, which is why setting a filter turns the live tail off: the
ring cannot answer "every blocked AAAA lookup last Tuesday", and the store is up to a
couple of seconds behind the very latest events (the writer batches rows and flushes on
a short interval), so it is the wrong source for an instant tail. The filtered reads use
a second, read-only database connection cloned off the writer's, so a long scan never
blocks the stream of new events.

**The server badge** resolves the row's origin — the node id the cluster sync tags each
pulled entry with — to a hostname at display time, using the HA roster (node id → each
node's advertised address). The badge shows the host part of that address, so you see
`dns-02` rather than a 16-character id. The roster is read from a small
`/api/nodes` endpoint that any signed-in user may call (it carries only node ids and
hostnames, never the sync key or peer health), so the explorer can label rows for viewers
as well as admins. A node with no advertised address yet simply shows no label.

**Paging.** "Load older" uses *keyset* pagination rather than an offset: each page
remembers the `(time, row)` of its last entry and the next page asks for rows strictly
older than that. This stays correct and fast even over millions of rows and when many
events share the same millisecond, where a plain `OFFSET` would drift or slow down. The
block reason for an entry is still fetched on demand when you expand it, so filtering
adds no schema and no per-row cost.
