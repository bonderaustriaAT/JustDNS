# Schedules

*A German version of this guide is in [schedules.de.md](schedules.de.md).*

Policies in JustDNS normally apply **around the clock**. **Schedules** add the
missing dimension — **time**. A schedule is a recurring weekly window (a set of
weekdays plus a start and end time) that, while it is open, either **pauses** a
target completely ("internet off") or applies an **extra filtering delta** (more
lists, or a few domains blocked or allowed). Outside the window it does nothing.

You manage all of this on the **Schedules** page.

## What a schedule is made of

- **Name** — how you recognise it in the list (e.g. *Bedtime*).
- **Active** — the master switch. A disabled schedule never applies, whatever its
  times say.
- **Weekdays** — the days the window opens on. For a window that runs past
  midnight, this is the day it **starts** (see below).
- **From / To** — the window, as local `HH:MM` times.
- **Target** — who it applies to:
  - **Everyone (global)** — the whole network.
  - **Group** — one device group.
  - **Device** — one named device.
- **Effect** — what it does while open:
  - **Pause** — block every request for the target. Your **always-allow** list
    still wins, so allow-listed names stay reachable (handy for a school portal
    or an emergency number).
  - **Lists/domains** — layer an extra delta on top of the normal filtering:
    switch more block lists on, or block/allow individual domains, exactly like a
    device or group policy.

## How schedules combine with policies

A schedule does not replace your normal filtering — it is **added on top** while
the window is open:

```
Global default  →  Group  →  Device        (your normal policy, always on)
        +  active schedules for that scope  (only while their window is open)
```

- A **pause** always wins: while a pause window is open for a target, everything
  is blocked for it (minus the allow list), no matter what else is set.
- A **lists/domains** schedule adds its lists and domains to whatever the target
  already has. An allow it adds still wins over a block, as everywhere in JustDNS.
- Several schedules can target the same scope at once; their effects combine, and
  any active pause dominates.

## Time zone

Windows are evaluated in the **time zone** you set under
[Settings](settings.md) → *Time zone for schedules*. Pick your local IANA zone
(e.g. `Europe/Vienna`) so that "22:00–07:00" means local bedtime, and so the
window follows daylight-saving changes automatically. An unknown value falls back
to UTC. The default is UTC.

## Windows that cross midnight

If **To** is earlier than (or equal to) **From**, the window runs **past
midnight**. The weekday set is read on the day the window **starts**. So a
*Bedtime* schedule of **22:00–07:00** on **Mon–Fri** covers Monday 22:00 through
Tuesday 07:00, and so on through Friday night into Saturday morning — exactly
what "school nights" means. The page shows an *overnight* note when this applies.

## Step by step

### Create a schedule

1. Open the **Schedules** page and click **New schedule**.
2. Give it a **Name**.
3. Pick the **Weekdays** it should run on (at least one).
4. Set **From** and **To**. Watch the hint: it tells you whether the window stays
   within a day or crosses midnight, and which time zone it is read in.
5. Choose a **Target**: everyone, a group, or a device.
6. Choose the **Effect**:
   - **Pause** — nothing more to set.
   - **Lists/domains** — the policy editor appears; switch lists on/off and add
     always-allow / always-block domains, just like on the Devices page.
7. Click **Create**.

### Edit, disable or delete

- **Edit** reopens the form. **Save** applies your changes at once.
- The **Active** toggle on each row disables a schedule without deleting it —
  useful during holidays.
- **Delete** removes it after a confirmation.

Changes take effect immediately for new lookups — there is nothing to restart.

### Example: internet off at bedtime

*Bedtime*, **Mon–Fri**, **22:00–07:00**, target **Group: Kids' room**, effect
**Pause**. Every device in that group is blocked overnight on school nights. Add a
reading-app domain to the group's always-allow list if it should stay reachable.

### Example: no social media during focus hours

*Focus*, **Mon–Fri**, **09:00–12:00**, target **Device: Office PC**, effect
**Lists/domains**, with the social-media domains under **Always block**. The rest
of the day is unaffected, and other devices never see the rule.

## Good to know

- A schedule that points at a deleted group or device is simply ignored until you
  fix or remove it — it never breaks resolution.
- Everything here is included in your [backup](backup.md).
- Schedules add no measurable latency: they are folded into ready-made policies,
  and the DNS hot path still only matches a finished policy (see below).

---

## For the technically curious

**When "now" is decided.** JustDNS keeps the current local time in your configured
IANA zone (via `chrono` + `chrono-tz`). A schedule is *active now* when it is
enabled, today's (or, for an overnight window, the start day's) weekday is in its
set, and the current local time is inside `[start, end)`. For an overnight window
(`end <= start`) a time is inside when it is `>= start` on a listed day **or**
`< end` on the day after a listed day. A window with `start == end` covers the
whole listed day.

**How it reaches the hot path.** Active schedules are folded into the same policy
rebuild that handles global/group/device inheritance: their deltas are added to
the matching scope, and a pause turns the resolved policy into a *block-all*
policy (the allow set still pierces it). The result is one finished policy per
device plus a global one — the DNS hot path never evaluates a schedule directly,
so per-request cost is unchanged.

**What re-runs the fold.** Two triggers: any change to a schedule (add/edit/
delete/toggle) rebuilds immediately, and a once-a-minute ticker recomputes the set
of currently-open schedules and rebuilds **only when that set changes** (a window
just opened or closed). Minute granularity matches the `HH:MM` windows and is
robust across daylight-saving transitions — there is no fragile "next boundary"
arithmetic. The time zone is re-read on each tick, so changing it in Settings
takes effect without a restart.

**Pause semantics.** A pause is realised as a policy whose block sources are
cleared and whose "block everything" flag is set, labelled with the schedule's
name. Classification still checks the allow set first, so always-allowed names
resolve even during a pause; everything else is answered with your configured
block response (the null IP by default, or NXDOMAIN).

**Persistence.** Schedules live in `schedules.json` on the `/data` volume. Each
new field is defaulted, so an older file loads unchanged and a file written today
still loads on an older build. They survive restarts and image updates, and are
part of backup/restore (older backups without a `schedules` section simply restore
with no schedules).
