# Why a query was blocked (decision trace)

*A German version of this guide is in [decision-trace.de.md](decision-trace.de.md).*

Every entry in the [query log](query-log.md) can tell you **why** JustDNS answered it the
way it did. A coloured **reason tag** on the row names the cause at a glance, and opening
the entry reveals the **decision trace**: the ordered stages the resolver ran, with the
one that made the decision called out. It is how you answer "why was *this* blocked?"
without guessing, and it puts the fix one click away.

## The reason tag on the row

A blocked or warned row carries a small coloured tag right next to the name, so you can
scan the log and see the cause without opening anything:

- **Block list** (red) — a name on one of your [block lists](lists.md) or a
  CNAME-cloaked tracker that was uncovered.
- **Threat IP** (blue) — the answer resolved to an address on a response-IP
  [threat list](lists.md), so the whole answer was refused.
- **Phishing** (violet) and **Warning** (amber) — a [phishing](phishing.md) block or a
  warn-only hit. These already carry the phishing marker, so they keep it.

## Opening the trace

Click a row to expand it. Below the record and the block reason is a **Decision** section:
the stages the query passed through, in the order they ran.

Each step shows the **stage**, a **mark** for what it did, and an optional detail (the
answering resolver, the imitated brand, the response code):

- **✕ hit** — this stage made the block. There is only ever one.
- **! warning** — this stage flagged the query but let it through.
- **✓ passed** — this gate ran and let the query through.
- **· info** — a neutral note, such as the answer's source or the final response code.

The stages, in pipeline order, are **Block list**, **Phishing**, **Source**,
**CNAME uncloak**, **Response IP**, **DNSSEC** and **Answer**. A query only lists the
stages that actually ran.

To keep a clean answer quiet, the gates that merely **passed** are folded away; the
decisive stage and the neutral notes show at once. Use **Show passed stages** to reveal
the rest.

## The pause banner

If you had the [troubleshooting pause](query-log.md) armed when the query came in, a
banner at the top of the trace says so. While the pause is on, the protective stages are
skipped, so they are not listed: the trace shows how the query was actually handled, not
how it would have been with protection on.

## Jump to the rule

The decisive stage is a **link**. It takes you straight to the page that governs that
rule, with the right place highlighted:

- A **name block** or **CNAME uncloak** opens your own [lists](lists.md) at the allow
  editor, so you can add the name as an exception in one step.
- A **response-IP** block opens the [lists](lists.md) presets, pre-filtered to the
  threat-IP lists.
- A **phishing** hit opens the [phishing protection](phishing.md) page.

So when a block is a false positive, you are one click from the page that fixes it.

## In a cluster

In a [high-availability](high-availability.md) cluster, the trace also names the node
that made the decision, so you can tell which server handled a query when devices use
more than one.

---

## For the technically curious

The trace is built once, as the query is handled, from the signals the resolver already
has: which stage refused a blocked answer, whether the troubleshooting pause was on, the
phishing verdict, the answer source and resolver, the DNSSEC verdict and the response
code. The reason tag is derived from the decisive stage (a name or CNAME block →
*block list*, a response-IP block → *threat IP*, a phishing block → *phishing*, a
warn-only hit → *warning*).

**Where it lives.** The full trace is stored only in the durable DuckDB log on disk, as
one extra column on the query row. It is **never** held in the in-memory ring that feeds
the live tail: the hot view keeps only the tiny reason tag, and the trace is stripped
from it before the row enters the ring. When you open an entry, the UI fetches the trace
on demand for that one row (`GET /api/queries/trace`, matched on the row's time, client,
name and record type). So the explainability costs nothing on the query path and nothing
in the resolver's memory, and an older row recorded before this feature simply shows no
trace.

**Why pass steps are kept.** The trace records the gates that passed, not just the one
that blocked, so the drawer can show the whole path a query took — useful for confirming
that, say, the name block and phishing stages both ran and cleared before an answer was
served. They are folded away by default so a plain answer stays readable.
