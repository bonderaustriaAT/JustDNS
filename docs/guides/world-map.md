# World map and watched networks

*A German version of this guide is in [world-map.de.md](world-map.de.md).*

The [GeoIP feature](geoip.md) already tells you the country and network behind every answer.
The world map turns that into a picture, and the watch list turns it into an action: you can
mark a country or network as **watched**, and JustDNS then flags every answer that resolves
into it — across the whole network, or just for one device or group.

This is a **read-path marker only**. Watching never blocks anything and never touches the
DNS hot path; it highlights and, if you want, alerts. To actually block a destination, use a
device or group [policy](policies.md) as usual.

## The map

On the **Privacy** dashboard, the **Where your devices phone home** section opens with a
world map. Each country your network reached over the chosen window is shaded by how many
answers resolved into it — the busier the country, the stronger the green. The ranked list of
networks (ASNs) sits below it.

Click a country to focus it: the map rings it and the list filters to that country's networks.
Click it again, click the ocean, or use **Show all countries** to clear the filter.

The map is a bundled, offline SVG — no map library and no network calls. It is the
*Simple World Map* by Al MacDonald (original artwork) and Fritz Lekschas (editor), used under
[CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/).

## Watching a country or network

As an administrator (on the primary node of a cluster), selecting a country shows a **Watch
country** switch in the filter bar, and each network row in a focused country shows a
**watch** switch. These edit the **global** watch list — they apply to every device.

For a narrower watch, open a device or group on the **Devices** page and use the **Watch
countries** and **Watch networks (ASN)** boxes in its policy. A device's effective watch set
is the union of the global list, its group's list and its own.

- Countries are ISO 3166-1 alpha-2 codes, space- or comma-separated (for example `ru cn`).
- Networks are ASN numbers, with or without an `AS` prefix (for example `13335 15169`).

## What watching does

- **In the query log** a watched row carries an amber **watched** badge and a quiet amber
  edge, so a device talking to a marked country or network stands out while you scroll.
- **As an alert** (optional, off by default): turn on **Watched country/network contacted**
  under Notifications, and JustDNS posts to your webhook when a recent answer resolved into a
  watched network. It is evaluated node-locally with a per-network cooldown so a persistent
  contact alerts at most once an hour, and it respects each client's effective watch set
  (global, group and device).

## For the technically curious

- The watch set is resolved the same way for the log flag and the alert, so both agree.
- Nothing is stored on the query itself: the "watched" flag is worked out as the log is read,
  so changing the watch list re-labels older rows immediately.
- The global watch list lives in the synced settings, so it is shared across a
  high-availability cluster; per-device and per-group watches ride the normal device policy.
- The alert never fires from the DNS hot path — a small background pass scans recent answers,
  exactly like the device-spike alert.
