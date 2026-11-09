# Settings

*A German version of this guide is in [settings.de.md](settings.de.md).*

The **Settings** page is where you set JustDNS's global behaviour — the baseline that
applies to your whole network. (To make exceptions per device or group, see
[policies](policies.md).) Changes are saved automatically as you make them.

## Protection

- **Instance name** — a label shown in the top bar, handy if you run more than one
  JustDNS. Display only; it does not affect filtering.
- **Ads & trackers** — turn the built-in ad/tracker block set on or off for the
  global default.
- **Malware & phishing** — turn the built-in malware block set on or off.
- **Safe Search** — force Google, YouTube, Bing and DuckDuckGo to their filtered
  endpoints, network-wide.
- **Block response** — what a blocked name answers with:
  - **Null IP** (`0.0.0.0` / `::`) — the default; the device gets a dead address.
  - **NXDOMAIN** — the device is told the name does not exist.

  Most setups are fine with Null IP; see the technical note for when NXDOMAIN helps.

> For your own block lists and feed subscriptions, see [Lists](lists.md). The Ads and
> Malware switches here are the on/off switches for the two built-in categories.

## Resolvers & speed

- **Speed profile** — one switch that balances privacy against speed:
  **Privacy**, **Balanced** (recommended) or **Speed**. It picks the resolvers and
  how eagerly answers are cached.
- **Encrypt upstream** — talk to your resolvers over DNS-over-TLS instead of plain
  DNS, so your lookups are private in transit.
- **Custom resolvers** — switch this on to use your own resolvers instead of the
  built-in ones. Each can be plain DNS, DNS-over-TLS (DoT) or DNS-over-HTTPS (DoH).
- **Upstream mode** — how JustDNS uses several resolvers at once (race them for
  speed, fail over, balance, or follow rules). Leave it on the default unless you
  know you want something else.

## Logging & housekeeping

- **Query log** — whether JustDNS records each lookup (needed for the
  [query log](query-log.md) page and the statistics).
- **Keep logs for** — how many days of history to keep before old entries are
  dropped.
- **Active window** — how long after its last query a device still counts as "active
  now" on the [Devices](devices.md) page (default 5 minutes).
- **IPv6** — whether JustDNS answers AAAA (IPv6) records.
- **Clear DNS cache** — drop every cached answer now, so the next lookup for each
  name is fresh. Useful after changing lists or resolvers.

Some advanced controls are hidden until you turn on **Expert mode** (a per-browser
preference).

---

## For the technically curious

**Block response.** Null IP (`0.0.0.0` / `::`) returns a valid-but-dead A/AAAA answer,
so clients fail fast without a lookup error. NXDOMAIN signals "no such name", which
some clients and browsers handle more gracefully (no connection attempt at all), but
can make a few apps retry or behave oddly. Both are valid; Null IP is the safe
default.

**Speed profile vs. upstream mode.** The speed profile is the single knob most people
use: it selects a resolver pool and cache aggressiveness (pre-fetching, serve-stale).
Upstream mode is the finer control over *how* multiple resolvers are used —
racing them and taking the first answer (lowest latency), fallback ordering,
balancing, or rule-based selection. `encryptUpstream` upgrades the built-in pool to
DoT; `customUpstreams` replaces the pool entirely with your own entries, each with
its own transport (plain / DoT / DoH), port and optional TLS server name.

**The cache.** Only genuine upstream answers are cached, and they are
client-independent, which is what lets per-client [policies](policies.md) share one
cache. Filtering, rewriting and Safe Search all run before the cache, so a blocked or
rewritten name never pollutes it. "Clear DNS cache" empties it immediately; a profile
change or a resolver change does not retroactively rewrite cached entries, so clear
the cache if you want an instant clean slate.

**Persistence.** All settings live in `config.json` on the `/data` volume and are
loaded on startup. Unknown fields are ignored, so a config written by a newer or
older build still loads — which is how the retired "protection level" field from
earlier versions is silently dropped without breaking your file.
