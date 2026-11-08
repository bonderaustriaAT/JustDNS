# Access requests

*A German version of this guide is in [access-requests.de.md](access-requests.de.md).*

When someone in your network lands on a blocked site, the [block page](block-page.md)
now offers a button — **Ask to unblock this page** — so they can request access
instead of just being stopped. Each request lands in a queue you work through under
**System → Access requests** (admin only). A small badge on that menu entry shows
how many are waiting.

## What the visitor sees

On the block page, under the explanation, there is an **Ask to unblock this page**
button. Opening it reveals an optional reason box (why they need the site) and a
**Send request** button. After sending, the page confirms with *Request sent* — no
account, no login, nothing to install. The visitor cannot release anything
themselves; they are only asking.

The button is shown whenever the block page knows which site was blocked (hosted and
reverse-proxy modes, where the blocked hostname is carried on the request). It does
nothing for a name that is not actually blocked, so it cannot be used to plant
arbitrary entries.

## What a request records

Each queue card shows:

- **The site** — the exact domain that was blocked, as the heading.
- **From** — the device the request came from (its name from your
  [device list](devices.md), and its group if it has one). An address JustDNS does
  not recognise as a named device shows as *Unknown device*.
- **When** — how long ago the request was made.
- **IP address** — the client address it came from.
- **Blocked by** — *what* stopped the site: the custom list or feed that matched, or
  the category (ads & trackers, malware, custom), with the exact matching rule when
  it differs from the domain.
- **Reason** — the visitor's note, if they left one.

## Releasing a site

Each card has a **Release for** picker with three reaches, then **Release** and
**Dismiss**:

- **This device** — only the device that asked may reach the site from now on. The
  domain is added to that device's own allow list (merged with anything already
  there). This is the default.
- **Whole group** — every device in the asking device's group may reach it. The
  domain is added to the group's allow list. Available only when the device is in a
  group.
- **Everyone** — the site is released permanently for the whole network. The domain
  goes on the **global allow list**. Because this is the broadest reach, JustDNS asks
  you to confirm it first.

An allow always wins over a block, so once released the site resolves normally again
for the chosen reach — on the next query, no restart needed. Releasing or dismissing
a request removes its card from the queue; the lasting record is the allow entry
itself (visible and reversible under [Block/Allow](lists.md)) and the audit log.

**Dismiss** clears the request without changing any filtering — use it for a site
you do not want to open.

## In a cluster

In a [high-availability](high-availability.md) cluster the queue is shared: a request
made on whichever node answered the blocked query appears on every node within one
sync interval, and a release or dismissal propagates the same way. Release once, on
any node.

## For the technically curious

The queue is a small JSON store (`access-requests.json`) on the data volume, keyed by
a deterministic id derived from the client address and the domain — so a visitor
hammering the button only ever refreshes one entry rather than piling up duplicates.
The block-page listeners write into it through a sink that first re-checks the policy
engine (`PolicyEngine::explain`) for that exact client and name: a request is recorded
only when the name is genuinely blocked for that client, which is also how the device,
group, category and matching list are filled in. The admin API (`/api/access-requests`,
admin-gated) lists the queue and releases or rejects an entry; a release maps the scope
to the matching allow mechanism — the global allow list, the device's `PolicyDelta`, or
the group's — merging the domain into the existing policy so nothing else is lost. The
store mirrors the other JustDNS stores (atomic write, on-change hook), so it rides the
existing HA ledger merge with last-writer-wins and tombstones.
