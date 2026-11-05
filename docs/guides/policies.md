# Per-device & per-group policies

*A German version of this guide is in [policies.de.md](policies.de.md).*

By default JustDNS filters every device on your network the same way. **Policies**
let you change that: a single device, or a whole **group** of devices, can get its
own rules on top of the global default — stricter for the kids' tablets, looser for
your work laptop.

You manage all of this on the **Devices** page.

## What you can set per device or group

- **Which block lists are on.** Switch any built-in category (Ads & trackers,
  Malware) or any list subscription **on** or **off** just for this scope.
- **Always allow** specific domains — they will never be blocked here, even if a
  list would block them.
- **Always block** specific domains — they are always blocked here, even if nothing
  else would.

An **allow always wins over a block**, everywhere. If a name is on an "always
allow" list, it resolves — full stop.

## How the rules stack (inheritance)

Think of three layers, each building on the one above it:

```
Global default   ← Settings + Lists: applies to everyone
      │  (a group changes only what it wants to)
   Group          ← e.g. "Kids' room"
      │  (a device changes only what it wants to)
   Device         ← e.g. "Living-room tablet"
```

- The **global default** is what you set under [Settings](settings.md) (the Ads and
  Malware switches) and [Lists](lists.md) (your feed subscriptions and manual
  entries).
- A **group** inherits the global default and only changes what differs.
- A **device** inherits its group (or, if it has no group, the global default) and
  only changes what differs.

So you never re-enter everything — you set only the difference at each level. A
device with no group and no rules of its own simply behaves like the global
default.

## Step by step

### Create a group

1. Open the **Devices** page.
2. Scroll to the **Groups** section at the bottom.
3. Type a name (e.g. *Kids' room*) and click **Add group**.

The group starts out identical to the global default. Click **Edit** on it to give
it rules.

### Give a device its own rules

1. On the **Devices** page, find the device and click **Edit**. (Not named yet? See
   [Devices](devices.md) — name it first, or name it as you add rules.)
2. **Group** — optionally put the device in a group.
3. **Block lists** — each list shows three buttons:
   - **Inherit** — use whatever the parent scope decided (the label shows whether
     that means on or off).
   - **On** — force this list on here.
   - **Off** — force this list off here.
4. **Always allow / Always block** — type domains, one per line.
5. Watch the **Active here** line: it shows the block lists that actually apply once
   everything is folded together.
6. Click **Save**.

### Example: looser rules for one laptop

Your global default blocks ads for everyone, but a work tool keeps breaking on your
laptop. Edit the laptop, set **Ads & trackers → Off**, and add the tool's domain
under **Always allow** for good measure. Every other device still gets ads blocked.

### Example: stricter rules for the kids

Create a *Kids' room* group, turn an extra strict list **On** for it, and add a few
game sites under **Always block**. Assign each child's device to the group. Change
the group once and every device in it follows.

## Good to know

- A device is matched by its **IP address** or by a **range** (see the technical
  note below). The most specific match wins.
- Deleting a group does not delete its devices — they simply fall back to the global
  default.
- Everything here is included in your [backup](backup.md).
- Changes take effect immediately, for new lookups — there is nothing to restart.

---

## For the technically curious

**How a client is matched.** Each device entry has a *target*: either a single IP
(`192.168.20.10`) or a CIDR range (`192.168.30.0/24`). When a query arrives, JustDNS
looks at the client's source IP and picks the **most specific** matching device by
longest-prefix: an exact IP (a /32 or /128) beats a range, and a narrower range
beats a wider one. If nothing matches, the client gets the global default. A device
that only carries a name (no group, no rules) is treated as "no policy" and also
resolves to the global default.

**How inheritance is resolved.** Each scope stores only a *delta* against what it
inherits:

- `listsAdded` / `listsDisabled` — list IDs switched on or off relative to the
  parent. A list ID is a feed subscription's ID, or the built-ins `ads` / `malware`.
- `allow` / `block` — ad-hoc single domains.

At rebuild time (on startup and after any change to lists, groups, devices or the
ads/malware switches) JustDNS folds `global → group → device` into **one finished
policy per device** plus one global policy. The DNS hot path never walks the
inheritance tree — it does a longest-prefix client lookup and then a normal
allow/block classification against a ready-made policy. That is why per-client
filtering adds no measurable latency.

**Precedence inside a policy.** Within the resolved policy the order is: allow set
first (an allow short-circuits and the name resolves), then block sources in a fixed
order — malware, your own manual block list, group ad-hoc blocks, device ad-hoc
blocks, your enabled feeds, then ads — and the first matching source wins and is
what the query log reports as the reason.

**Why the cache is still shared.** Blocking, rewriting and Safe Search all run
*before* the response cache, and the cache only stores genuine upstream answers,
which are client-independent. So two clients with different policies safely share
one cache: the policy decides *whether* a client reaches the cache for a name, not
*what* the cached answer is.

**Persistence.** Groups and device deltas live in `clients.json` on the `/data`
volume, with every new field defaulted, so an older file loads unchanged and a file
written today still loads on an older build. They survive restarts and image
updates, and are part of backup/restore.
