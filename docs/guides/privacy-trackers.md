# Privacy dashboard

*A German version of this guide is in [privacy-trackers.de.md](privacy-trackers.de.md).*

The [query log](query-log.md) answers *what* was asked. The **Privacy** page answers
*who is behind it*: it takes every name your devices resolved and attributes it to the
**company** that runs it, so you can see at a glance that a phone reached Google 240
times today, or that a smart TV keeps talking to an analytics firm you have never heard
of. It turns a dry list of domains into a story you can show someone who does not read
DNS logs.

Open it from **Privacy** in the sidebar (the eye-in-shield icon, near the overview and
the query log). It is readable by any signed-in user.

## What you see

At the top, three figures for the whole network over the chosen window:

- **Tracker contacts** — how many lookups went to a company in the catalogue.
- **Blocked** — how many of those JustDNS turned away, with the blocked share.
- **Got through** — how many were answered, because no list caught them.

Then a one-line headline (*"living-room-tv contacted Google 1,204×"*), the
**most-contacted companies** across all devices, and a **by-device** list. Each company
row carries a coloured dot for its category and a bar whose green part is the share that
was blocked — so a mostly-green bar means your lists are doing their job, and a mostly
amber one is a company that is getting through.

Pick the window — **24 hours**, **7 days** or **30 days** — with the switch in the top
right. Your choice is remembered on this browser.

### Drilling into a device

Tap any device to see only the companies *it* reached, busiest first. This is the view
to open when one device looks unusually chatty: it names the organisations behind the
traffic, and shows how much of it was blocked versus answered. Use **‹ All devices** to
go back.

## Categories

Every company in the catalogue is filed under one of four families, each with its own
colour:

| Category | What it means |
| --- | --- |
| **Advertising** | Ad networks and exchanges that serve or auction ads. |
| **Analytics** | Measurement, attribution and session-recording services. |
| **Social media** | Social platforms' tracking and share widgets. |
| **Fingerprinting** | Device-fingerprinting, bot-detection and fraud-scoring services. |

## What it does *not* show

- **Only node-local queries.** The dashboard counts lookups this node answered. In a
  [high-availability](high-availability.md) cluster, each node shows its own traffic;
  it does not pull a peer's.
- **Only names in the catalogue.** A lookup that belongs to no catalogued company is
  simply not counted here — it is not "unknown traffic", just traffic this feature does
  not attribute. A quiet dashboard can mean a quiet network, or lists that blocked most
  trackers upstream before they could be attributed.
- **Not a block control.** This is a read-only view. To act on what you find, use
  [block & allow lists](lists.md), [blocked services](blocked-services.md) or a
  [per-device policy](policies.md).

---

## For the technically curious

**Where the mapping comes from.** The company catalogue is compiled into JustDNS, like
the blocked-services list — a curated set of the large advertising, analytics, social
and fingerprinting networks and the registrable domains they use. It is deliberately not
exhaustive, and it ships in the binary, so there is no feed to fetch and nothing phones
home. It is not DuckDuckGo's Tracker Radar: that dataset is licensed CC BY-NC-SA
(non-commercial), which does not fit an AGPL product, so the catalogue here is
independent and hand-curated.

**How a name is attributed.** Attribution walks the name's parent labels, exactly like
the block engine: `px.ads.doubleclick.net` tries the whole name, then
`ads.doubleclick.net`, then `doubleclick.net` — which is in the catalogue — and stops,
crediting Google. A name that reaches the top without a hit is left unattributed. Each
registrable domain belongs to exactly one company, so a lookup is never double-counted.

**How it is computed.** One grouped DuckDB read over the window returns, per device and
per name, how many lookups there were and whether they were blocked; the handler maps
each name through the catalogue and sums the hits per company and per device. The scan is
capped, and the whole thing is one query per page load — it does not run on the hot path,
so nothing here slows down resolving.
