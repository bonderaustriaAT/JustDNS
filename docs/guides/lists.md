# Block & allow lists

*A German version of this guide is in [lists.de.md](lists.de.md).*

This is where you decide what JustDNS blocks for the whole network by default, and
what it should always let through. Open the **Block/Allow** page.

Everything here sets the **global default** — the baseline every client gets. To make
exceptions for one device or group, see [policies](policies.md).

## The three things on this page

### 1. Always allow

Domains on this list are **never** blocked — even if an active block list would block
them. This is how you fix a *false positive*: a site you want that some list catches
by mistake.

- Add a domain (e.g. `my-bank.example`), optionally with a note explaining why.
- An entry automatically covers all subdomains: `example.com` also allows
  `www.example.com`. To allow just one host, enter exactly that host
  (`login.example.com`).

**An allow always wins over a block.** It is the strongest rule in the system.

### 2. Always block

Your own manual block list — domains you want gone regardless of any subscription.
Same subdomain rule applies (`tracker.example` also blocks `ads.tracker.example`).

### 3. Subscriptions & the catalogue

A **subscription** is an external block list (a "feed") that JustDNS downloads and
keeps updated — for example the curated
[Hagezi](https://github.com/hagezi/dns-blocklists) lists. The **catalogue** offers a
set of ready-to-use feeds; switch one on and JustDNS fetches it.

Each catalogue entry shows a **false-positive risk** (1 = negligible … 5 = high) so
you can judge how aggressive it is before turning it on. You can also add your own
feed by URL.

Click **Refresh all lists** to re-download every feed now; otherwise they refresh on
their own schedule.

## The built-in categories

Two block sets ship inside JustDNS and are controlled by simple switches under
[Settings](settings.md), not here:

- **Ads & trackers**
- **Malware & phishing**

They behave like any other list in the [policy](policies.md) editor, where they
appear as `Ads & trackers` and `Malware`.

## Good to know

- Allow and block entries, your subscriptions and which catalogue feeds are on are
  all part of your [backup](backup.md).
- Changes apply live to new lookups.

---

## For the technically curious

**Matching.** Entries are host rules matched by domain suffix: an entry for
`example.com` matches `example.com` and any `*.example.com`, but not `notexample.com`.
Full `*.example.com` wildcard syntax is accepted and treated the same as
`example.com`; partial wildcards like `ads.*.com` are not.

**Resolution order.** At rebuild time your manual allow entries and every enabled
*allow* feed merge into one allow set; your manual block entries and every enabled
*block* feed become block sources; the built-in ads/malware sets are added according
to their switches. The [policy engine](policies.md) then checks allow first (an allow
short-circuits), then block sources in a fixed order. The first matching block source
is what the [query log](query-log.md) names as the reason.

**Feeds on disk.** Subscriptions are cached under the `/data` volume so the blocker is
populated immediately on startup with no network round-trip; a scheduled refresh (and
the **Refresh all** button) re-fetches them. Your list configuration persists across
restarts and image updates.
