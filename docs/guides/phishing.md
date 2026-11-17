# Phishing protection

*A German version of this guide is in [phishing.de.md](phishing.de.md).*

Block lists work by membership: if a name is on the list, it is blocked. Against
targeted **phishing** that barely helps. A freshly registered lookalike domain —
a Cyrillic `а` in `pаypal.com`, a `micros0ft-login.com`, a Punycode name encoded
as `xn--…` — is on no list, because it did not exist yesterday. So **phishing
protection** adds a **computed** detection layer alongside the blocker:

- **Impersonation detection** — confusable characters (homoglyphs) and Punycode,
  checked against a list of protected brands. Fully local, no external lookup.
- **Newly registered domains (NRD)** — a subscription to fresh domains through the
  list infrastructure that does **not** feed the blocker, but goes into phishing
  protection on its own.

The **Phishing protection** page sits in the menu directly under *DNSSEC*.

## Warn or block

For each of the two categories you choose between two modes:

- **Warn** — the answer stays **unchanged**; the hit is only marked on the query
  log and, if you like, reported over a webhook. This is the default: a lookalike
  check has false positives, so warning is the safe choice.
- **Block** — the name is refused like a normal block, before any lookup. Pick
  this deliberately, especially for NRD: many fresh domains are harmless.

"Warn" is not a separate answer path, just a note — exactly like the DNSSEC
verdict. The client gets the same answer it would without phishing protection.

## Using the page

- At the very top a **master switch** turns phishing protection as a whole on or
  off. While it is off, nothing is checked and the cards below have no effect.
- The **Impersonation detection** card has a **Warn/Block** segment and, below it,
  the protected brands sorted by group (*Tech & Mail*, *Banks AT*, *Banks DE*).
  Each brand is a chip: tap it to switch a shipped brand off or back on. The input
  field adds your **own brands** by domain (say `my-company.com`) and removes them
  again.
- The **Newly registered domains** card has its own switch, a **Warn/Block**
  segment, and the **NRD feeds** to click on (see below). The more windows are
  active, the wider the coverage — and the more memory the list costs.
- A **webhook switch** also reports hits through the global
  [notifications](notifications.md).
- Below that, a **hits list** shows the most recent phishing hits from the log,
  each with its kind (*Lookalike*, *Punycode*, *Newly registered*) and mode
  (*Warned* or *Blocked*).

A hit also shows up directly in the [query log](query-log.md): as a coloured badge
on the domain (amber for warn, red for block) and as its own field in a row's
detail view.

## Clicking the NRD feeds

The newly registered domains arrive as a list subscription, split into time
windows, each activatable with one click:

- **Last 7 days** — the smallest window, around 40 MB in memory.
- **Days 8 to 14** — together with the 7-day window, the full last 14 days.
- **Days 15 to 21** and **Days 22 to 28** — for broader coverage, if memory
  allows.

The source is the openly maintained **HaGeZi** NRD project (GPLv3). The windows
are split on purpose: each file stays small enough, and you load only what you
actually need. On a small box the 7-day window alone is often enough. None of the
windows is marked as recommended, so you never pull tens of MB by accident.

## Turning it on

Phishing protection is **off by default**. Everything is driven from the dashboard
— the master switch, the modes, the brands and the NRD feeds. A change takes
effect **immediately**, with no restart. While the master switch is off, nothing
is checked and nothing is logged.

---

## For the technically curious

Phishing protection is not a second blocker in the hot path, but a single
evaluation of the queried name before resolution, right where the existing block
check sits:

- **Punycode.** A label with `xn--` is decoded to Unicode and then matched against
  the brands, so a name that reads harmlessly but decodes to an imitation of a
  brand is caught.
- **Homoglyph.** For each registrable label a "skeleton" is built — a curated,
  compiled confusable table maps lookalike characters to a normal form (`0→o`,
  `1→l`, Cyrillic `а→a`, Greek …). If the skeleton matches that of a protected
  brand but the domain is **not** the brand's real domain, that is a hit. The real
  domain and legitimate internationalised names deliberately do **not** fire.
- **NRD.** The registrable domain is tested against the active NRD set — plain set
  membership, like the blocker.

The effective brand list is *shipped − disabled + your own*. The detector holds
its configuration in an atomic swap; when something changes on the dashboard or an
NRD feed is refreshed, the configuration is rebuilt and swapped in without
interrupting resolution. A **warn** hit is stored alongside the query event in the
DuckDB store, just like the DNSSEC verdict; a **block** hit writes the block answer
and returns before the upstream is even asked. Rewrites and your own names are
exempt.

On memory: an NRD set is large. The 14-day window is roughly 86 MB, the full 28
days several hundred MB in memory. On a small box it pays to start with the 7-day
window and widen only when you need to.

**Not wired yet:** an optional online check of the registration date over **RDAP**
is provided for in the configuration (off by default, and the only path on which a
domain name would leave the box), but is not yet exposed in the dashboard. Until
then, which NRD windows you have active alone decides how far "newly registered"
reaches offline.
