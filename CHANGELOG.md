# Changelog

What's new in JustDNS, in plain language. A German version of this file is kept
alongside it in [CHANGELOG.de.md](CHANGELOG.de.md). Dates are year-month-day.

## [0.1.30] - 2026-11-05

- New: **per-device and per-group filtering.** Until now JustDNS filtered every
  client the same. On the "Devices" page you can now give a single device, or a
  whole **group** of devices, its own rules on top of the global default: switch
  individual block lists **on** or **off**, and **always allow** or **always
  block** individual domains — an allow still wins over any block. Rules stack:
  a device inherits its group's rules, and the group inherits the global default,
  so you only set what differs. Each editor shows "Active here", the block lists
  that actually apply once everything is folded together. Create groups in the new
  "Groups" section and assign a device to one from its editor. Everything is
  included in backups.

- Changed: **the "protection strength" level is gone.** The old Off / Minimal /
  Standard / Extreme switch (and the matching step in the setup wizard) has been
  replaced by the global block lists and the ads/malware switches you already see
  under Settings — that is now the default policy, which devices and groups refine.
  Your existing lists and switches are untouched; only the extra level control is
  removed.

## [0.1.29] - 2026-11-05

- Improved: **the response time on the overview is harder to skew now.** A single
  slow upstream lookup could pull the plain average up and out of proportion,
  especially on a quiet day. The overview now also shows the **median and 95th
  percentile** beneath it: the median is the typical query — close to zero when
  most answers come from the cache — while the p95 still shows the slow tail. So a
  cache-heavy setup reads as fast as it really is, and one outlier no longer
  dominates the figure.

