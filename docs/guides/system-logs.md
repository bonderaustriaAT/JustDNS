# System & diagnostics

*A German version of this guide is in [system-logs.de.md](system-logs.de.md).*

The **System** page (under *System*, admin only) answers one question at a glance:
**is JustDNS healthy, and if something is wrong, where?** It pairs a snapshot of the
server's **runtime status** with a live feed of its **diagnostic log** — the
server's own operational events — so you rarely need to drop to `docker logs`.

This is the **system** log (how the server itself is doing). It is a different thing
from the **query log** (what each device looked up), which lives under *Monitoring*
and is covered in the [query-log guide](query-log.md).

## Runtime status

The tiles across the top are a point-in-time snapshot, refreshed every couple of
seconds:

- **Version** — the running build, with the process **uptime** beneath it.
- **Services** — the four DNS front ends (**UDP**, **TCP**, **DoT**, **DoH**), each
  with a dot: green when it is serving, grey when it is off. DoT and DoH are off
  until you enable them under [Encrypted client DNS](encrypted-dns.md), so grey
  there is normal.
- **Last list refresh** — when the block/allow subscriptions were last fetched, and
  whether that run succeeded. The tile turns red if the last refresh failed.
- **Cache entries** — how many answers are currently in the response cache.
- **Active devices** — how many clients have queried recently (within the active
  window set under [Settings](settings.md)).
- **Loaded list entries** — the total number of rules loaded, broken down into
  ads/trackers, malware, your own blocks and allow entries.

## The diagnostic log

Below the status is the live log. Each line is one event the server emitted, newest
first, with its **time**, a coloured **severity badge**, the **source** (which part
of JustDNS logged it) and the **message**.

- **Filter by severity** with the buttons: *All*, *Warnings+* (warnings and errors)
  or *Errors only*.
- **Search** narrows the list to lines whose message, source or fields contain your
  text — applied instantly as you type.
- **Filter by category** with the dropdown (shown once more than one subsystem has
  logged): keep only *DNS & upstreams*, *Lists*, *Web & HA*, and so on, so you can
  watch one part of the server without the rest scrolling past.
- **Search** narrows the list to lines whose message, source or fields contain your
  text — applied instantly as you type.
- **Download .txt** saves exactly what you are looking at (filter and search
  applied) as a plain-text file, handy for pasting into a bug report.
- If any **error** is in view, a **red banner** appears above the log and the error
  rows are tinted red, so a problem is hard to miss.

### Verbosity (temporary)

Above the log is a **Verbosity** switch — *Normal*, *Debug* or *Trace*. It changes
how much JustDNS writes, not just what the stream shows: *Debug* and *Trace* turn on
the extra detail the DNS engine normally keeps quiet (per-query upstream retries,
matcher recompiles, and more). It takes effect immediately, with no restart.

The change is **deliberately temporary**: a restart returns to *Normal*, so a debug
session you forget about cannot quietly keep filling the disk. Use it to watch a
problem happen, then drop back to *Normal*. (If the server was started with an
explicit `RUST_LOG`, the switch shows *Custom* and a choice here overrides it until
the next restart.)

### What the engine logs

Beyond startup and errors, the DNS engine reports the operational events that used
to be invisible — so a problem shows up here rather than only in the symptoms:

- **Upstream health** — when a resolver stops answering it is marked *unhealthy*
  (one warning, not one per query); when it answers again, one *recovered* line.
- **Feed refreshes** — each subscription logs its new entry count and how long the
  download took, and a **warning** if a feed suddenly lost most of its entries (a
  likely-broken source, which would otherwise silently weaken blocking).
- **DNSSEC** — a validation failure (*Bogus*, the forged answer is refused) is logged
  as a warning with the name.
- **Settings & schedules** — applying new settings (which rebuilds the upstream pool,
  cache tuning and policies) and a schedule window opening or closing each log one
  line, so a live change is accounted for.

Most of these are *info* or *warning* and are always on. The per-query detail (retry
over TCP, the DNSSEC checking-disabled probe) is *debug*, so it appears only while
the verbosity switch is on *Debug* or *Trace*.

## Where the log is kept

The diagnostic log is written to three places at once:

- **Standard output** — as before, for `docker logs` and your container platform.
- **In memory** — a rolling buffer of the most recent events, which is what the
  System page shows. It is cleared on restart.
- **A file on disk** — a rotating daily file, so the log **survives a restart**. In
  the container it lives under `/data/logs` (on the mounted volume), named
  `justdns.<date>.log`.

By default the on-disk log keeps the **last 7 days**. See below to change or disable
it.

---

## For the technically curious

### Configuration

The file sink is controlled by three environment variables, read at startup:

| Variable | Default | Effect |
| --- | --- | --- |
| `JUSTDNS_SYSLOG_DIR` | `logs` (the image sets `/data/logs`) | Directory for the rotating log files. |
| `JUSTDNS_SYSLOG_RETENTION` | `7` | How many daily files to keep; older ones are deleted. |
| `JUSTDNS_SYSLOG` | *(on)* | Set to `off`, `0`, `false` or `no` to switch the **file** sink off. |

Turning the file sink off still leaves the in-memory buffer (and so the System page)
and standard output working — only the on-disk copy stops. The in-memory buffer
holds the most recent 5000 events regardless.

The overall log level follows the usual `RUST_LOG` / `tracing` `EnvFilter`
convention (default `info`); raising it to `debug` feeds the extra detail to all
three sinks at once. The **Verbosity** switch on the System page does the same at
runtime via a `tracing` reload layer — it swaps in a new filter spec (for example
`info,justdns_dns=debug,…`) and rebuilds the callsite-interest cache, so
previously-disabled events start flowing without a restart. It is **not** persisted;
a restart returns to the `RUST_LOG`/default level.

### Endpoints

Three endpoints back this page:

- `GET /api/health` — **unauthenticated**. Returns `{"status":"ok","uptimeSecs":N}`
  and nothing else: no version, no internals, so it is safe to expose to a health
  checker. The container's Docker `HEALTHCHECK` curls it.
- `GET /api/logs/system?level=&limit=` — **admin only**. Returns the recent events
  from the in-memory ring, newest first, optionally filtered to a minimum severity.
- `GET /api/logs/level` / `PUT /api/logs/level` — **admin only**. Read or set the
  live verbosity (`normal`/`debug`/`trace`). The change is applied through the
  reload layer and is not persisted. A set is recorded in the [audit log](users-roles-mfa.md)
  (`logs.level`).
- `GET /api/diag` — **admin only**. Returns the runtime-status snapshot behind the
  tiles (version, uptime, listeners, feeds, cache and list sizes, active clients,
  and the active log level).

The version is deliberately exposed **only** on the authenticated `/api/diag`, never
on the open health endpoint, so an unauthenticated caller cannot fingerprint the
build.

### Implementation

The log capture is a small `tracing` layer that appends each event to a bounded,
shared ring buffer; the same buffer is handed to the web layer so the API reads what
the rest of the process writes. The rotating file is a separate `tracing-appender`
layer (daily rotation, ANSI disabled). The runtime status is a cheap shared handle
the binary updates as listeners come up and feeds refresh, which the diagnostics
endpoint reads on demand — nothing is polled or recomputed per request.
