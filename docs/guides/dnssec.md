# DNSSEC validation

*A German version of this guide is in [dnssec.de.md](dnssec.de.md).*

**DNSSEC** is the signature system that lets a resolver check that a DNS answer
really came from the zone it claims to, unaltered on the way. Without it, anyone
who can tamper with your traffic can hand back a forged address for a name. With
validation on, JustDNS checks those signatures itself and **refuses an answer that
fails** — so a tampered reply becomes a hard error instead of a wrong address.

The **DNSSEC** page (under *Monitoring*) shows what that checking found.

## The four verdicts

Every forwarded lookup gets one of four verdicts:

- **Secure** — a valid, unbroken chain of signatures from the root down to the
  answer. The answer is genuine.
- **Insecure** — the zone is not signed, so there is nothing to verify. This is
  normal: most of the internet is still unsigned.
- **Bogus** — signatures were present but did **not** validate. JustDNS refuses
  the answer and returns **SERVFAIL**, so the client never sees the forged data.
  This is the one worth noticing.
- **Indeterminate** — no verdict could be reached, for example because the chain
  of trust could not be evaluated.

## Reading the page

- **Four counter cards** at the top show how many lookups fell into each verdict.
- **Click a card** to filter the table below to only that verdict; click it again
  (or *Clear filter*) to show everything.
- If any answer has been refused as **Bogus**, a red **alert banner** appears at
  the top — a tampered or badly signed zone is worth a look.
- The **table** lists the most recent validated lookups: time, device, name, and a
  coloured **verdict badge**. Bogus rows are highlighted so they catch the eye.

## Turning it on

DNSSEC validation is **off by default**. It is switched on in the server
configuration with the **`JUSTDNS_DNSSEC`** environment variable (set it to `1`,
`true` or `on`) and takes effect when JustDNS starts. While it is off, no verdicts
are recorded and the page stays empty.

---

## For the technically curious

JustDNS is a **forwarder**, not a recursive resolver, so it leans on the validating
upstream resolver (the configured DNS4EU and Quad9 endpoints both validate) rather
than re-walking the signature chain itself. Doing the full chain walk over a
forwarder is unreliable — it cannot always fetch the `NSEC`/`NSEC3` denials that
*prove* a delegation is unsigned — and an earlier version that tried paid for it by
marking perfectly good unsigned domains **Bogus**. The verdict is now derived from
the upstream's own signals:

- The query carries the EDNS **DO** bit, which asks the upstream to validate and to
  report the result in the **AD** (Authentic Data) flag. An answer **with** AD is
  **Secure**; a plain answer **without** AD comes from an unsigned zone and is
  **Insecure**.
- A validating upstream withholds a signature failure as **SERVFAIL** rather than
  returning the data. To tell that apart from an ordinary upstream failure, JustDNS
  retries the query once with the **CD** (Checking Disabled) bit set: if the data
  then comes back with checking off, the original SERVFAIL was a validation failure
  — **Bogus**. If it still fails, it is treated as a genuine upstream error.

A **Bogus** verdict is turned into **SERVFAIL** with no answers and is **not
cached**, and neither is it refreshed into the cache by the background prefetch, so
a forged or tampered answer never reaches a client. (Negative answers — NXDOMAIN /
NODATA — carry no verdict, because the AD flag is not available to us on that path.)

The verdict is produced **only while validation is on** — with DNSSEC off the DO
bit is not set and no verdict is ever reported, so the query log is not flooded and
the *Bogus → SERVFAIL* enforcement stays inert. The verdict is persisted alongside
the rest of the query event in the DuckDB store, and the counter cards are the
lifetime totals from the in-memory statistics.

Validation adds at most one extra round-trip on a cache miss (only when the upstream
SERVFAILs, for the CD retry); normal answers cost nothing beyond the DO bit, and
cache hits are unaffected.