- New: **recognise and name your devices.** On the new "Devices" page you can give
  the clients on your network names — a single address (e.g. `192.168.20.10` =
  "Living room TV") or a whole range in CIDR form (e.g. `192.168.30.0/24` = "Guest
  Wi-Fi"). The log and the overview then speak in names instead of IP addresses,
  and the log search finds devices by their name too. The list shows every named
  device plus every recently seen, still-unnamed client, with "last seen", a query
  count and a dot for "active now". Each device can have a name, an optional note
  and a device kind. How long a device still counts as "active" after its last
  query is set in Settings (default 5 minutes). The names are included in backups.

## [0.1.27] - 2026-11-02

- New: **reusable address objects for rewrites.** Give an address a name — say
  `reverse-proxy` for `192.168.20.9` — under the new "Address objects" section on
  the Rewrites page, then point as many rewrites at that object as you like
  instead of typing the address each time. Change the object's address once and
  every rewrite that points at it follows at once. An object can hold several
  addresses (IPv4 and IPv6), and an object that is still in use cannot be deleted
  by mistake — the message tells you which rewrites still reference it. Objects
  are included in backups.

## [0.1.26] - 2026-11-02

- New: **live statistics on the overview.** The overview now shows a live query
  rate — switchable between per second and per minute — alongside last-24-hour
  figures (queries handled, how many were blocked, and the average response time)
  and the five busiest clients with their share of queries.

## [0.1.25] - 2026-11-02

- New: **the dashboard shows which version is running.** A small line at the foot
  of every page now says, for example, "JustDNS v0.1.25" — so you can tell at a
  glance which build a box is on.
- Fixed: **the top bar is no longer hidden on a phone.** When JustDNS was added to
  the home screen, the status bar could sit on top of the header and hide the
  language switch and sign-out. The header now clears the status bar and notch.

## [0.1.24] - 2026-11-02

- New: **install JustDNS as an app, and a redesigned phone experience.** JustDNS
  can now be added to your home screen on a phone or desktop and opens like a
  normal app, full-screen and with its own icon — it even opens the last page
  while your connection is briefly down. When it can be installed, a small
  "Install JustDNS as an app" bar offers a one-tap install on Android and
  Chrome, or shows the "Share → Add to Home Screen" step on an iPhone.
- Improved: **the whole interface is now built for phones.** On a small screen
  the top menu becomes a bottom bar you can reach with your thumb, and every
  page — overview, log, block/allow, rewrites and settings — reflows to fit so
  nothing is cut off or spills sideways. On a computer the layout is unchanged.

## [0.1.23] - 2026-11-02

- New: **restore from a backup during first-time setup.** The very first screen of
  the setup wizard now asks whether you want to set JustDNS up fresh or restore a
  backup. Restoring still creates your login first (a backup never contains one),
  then brings back your settings, lists and rewrites from the file in one step —
  ideal for a new server, a reinstall or a move.

## [0.1.22] - 2026-11-02

- New: **choose what a blocked name answers with.** Under Settings → Protection,
  "Answer for blocked domains" now lets you pick between a null IP (`0.0.0.0` /
  `::`) and NXDOMAIN. **The new default is the null IP** — the same as AdGuard —
  so a blocked ad or telemetry host fails to connect instantly and apps move on.
  This fixes stalls some video players (for example YouTube) had with NXDOMAIN,
  where the player would retry the failed lookup. You can switch back to NXDOMAIN
  at any time.

## [0.1.21] - 2026-11-02

- New: **back up and restore your setup.** Under Settings → Backup & restore you
  can now download your whole configuration as a single file — your settings,
  your allow/block entries with their notes, your subscribed lists and your
  rewrites — and restore it later, or onto a new server. Restoring replaces your
  current setup and takes effect at once. The query log and your login are not
  part of the file.

## [0.1.20] - 2026-11-02

- The "Protection active" dot now shows a gentle sign of life for everyone. If
  you have "reduce motion" turned on, you get a soft fade instead of the
  expanding ring — so the dot still looks live without any large movement.

## [0.1.19] - 2026-11-01

- Fix: **your DNS rewrites now survive updates and restarts.** They are kept with
  the rest of your settings on the data volume, instead of inside the app image
  where a container update could drop them. Rewrites you add from now on are safe.

## [0.1.18] - 2026-11-01

- New: **notes on your allow and block entries.** Add an optional note to any
  domain you allow or block — a reminder of why it is there, like "my own NAS" or
  "smart-TV telemetry". The note shows under the name and can be edited or cleared
  at any time; it is never required and never changes what is resolved.
- Your own allow and block entries now stand out more clearly from the rest of
  the card, so a name you added by hand is easy to spot.
- Rewrites: deleting a rewrite now asks you to confirm first, so nothing goes by
  accident.
- Rewrites: edit a rewrite right in its row, instead of the form at the top
  filling back up.
- Rewrites: the target addresses now line up in neat columns, so a longer list is
  easy to read down.
- The Overview's "Protection active" marker gently pulses, so you can see at a
  glance that it is live.

## [0.1.17] - 2026-11-01

- New: **setup wizard.** On the very first start, JustDNS now walks you through
  setup step by step: administrator account and a name for the instance, the
  protection strength you want (Off/Minimal/Standard/Extreme, each with a plain
  description and a note on its false-positive risk), a resolver profile, and how
  long logs are kept.
- New: **protection strength as one setting.** Under Settings → Protection you
  pick the level with a single switch; it turns on the matching block lists
  automatically. Change individual lists by hand and the level reads "Custom".
- New: **instance name.** Give your JustDNS a name that shows in the top bar —
  handy when you run more than one.

## [0.1.16] - 2026-11-01

- New: **your own resolvers.** Under Settings → Upstream resolvers, you can now
  replace the profile's servers with a list of your own. Each entry takes an IP
  address and a transport — plaintext, DNS over TLS or DNS over HTTPS — plus the
  server name for the encrypted ones. The change takes effect immediately, with
  no restart; until there is a valid entry, the profile servers stay active.

## [0.1.15] - 2026-11-01

- New: **speed profiles.** Under Settings → Upstream resolvers, one switch picks
  between "Privacy", "Balanced" and "Speed". Each profile uses fitting resolvers
  — shown to you right below the switch — and sets how eagerly answers are cached
  and refreshed ahead of time, from privacy-friendly to extra fast.
- Changes to the profile, encryption and selection mode now take effect
  immediately, with no restart.

## [0.1.14] - 2026-11-01

- The top menu bar no longer wraps onto a second line and sits more tidily —
  including in German, where the labels are longer.

## [0.1.13] - 2026-11-01

- The whole interface is now available in **English and German**. A DE/EN switch
  in the top bar changes the language instantly, and your choice is remembered
  the next time you open the dashboard.

## [0.1.12] - 2026-11-01

- **Faster answers.** Frequently used sites are refreshed in the background
  before their entry expires, so you keep getting instant replies instead of an
  occasional slow one. If your DNS providers are briefly unreachable, JustDNS
  keeps answering from recent results so browsing doesn't stall.

## [0.1.11] - 2026-11-01

- The query log now highlights the names you redirect yourself and shows the IP
  address each request resolved to, so you can see at a glance where a device
  was actually sent.

## [0.1.10] - 2026-11-01

- You can now edit your redirects — rename them, point several names at the same
  address, and switch a single one off without deleting it.
- A new "Clear DNS cache" button under Settings makes a changed address or a new
  redirect take effect immediately.

## [0.1.9] - 2026-11-01

- Added "Enforce safe search": switch it on to keep Google, Bing, YouTube and
  DuckDuckGo in their family-safe mode for every device on the network.

## [0.1.8] - 2026-11-01

- You can now give a name your own fixed address — for example to reach a device
  at home under a memorable name like `nas.home` instead of its IP.

## [0.1.7] - 2026-11-01

- When something is blocked, the query details now name the exact list it came
  from, so you can tell why it was stopped.

## [0.1.6] - 2026-11-01

- The allow and block fields now explain how an entry also covers its
  subdomains, so it's clearer what a single entry will catch.

## [0.1.5] - 2026-11-01

- The "Lists" area is now called "Block/Allow", and your own allow and block
  entries live together on one page.

## [0.1.4] - 2026-11-01

- The lists section is split into clear sub-pages reached from a menu, so no
  single page runs long.

## [0.1.3] - 2026-11-01

- Your own lists added by URL now sit in a tidy, collapsible section.

## [0.1.2] - 2026-11-01

- Added a category filter to the ready-made list catalogue, so you can quickly
  find lists for ads, security, telemetry and more.

## [0.1.1] - 2026-11-01

- Manage what's filtered: keep your own always-allow and always-block lists,
  subscribe to lists by URL, and switch on curated ready-made lists with a
  single click.

## [0.1.0] - 2026-11-01

- First release. JustDNS filters ads and malware for every device on your
  network, with:
  - an overview dashboard showing protection status, resolved and blocked
    queries, active devices and a 24-hour activity chart;
  - a live query log you can search by domain or device;
  - settings to turn protection on or off, choose how upstream providers are
    used, and decide whether answers are encrypted;
  - a guided first-run setup that creates your administrator login.
