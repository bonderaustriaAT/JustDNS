# Devices

*A German version of this guide is in [devices.de.md](devices.de.md).*

Your network speaks in IP addresses — `192.168.20.37`, `192.168.20.41` — which tell
you nothing at a glance. The **Devices** page lets you give those addresses friendly
names, so the overview and the query log read *"Living-room TV"* instead of a
number. Naming a device is also the first step to giving it its own
[filtering rules](policies.md).

## The devices list

Open **Devices**. You see two kinds of rows:

- **Named devices** you have added.
- **Seen but unnamed clients** — addresses that have recently made DNS queries but
  don't have a name yet. Each shows a **last seen** time, a **query count**, and a
  dot that lights up when the client is **active now**.

This makes the page read like "what's on my network right now".

## Name a device

1. Click **Name this** on an unnamed client (this pre-fills its address), or type an
   address into the **IP or range** box at the top.
2. Enter a **name** (e.g. *Living-room TV*).
3. Pick a **kind** (phone, laptop, TV, …) — this only sets a small icon.
4. Optionally add a **note** (e.g. *kids' room*).
5. Click **Add**.

From now on, that address shows up by name across JustDNS, and the log search finds
it by name too.

## Name a whole range

Instead of a single address you can enter a **CIDR range**, e.g.
`192.168.30.0/24` named *Guest Wi-Fi*. Every address in that range resolves to the
name — handy for a guest network or an IoT VLAN. If a single address *inside* the
range also has its own named entry, the exact address wins.

## Edit or remove

- **Edit** lets you change the address, name, kind, note, [group and rules](policies.md).
- **Remove** turns the device back into an unnamed sighting — it does not stop it
  querying, it just forgets the name.

## Good to know

- "Active now" means a query was seen within a window you can change under
  [Settings](settings.md) (default 5 minutes).
- Named devices are included in your [backup](backup.md).

---

## For the technically curious

Device names are a **read-time** concern: the log and statistics resolve a client's
source IP to a name when they are displayed, through a resolver built from a snapshot
of your device list. There is no per-query name lookup on the hot path. A device's
*target* is normalised and stored canonically (a masked CIDR base, a canonical IP),
so the file and the UI round-trip exactly what the matcher uses. Matching a client to
a device uses longest-prefix, so an exact `/32` beats any range and a `/24` beats a
`/16` — the same mechanism the [policies](policies.md) feature uses to decide which
rules apply.
