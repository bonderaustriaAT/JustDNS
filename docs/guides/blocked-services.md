# Blocked services

*A German version of this guide is in [blocked-services.de.md](blocked-services.de.md).*

Some things you want to block are not a single domain but a whole product — TikTok,
YouTube, Netflix, a game store. Each of those reaches the internet over a handful of
domains, and blocking it by hand means hunting all of them down. **Blocked services**
does that for you: a curated catalogue with one switch per service, each bundling the
domains that service relies on.

## The global catalogue

Open the **Blocked services** page (under *Rules*, admin only). You get a card for
every service in the catalogue, grouped into categories — social networks, video,
messengers, gaming, music. Filter by category or search by name.

Flip a switch and that service is blocked for the **whole network** by default,
straight away. Flip it back and it is allowed again. The counter at the top tells you
how many services are blocked globally.

This page sets the **global default**, the baseline every client inherits — the same
role [Block/Allow](lists.md) plays for lists.

## Per device, group and schedule

The global switch is rarely the whole story. You will often want a service blocked
everywhere *except* your own phone, or blocked for the kids' group, or blocked only
during homework hours. That is where the per-scope controls come in.

In the [device and group editors](policies.md) and in the [schedule editor](schedules.md)
each service shows the same three-state control as a block list:

- **Inherit** — follow the level above (the global default, or the group).
- **On** — block the service here, even if it is off above.
- **Off** — allow the service here, even if it is on above.

So you can block TikTok globally and switch it **Off** for your own device, or leave it
alone globally and switch it **On** for a single group between 18:00 and 20:00 with a
[schedule](schedules.md).

## Good to know

- **An allow always wins.** If a domain is on your [allow list](lists.md), it is let
  through even when a blocked service would catch it.
- When a service blocks a lookup, the [query log](query-log.md) names the service (for
  example `TikTok`) as the reason, so a block is never a mystery.
- Which services are blocked globally is part of your [backup](backup.md).
- Everything applies live to new lookups — no restart.
- Services are **off by default**: a fresh install blocks nothing here until you say so.

## Good to know about coverage

The catalogue aims at the domains a service genuinely needs, not every domain it has
ever touched. That keeps collateral damage low, but a determined app may still reach
some content through a shared CDN the catalogue leaves alone. For an airtight block,
combine a service switch with an aggressive block [list](lists.md).

---

## For the technically curious

**One mechanism, not a new one.** A service is just another block source in the
[policy engine](policies.md). The catalogue is compiled into the binary (like the
bundled ads/malware starter sets), so it needs no network access. Each service carries
a small [`DomainSet`](lists.md) of its registrable roots, matched exactly or by
sub-domain — an entry for `tiktok.com` matches `tiktok.com` and any `*.tiktok.com`.

**How a switch travels.** Every service has a list-object id of the form `svc:<id>`
(for example `svc:tiktok`). The global switch stores the bare id in
`settings.blocked_services`; a per-scope switch writes the `svc:<id>` id into that
scope's policy delta — the very same `listsAdded` / `listsDisabled` channel the block
lists use. Nothing in the policy editor, the inheritance maths or the schedule folding
knows a service from a list, so per-client resolution, live rebuild, "allow wins" and
scheduling all come for free.

**Resolution order.** At rebuild time each active service becomes a block source
labelled with the service's name and slotted in with the feeds. The engine still checks
the allow set first (an allow short-circuits), then the block sources; the first match
is the reason the [query log](query-log.md) records.

**Extending the catalogue.** The list lives in `crates/justdns-filter/src/services.rs`
as a static table of `Service { id, name, category, domains }`. Adding a service is one
entry there plus its domains; a test asserts every listed domain survives the rule
parser, so a typo fails the build rather than silently dropping a domain.
