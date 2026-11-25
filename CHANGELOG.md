# Changelog

What's new in JustDNS, in plain language. A German version of this file is kept
alongside it in [CHANGELOG.de.md](CHANGELOG.de.md). Dates are year-month-day.

## [0.48.0] - 2026-11-25

- New: **See why a query was acted on (decision trace).** Every entry in the query log now records the
  path the resolver took through its decision stages. Open an entry and the detail panel shows the
  ordered stages (block list, phishing, source, CNAME uncloak, response-IP, DNSSEC, answer): the stage
  that made the decision is called out, the gates that merely passed fold away behind a toggle, and a
  banner shows when the troubleshooting pause was in effect. A coloured reason tag on the row marks the
  cause at a glance (block list, threat IP, phishing or a warning). The decisive stage links straight
  to the page that governs it, so a false positive is one click from the fix. The full trace is kept
  only in the durable log on disk and read on demand, so it adds nothing to the resolver's memory on
  the query path.

## [0.47.2] - 2026-11-24

- Changed: **Alerts fire the moment a threat is caught.** The notification webhook for a phishing or
  newly-registered-domain block used to wait for a periodic check and could take up to a minute to go
  out. A block now wakes the notifier immediately, so the webhook arrives within seconds. The periodic
  check stays as a fallback for the conditions that are not tied to a single block.
- Fixed: **Hardening from a full security review.** Several defensive fixes: list feeds and
  notification webhooks can no longer be pointed at internal metadata or link-local addresses and no
  longer follow redirects (SSRF guard); a response-IP block now also catches an AAAA answer whose
  sibling A record is on the block list; a read-only follower no longer accepts account changes, which
  the authoritative sync from the primary would silently revert; and a one-time TOTP code can no longer
  be replayed within its time step.

## [0.47.1] - 2026-11-22

- Fixed: **Pausing protection now covers the whole cluster.** When you temporarily turn protection off
  (the troubleshooting pause), it used to pause only the one node you were on, so a device that uses
  more than one cluster node as its DNS would, after a short while, reach a node that never paused and
  see blocking come back before the timer ran out. The pause deadline is now mirrored to every node, so
  protection stays off everywhere until it expires. It is still runtime-only, and a full restart clears
  it. Arming a pause now also flushes the DNS cache, so a name that was being answered from a stale or
  cached empty entry resolves fresh at once instead of staying stuck until that entry expires.
- Fixed: **Large services no longer stall when a response-IP list is active.** YouTube and other big
  sites rotate across many CDN addresses, and a response-IP list that happened to flag one of them
  would refuse that answer and stall the service. A built-in allowlist now always exempts the
  published IPv4 ranges of the large eyeball networks (Google/YouTube, Cloudflare, Fastly) from
  response-IP blocking. It is always on, needs no setup and is not shown in the list UI, and it only
  has an effect while a response-IP list is active. A direct-IP threat is not in that space and stays
  blocked, and a malicious name fronted by such a CDN is still caught by the ordinary name block.
- Changed: **DNS chaff reaches higher and varies more.** The intensity slider now goes up to 600 decoy
  lookups per minute (was 120), and the decoy pool grew to about 150 varied hostnames so the cover
  traffic blends in better.

## [0.47.0] - 2026-11-22

- Added: **Block answers that resolve to a known-bad IP.** A new kind of list holds IPv4 addresses and
  CIDR ranges instead of domain names and is checked against the addresses a resolver returns: when an
  answer resolves to an address on the list, the whole answer is refused with the usual block response,
  so a name whose address is malicious is stopped even when the name itself is on no list. The
  ready-made "JustDNS Threat-IPs" list (C2, dropper and malware addresses from our honeypot network) is
  one click away in the catalogue, and you can add your own IP lists by URL like any other feed. IPv4
  only for now.

## [0.46.3] - 2026-11-21

- Changed: **DACH blocklist moved to its new repository.** The JustDNS DACH supplement is now fetched
  from `justdns-blocklist` (previously `justdns-dach-blocklist`). The list contents are unchanged, and
  saved subscriptions keep working because they store the preset key rather than the URL.

## [0.46.2] - 2026-11-21

- Fixed: **Settings no longer switch themselves off across a cluster.** Protections such as the newly
  registered domain (NRD) block, the lookalike and homoglyph URL check and the privacy module could
  turn off on their own, and the chosen primary (main node) could clear itself. In a cluster the
  configuration is edited only on the primary, but the sync still merged config both ways by last
  write, so a node that had not yet caught up could overwrite a setting cluster-wide just by saving
  something else. The primary is now authoritative: every other node mirrors the primary's
  configuration and never pushes its own over it. With no primary designated a cluster still syncs both
  ways as before, so designating a primary is what makes a setting stick.

## [0.46.1] - 2026-11-21

- Fixed: **The designated primary (main node) no longer clears itself.** In a cluster the chosen
  primary is the only node that may edit configuration; every other node is a read-only follower. A
  sync race could wipe that designation: a node that had not yet learned the primary would, on any
  unrelated settings save, stamp its own empty designation with a newer time and win the merge, so the
  whole cluster fell back to no primary and every node turned writable again. The designation is now
  ranked by when it was explicitly set, not by the last settings write, so an empty one can no longer
  overwrite a chosen primary. Clearing the primary on purpose still applies across the cluster.

## [0.46.0] - 2026-11-21

- Added: **iOS profile for encrypted DNS.** Settings, Encryption now offers a download for a
  configuration profile (`.mobileconfig`) that points iOS and iPadOS at this node's DNS-over-TLS
  endpoint. iOS has no "Private DNS" switch like Android; install the profile via Settings, General,
  VPN & Device Management. With a publicly trusted certificate no CA install is needed.
- Fixed: **The DoT/DoH toggles and switching the certificate now take effect instantly.** Previously
  every save in this area kicked off the full policy rebuild (millions of domains re-materialized from
  the feeds) even though only a TLS listener goes up or down. The rebuild now runs only when a field
  the DNS engine actually reads changes.

## [0.45.0] - 2026-11-20

- New: **Privacy module.** An opt-in module that stays dormant until you switch it on, so a node that
  leaves it off needs no extra resources. It is off by default; enable it under Settings, Modules. It
  adds two things to the Privacy page. The **surveillance graph** shows who on your network talks to
  which tracker company, and how often, as a **heatmap** (device × company) and a **ranking**, and
  lets you cut a single device off from a company with one tap; the view stays legible even with many
  devices and companies. **DNS chaff** sends a steady trickle of plausible decoy lookups to the
  upstreams so your real query stream is harder to profile from the outside; a single slider sets the
  rate. The decoys never appear in the DNS query log, which keeps showing only what your devices
  really asked for; they have their own separate log under the module.
- The module's background profile now streams the history into the aggregation row by row instead of
  loading the whole scan into memory at once, so the module no longer OOMs a small node.

## [0.44.1] - 2026-11-20

- Fixed: **A beta update no longer stalls silently when a node runs an outdated updater sidecar.** The
  updater sidecar is a separate image from the app and is not refreshed by a one-click update, so a
  node could keep an older sidecar that does not understand pre-release (-rc) versions and rejects
  them as "invalid version" without reporting back. On a cluster that left the rolling update waiting
  on a follower that would never return. The sidecar now advertises what it supports in its heartbeat;
  the app refuses a beta update up front with a clear message to refresh the sidecar, and the beta
  channel shows a warning when the sidecar is too old. A follower raises the same error, so the
  primary's cluster update aborts in seconds instead of after the node timeout.

## [0.44.0] - 2026-11-20

- New: **Release channels (stable and beta).** Each node can follow the stable channel (vetted
  releases, the default) or the beta channel, which also pulls pre-release builds for testing. The
  channel is per node and is never synced across the cluster, so a single node can trial a build
  before it reaches everyone. Set it under System, Update.

## [0.43.2] - 2026-11-20

- Fixed: **Saving the auto-backup connection no longer OOM-kills the node.** Every settings save
  rebuilt the full blocking and phishing sets from the enabled feeds (millions of domains), even when
  the change had nothing to do with DNS, such as saving the backup target or recording a backup run.
  On a memory-capped node that rebuild could spike over the limit and restart the resolver. A save
  that touches only the backup target, the alert webhook or the metrics token now skips the rebuild
  entirely.

## [0.43.1] - 2026-11-19

- Fixed: **A scheduled backup no longer OOM-kills the node.** On a memory-capped node the backup
  built, sealed and uploaded the whole configuration while holding several full copies in memory at
  once, which could push a tight node over its limit and restart it. The backup now frees each copy
  as soon as it is done and streams the sealed file from disk to the share (SMB and NFS), so it stays
  within a small, steady footprint.
- Fixed: **An interrupted backup can no longer loop the node.** If a run was cut short before it
  finished, the next start treated the backup as still due and tried again at once. The run time is
  now recorded before the upload, so an interrupted backup waits for the next interval instead of
  retrying in a loop.
- New: **"Save connection" button.** Creating a backup connection now has an explicit save button,
  instead of only saving each field as you leave it.

## [0.43.0] - 2026-11-19

- New: **Scheduled backup to a network share.** A new card under "Sicherung" stores your setup as an
  encrypted file on a Samba or NFS share on a schedule, with no mount needed. Status and metrics sit
  up top (last backup, interval, retention), with "Back up now" and "Test connection" below; target
  and access tuck behind an expander. The backup uses the same format as the manual download and is
  encrypted with your backup password; older copies are pruned to the configured count. In a cluster
  the schedule runs on the primary node only. Both SMB (Samba) and NFS are supported; for NFS the
  "Share" is the export path and access is granted by the node's IP.
- Fixed: **A primary node stays set once chosen.** An unrelated settings change on another node could
  clear the cluster's primary assignment, so the high-availability page would offer "make primary"
  again. The assignment is now synced as its own entry and survives other nodes' settings changes.
- Fixed: **A daily report no longer crashes the node.** On memory-capped containers a large report
  could OOM-kill the node, because DuckDB sized itself to the whole host's memory rather than the
  container limit. DuckDB now gets a fitting memory limit and a place to spill, so large reports run
  through.

## [0.42.2] - 2026-11-18

- Fixed: **The log and devices pages open faster.** On instances with large block lists, opening
  "Protokoll" or the devices page could take several seconds, because building the device filter
  rebuilt every block list in memory first. It now reads only the list names it needs for the filter,
  so the pages load quickly again. Nothing on the pages changes.

## [0.42.1] - 2026-11-18

- Fixed: **The overview banner reflects a paused state.** While all blocking is paused for
  troubleshooting, the status line at the top now reads "Protection paused" in a danger colour
  instead of staying on the green "Protection active", so it no longer contradicts the pause card
  below it. The banner follows a pause started on another tab or node too.

## [0.42.0] - 2026-11-18

- New: **Pause all blocking for troubleshooting.** A new card on the overview lets an administrator
  switch every block off at once (block lists, phishing and NRD detection, CNAME uncloaking) for one
  minute, five minutes or a custom number of hours. While it runs, the card shows a ring countdown
  and a resume button, so a name a list would refuse resolves normally while you track down a false
  positive. The pause expires on its own and a restart clears it, so protection can never stay off
  by accident.
- Changed: **The high availability page shows the cluster as node cards.** The paired view now lays
  the nodes out as a responsive card grid with the cluster actions in a header toolbar and the main
  node marked with an accent, instead of a single tall list. Everything the view could do before is
  kept.

## [0.41.0] - 2026-11-18

- Changed: **The cluster update on the System page shows each node's progress.** While a rolling
  update runs, "System & Diagnose" now draws an overall progress bar with a percentage and a tile
  per node with its own ring, so you can see at a glance which node is updating, restarting or
  already back on the new version. A failed node no longer counts towards the overall percentage.
- New: **A reminder not to change settings during the rollout.** Starting a cluster update now opens
  a warning first, and a standing notice stays on the card while the update runs, since the nodes
  restart one after another and settings changed in between can be lost.

## [0.40.1] - 2026-11-18

- Fixed: **The NRD list switches on the phishing page respond instantly.** Turning an NRD list on or
  off now flips the switch at once and pulls the large list in the background, instead of leaving the
  control stuck until the download settled. The server also rebuilds the list off the request thread,
  so the page stays responsive.
- Changed: **The NRD list descriptions name their window and size.** Each of the four lists now
  states which 7-day window it covers and its approximate size, and the 7-day list is marked as the
  recommended starting choice.

## [0.40.0] - 2026-11-18

- Changed: **The phishing filter in the query log opens a themed menu.** Its choice list (any hit,
  warned only, blocked only) is now drawn to match the rest of the dark interface, with a coloured
  dot for each mode and a check on the active choice, in place of the browser's plain system
  dropdown. It still opens by click or arrow key and reads the same to a screen reader.
- Changed: **Dependencies brought up to their current releases.** Every Rust and web dependency
  moved to its latest version, including several major upgrades: argon2, totp-rs, rcgen, ureq,
  axum-extra, base64 and x509-parser on the Rust side; React, TypeScript, Vite and
  @vitejs/plugin-react on the web side. Nothing changes in how the app behaves; it builds and the
  full test suite passes on the new versions.

## [0.39.0] - 2026-11-17

- Changed: **The query log is redesigned as a focus stream.** Instead of the filter rail on the
  left, the search sits on top, with a quiet bar of source toggles and the phishing filter beneath
  it; the rarer filters (type, response code, resolver, device, country/network, time range) fold
  behind "More filters". Each row leads with the domain, a coloured source dot at the start and tags
  only where there is something to show. Everything the view could do before is kept.
- New: **Filter by phishing.** A new filter shows only entries where phishing detection fired,
  optionally warned-only or blocked-only, across the whole stored history. That is how you track
  down a lookalike domain on purpose, even once it has scrolled out of the live stream.
- Changed: **The group headings in Settings are clearer.** "General" and "DNS" now stand brighter,
  bolder and with a hairline rule above their rows, so the rail falls into its sections at a glance.

## [0.38.1] - 2026-11-17

- Fixed: **Switching settings category no longer shifts the page sideways.** The window now always
  reserves room for the vertical scrollbar, so a taller or shorter category view no longer makes the
  bar appear and disappear and jolt the whole layout left and right.

## [0.38.0] - 2026-11-17

- Changed: **The sidebar is grouped for easier scanning.** The menu is now split into four
  labelled sections (Overview, Protection, Rules, System), each a quiet heading above its pages,
  with every entry shown at once. The two long sections no longer fold in and out, so a page is
  always one click away and the whole list stays readable.
- Changed: **The Block/Allow submenu is tidier.** Its two sub-pages ("Your own lists by URL" and
  "Ready-made lists") now carry their own small icons and line up under a guide line, so the
  submenu reads as a clear branch of the list.
- Changed: **Settings are rebuilt for clarity.** The category rail is now grouped under General
  and DNS, with a search field that finds a category by name or by what it holds (typing "ipv6"
  reaches Network). Each setting is one line (its name and its control); the explanation moves
  behind the ⓘ next to the name. Expert mode is now a quiet switch at the foot of the rail instead
  of a wide band at the top.

## [0.37.3] - 2026-11-17

- Changed: **The phishing marker in the query log is clearer.** A hit now shows a small
  "Phishing" pill in the status column beside the source (amber when warned, red when blocked)
  instead of a wide "Warned" bar, so the row stays tidy. The kind of match and the imitated brand
  are in the row's expanded detail and its tooltip.
- Changed: **Clearer phishing alerts.** The webhook now reads "Phishing-Warnung" or "Phishing
  blockiert", names the device that asked, and says whether the answer was only marked or blocked.
- Fixed: **Phishing alerts no longer repeat for one open page.** Alerts are de-duplicated per device
  and registrable domain, so a page that re-resolves many sub-hosts of the same site raises one
  alert per hour instead of one per sub-host.
- Fixed: **Searching the query log now reads the full retention window.** A text search previously
  scanned only the live in-memory feed, so an entry could seem to vanish within minutes; it now
  reads the durable history (kept for the configured retention, e.g. 7 days), exactly like the
  sidebar filters.

## [0.37.2] - 2026-11-17

- Fixed: **A client dropping a TCP connection is no longer logged as an error.** When a client
  simply closed a DNS-over-TCP connection (reset by peer) while JustDNS was waiting for the next
  message, this produced a scary `WARN` about a connection ended on error. It is now treated as the
  clean close it is, so routine client disconnects stay quiet in the log.

## [0.37.1] - 2026-11-17

- Fixed: **Newly-registered-domain (NRD) lists now start working right after you switch them on.**
  Until now a freshly enabled NRD feed was only registered and did not download until the next
  background refresh up to twelve hours later, so for a while suspicious fresh domains were neither
  marked in the log nor reported. Enabling a feed (or preset) now fetches it immediately in the
  background, and the download timeout was raised so the larger windows have time to finish.

## [0.37.0] - 2026-11-17

- Added: **Phishing protection against lookalike and fresh domains.** Block lists only catch a name
  that is already on them, which barely helps against targeted phishing through freshly registered
  lookalike domains. JustDNS now also checks queries by **computation**: for confusable characters
  and Punycode against a list of protected brands (Tech & Mail, Banks AT and DE, plus your own) and,
  through a list subscription, for **newly registered domains** (NRD). Per category you choose
  between **warn only** (the answer is unchanged, the hit is just marked in the log and optionally
  reported over a webhook) and **block**. Detection runs fully locally; the NRD feeds (source
  HaGeZi, GPLv3) are clickable as individual time windows, from the smallest 7-day window up to 28
  days. A new *Phishing protection* menu entry sits under DNSSEC, and hits show up as a badge in the
  query log. Off by default, and changes take effect immediately with no restart. (An optional
  online check of the registration date over RDAP is prepared but not yet exposed in the dashboard.)

## [0.36.0] - 2026-11-15

- Added: **Uncloak CNAME-cloaked trackers.** Some trackers hide as a CNAME on a seemingly
  first-party subdomain (for example `metrics.shop.com` pointing at a tracker), so the queried
  name is on no block list and slips through. JustDNS now checks the CNAME targets in an answer
  against your block lists and blocks the query when a target is a known tracker, even if the
  queried name itself is allowed. On by default; a new switch under Filtering turns it off. It
  only inspects the records an answer already carries, so it adds no extra lookup.

## [0.35.0] - 2026-11-15

- Added: **Test a block list against your own history before you turn it on.** Each catalogue
  preset and every URL block feed now has a *Test* button that measures the list against your
  recorded query log over the last day or week, without activating anything. It shows how many
  queries the list would newly block, broken down by device and by domain, and flags any names
  you explicitly allow that it would catch, so you can see the effect and the collateral before
  committing to a list.

## [0.34.6] - 2026-11-15

- Changed: **The list catalogue now leads with the recommended picks, gold-outlined.**
  Recommended lists sort to the top of the catalogue (the JustDNS list first) and wear a gold
  border, so the sensible starting choices stand out at a glance. The JustDNS supplement is
  renamed from *JustDNS DACH-Ergänzung* to *JustDNS DACH*.

## [0.34.5] - 2026-11-15

- Fixed: **Typing in a dialog no longer loses focus after each keystroke.** Dialogs such as
  *Create user* and the certificate dialog reset focus on every character, so you could type only
  one letter before the cursor jumped out of the field. The dialog's focus handling now runs once
  when it opens instead of on every render, so a field stays focused while you type.

## [0.34.2] - 2026-11-15

- Security: **The cluster peer API now rate-limits wrong sync keys.** The peer
  snapshot — which carries every password hash and TOTP secret — was gated only by the shared
  sync key, with no limit on guessing it. Failed peer-auth attempts are now throttled (a valid
  key is always checked first, so real nodes are never blocked), so the key can no longer be
  brute-forced against the endpoint at line rate.

## [0.34.1] - 2026-11-15

- Fixed: **The rolling cluster update now reliably downloads the restore file.** The
  automatic backup download was cleaned up in the same instant it was triggered, which could
  cancel it before the browser had begun saving — so a whole-cluster update often left the admin
  without the restore file it promised. Cleanup is now deferred, and a genuinely failed download
  is shown plainly instead of silently swallowed — the update still runs, since each node writes
  its own on-disk safety backup, so a rollback stays protected.

## [0.34.0] - 2026-11-15

- Added: **A JustDNS DACH blocklist in the catalogue, with the "Recommended" badge.** The
  Block/Allow catalogue now offers a JustDNS-maintained supplement for German-language
  reach-measurement trackers (INFOnline/IVW, VG Wort METIS, ÖWA) that international lists cover
  less well — a small, hand-verified delta meant to run alongside *Hagezi Normal*, not replace
  it. Each domain is a pure measurement beacon with no page function, so false positives are
  near zero and it carries the green **Recommended** badge in the ads & trackers category. Its entries
  are fetched from the public
  [justdns-dach-blocklist](https://github.com/bonderaustriaAT/justdns-dach-blocklist) repository.

## [0.33.0] - 2026-11-15

- Added: **A "Recommended" badge in the list catalogue.** The Block/Allow catalogue now marks
  the sensible default picks with a green **Recommended** badge, so a new user can switch on a
  good starting list without weighing every entry against its false-positive risk. For now
  *Hagezi Normal* (ads and trackers) and *Hagezi Threat Intelligence* (security) carry it.

## [0.32.1] - 2026-11-14

- Changed: **The Privacy dashboard reaches the world map faster.** The "most-contacted
  companies" and "devices" lists now show the top five by default, each with a "show more"
  link to expand the rest, so the world map below them is in view without a long scroll.

## [0.32.0] - 2026-11-14

- Added: **A world map of where your devices phone home, and watched networks.** The Privacy
  dashboard's "where your devices phone home" section now opens with an offline world map,
  shading each country by how many answers resolved into it; click a country to filter the
  network list to it. You can mark a country or network (ASN) as **watched** — globally from
  the map, or per device and group in their policy — and JustDNS then flags every matching
  answer in the query log with an amber badge, and can post a webhook alert when a watched
  network is contacted (off by default). Watching is a read-path marker only: it highlights
  and alerts, and never blocks or touches the DNS hot path. The map is a bundled, offline SVG
  (Simple World Map by Al MacDonald and Fritz Lekschas, CC BY-SA 3.0) — no map library and no
  network calls.

## [0.31.0] - 2026-11-14

- Changed: **A smoother, safer cluster update.** Updating the whole cluster is now easier to
  reach and harder to get wrong. On the main node, the update button under **System &
  diagnostics** reads **Update cluster** and starts the rolling update right there (a follower
  points you at the main node instead); a quick check runs first, so you are not asked for a
  backup password when there is nothing new to install. The backup password is entered twice
  to catch a typo in a password that cannot be recovered, and a glitch where that field lost
  focus after a single keystroke is fixed.

## [0.30.0] - 2026-11-14

- Added: **See the country and network behind every answer (GeoIP).** JustDNS now maps each
  address a name resolves to onto its **country** and **network (ASN)**, so you can tell at a
  glance that a device just resolved a name into a network in another country. The query log
  shows a flag and the network operator next to the resolved addresses, the live feed can be
  narrowed to one country or ASN, and the Privacy page gains a **Where your devices phone
  home** section that groups the whole network's destinations by country and network. It is
  **offline only** — a local database file, never an online lookup — and ships ready to use
  with the DB-IP Lite databases; point it at your own MaxMind GeoLite2 files to swap it, or
  leave it off entirely. Nothing about the DNS service or the log changes; the lookup happens
  as the dashboard is read. See [the guide](docs/guides/geoip.md).

## [0.29.1] - 2026-11-14

- Changed: **The read-only notice on a follower node is now impossible to miss.** On a node
  managed by a main node, the banner that points you at the main node runs as a full-width
  yellow strip across the very top of every page — above the sidebar and content, with a lock
  icon and a link to the main node — instead of a quiet line inside the page.

## [0.29.0] - 2026-11-14

- Added: **Manage the whole cluster from one node.** You can now pick a **main node**
  (primary) and do all your managing there — every other node becomes a read-only
  follower that applies what the main node syncs. A follower's dashboard still opens (so
  you can log in and look around to debug), shows a banner pointing at the main node, and
  refuses changes. From the main node's High-availability page you can also set each
  node's own settings that never sync — its display name, DoT/DoH hostname, certificate
  choice and block-page address — and trigger a one-click update on any node, without ever
  logging into it. You can also **update the whole cluster in order** from the main node:
  each follower is updated one at a time and only moved past once it is back and serving the
  new version, then the main node updates itself last — and if a node does not come back in
  time the run stops with the main node left untouched. If the main node is ever gone, any
  node can take over as main from its own High-availability page. Off by default: with no
  main node chosen, every node stays writable exactly as before. See
  [the guide](docs/guides/high-availability.md).

## [0.28.0] - 2026-11-14

- Added: **Browser-hardening security headers.** Every dashboard response now carries a
  strict Content-Security-Policy plus anti-clickjacking (`X-Frame-Options`,
  `frame-ancestors`), no-sniff, a `no-referrer` policy and a locked-down permissions
  policy. The browser will only run scripts and load resources that came from JustDNS
  itself, and refuses to render the dashboard inside another site's frame. On by default,
  nothing to configure. (No HSTS yet — that waits for serving the dashboard over HTTPS by
  default.) See [the guide](docs/guides/browser-hardening.md).

## [0.27.0] - 2026-11-14

- Added: **Block a company for a single device from the privacy dashboard.** Open a device
  (tap it in the by-device list) and each company row now carries a **For this device**
  button (for Editors and Admins). It blocks that company — every domain the catalogue
  knows for it — for that one device only, by adding it to the device's own policy, so the
  rest of the network is unaffected. Like the network-wide block it is idempotent and the
  result says how many entries were new. The company view's **Block** button still blocks
  network-wide; now you can choose the scope.

## [0.26.1] - 2026-11-14

- Fixed: **Company and device names are fully readable on the privacy dashboard.** Long
  names were cut off even when there was room for them. The company list and the company
  view now give the name column the space it needs, and long device names wrap to a second
  line instead of being clipped.

## [0.26.0] - 2026-11-14

- Added: **Block a company straight from the privacy dashboard.** Clicking a company now
  shows a **Block** button (for Editors and Admins). *Block company* adds every domain the
  catalogue knows for that company to your block list — not only the ones seen so far — so
  it is blocked everywhere from the next query on; each domain row also has its own block
  button for a single name. Blocking is idempotent (a re-block adds nothing, and the result
  says how many entries were new), and the entries land in your own block list tagged with
  the company name, so you can review or undo them on the Block & allow lists page. This
  turns the dashboard's "who does this device talk to" into one-click action.

## [0.25.1] - 2026-11-14

- Changed: **Privacy dashboard polish.** Clicking a company now opens its own view
  showing **which devices** reached it and **which exact domains** under it were looked
  up (for example `graph.facebook.com`, `connect.facebook.net`), each with its blocked
  share. The "Got through" figure is now red to read as the number to watch, the
  per-device cards are easier to read against the card behind them, and the one-line
  headline sentence was removed.

## [0.25.0] - 2026-11-12

- Added: **Privacy dashboard.** A new **Privacy** page (in the sidebar, next to the
  overview and the query log) attributes your devices' DNS lookups to the **companies**
  behind the names — so instead of a list of domains you see, in plain language, that a
  phone reached Google 240× today or that a TV keeps talking to an analytics firm. It
  shows network-wide totals (tracker contacts, how many were blocked, how many got
  through), the most-contacted companies with a per-category colour and a blocked-share
  bar, and a per-device breakdown you can tap into to see exactly which companies one
  device talks to. Pick a 24-hour, 7-day or 30-day window. The company catalogue is
  curated and compiled into JustDNS — it covers the large advertising, analytics, social
  and fingerprinting networks, fetches no feed and phones nothing home. Readable by any
  signed-in user; node-local, like the other stats views.

## [0.24.1] - 2026-11-11

- Added: **Prometheus metrics token in the dashboard.** The `GET /metrics` scrape
  endpoint no longer needs an environment variable to turn on. Under _Settings →
  Network_ there is now a **Metrics token (Prometheus)** field: generate a token with
  one click, copy it into your Prometheus scrape config, and the endpoint switches on;
  clear the field to turn it off. The token is stored with the rest of your settings,
  encrypted at rest, and blanked for non-admin readers. The old `JUSTDNS_METRICS_TOKEN`
  environment variable still works and applies as a fallback whenever no token is set in
  the dashboard, so existing installs are unaffected.
- Changed: **Tidier backup password field.** The encryption-password inputs on the
  backup download now sit in a labelled, width-constrained group with a small lock icon
  and shorter helper text, instead of stretching across the whole row.

## [0.24.0] - 2026-11-10

- Added: **Encryption at rest.** The files that hold JustDNS's secrets — the account
  list (password hashes, TOTP secrets, recovery codes) and the secret parts of your
  settings (notification webhook or bot token) — are now **encrypted on disk**, so a
  copied data volume, a backup archive or a VM snapshot is useless without the key. It is
  fully automatic: there is nothing to switch on, and an existing install upgrades itself
  on first start (it reads the old plaintext files and rewrites them sealed). The key is a
  random value generated on first boot and kept in its **own volume** (`/keys`), separate
  from the data so a stolen data volume cannot also decrypt it; the official `docker
  compose` install adds this volume automatically. Each file is sealed with
  ChaCha20-Poly1305 under a per-file key derived (HKDF-SHA256) from the master key, and
  loading is fail-closed — a missing or wrong key stops the node with a clear error rather
  than silently starting with no accounts. This protects data at rest, not a fully
  compromised running host (the deliberate price of needing no passphrase at boot). New
  guide: _Encryption at rest_ (DE + EN). **Back up the key volume separately — losing it
  makes the sealed files unreadable.**
- Added: **Password-encrypted backups.** A configuration backup is no longer a plaintext
  file. You now set an **encryption password** (typed twice) when you download one, and
  the file is sealed with it — Argon2id to stretch the password, ChaCha20-Poly1305 to
  encrypt — so a backup that still holds settings like resolver addresses or a notification
  webhook URL is never readable in the clear. The same password protects the safety backup
  the one-click **update** writes before it runs, so the on-disk pre-update copy is sealed
  too. Restore asks for the password and reports a wrong one plainly; older unencrypted
  backups still restore (leave the password empty). **Keep the password — there is no
  recovery, and without it a backup cannot be restored.**

## [0.23.0] - 2026-11-08

- Added: **Notifications & alerts.** JustDNS can now reach out to you over a **webhook**
  when something in the cluster needs attention, instead of waiting for you to spot it in
  the dashboard. Pick a format — generic/ntfy, Slack, Discord or Telegram — and switch on
  any of four events: a **node going down** (and coming back), a new **version available**,
  a **certificate nearing expiry**, and a **device blocked an unusual amount** within a
  window (a classic malware-beacon sign; off by default, with a tunable threshold and
  window). Each node sends its own alerts, prefixed with its instance name; the
  cluster-identical "update available" alert is sent by just one node so you don't get it
  twice. Configured as a new **Settings → Notifications** section, with a **Send test**
  button that posts a one-off message so you can confirm the channel. The webhook URL and
  Telegram token are stored like the rest of the configuration and masked in the UI. New
  guide: _Notifications & alerts_ (DE + EN).

## [0.22.2] - 2026-11-08

- Fixed: **The "Query log" switch now actually works.** The toggle under Settings was
  shown and stored but never read — queries were always recorded. It is now a real switch
  on the recording: turn it off and this node writes no new rows, so the live log, the
  query explorer and the dashboard statistics stop gaining entries (what is already stored
  stays until retention drops it); turn it back on and recording resumes on the next query,
  no restart.
- Changed: **The query log says whose queries it shows.** A short note under the title
  makes it clear that the **live** view is only this node's own traffic, while **filtering**
  reads the full history — which in a cluster spans every node, each row badged with the
  node that answered it. This clears up "why isn't my device's blocked query here?" when
  the device uses a different node for DNS. The guide gained a matching section.

## [0.22.1] - 2026-11-08

- Fixed: **A cluster no longer switches the block page off by itself.** The block page's
  on/off flag (and its text, mode and address) used to ride inside the single settings
  record the nodes sync, so changing *any* unrelated setting on one node could carry an
  older "block page off" state onto the other — the listeners would come up at boot and
  then go dark a few seconds later, after the first sync. The block page now syncs as its
  own record with its own timestamp, so it only changes across the cluster when you
  actually change the block page. Each node still keeps its own server IP.

## [0.22.0] - 2026-11-08

- Added: **"Ask to unblock this page" on the block page.** When someone lands on a
  blocked site, the block page now offers a button to request access — with an optional
  reason — instead of just stopping them. Each request lands in a new **System → Access
  requests** queue (admin only, with a count badge) showing which device asked, when,
  the site, and *what* blocked it (the matching list or category). Release the site for
  the one device, its whole group, or everyone (a permanent global allow, confirmed
  first), or dismiss the request. An allow wins over a block, so a released site resolves
  normally again on the next query. In a cluster the queue is shared, so you release once
  on any node. New guide: _Access requests_ (DE + EN).
- Fixed: **A blocked domain with no usable block-page IP no longer answers `0.0.0.0`.**
  That answer made browsers show `ERR_ADDRESS_INVALID`; when the block page is on but its
  server IP is blank or invalid, the A query now returns NODATA (no address) instead,
  matching the AAAA behaviour.
- Changed: **The block page's server IPs fill themselves in.** When you turn the block
  page on without a server IPv4/IPv6, JustDNS now derives them from this node's own
  cluster address, so a blocked name points at the right node without hand-entering an
  IP (a wrong value — such as a pasted URL — was the usual cause of `ERR_ADDRESS_INVALID`).

## [0.21.7] - 2026-11-08

- Fixed: **Blocked HTTPS domains reach the block page again behind a reverse proxy.**
  In reverse-proxy mode the node had stopped listening on port 443, so opening a blocked
  site over `https://` ended in a bare "connection refused" instead of the redirect —
  because blocked domains point straight at the node and bypass the proxy. The node now
  always serves both port 80 and port 443 while the block page is on; the reverse-proxy
  switch only controls whether the `X-Forwarded-Host` header is trusted. The unavoidable
  certificate warning on a blocked `https://` domain is unchanged (it appears in every
  mode); the proxy still gives the landing page itself a clean certificate.

## [0.21.6] - 2026-11-09

- Changed: **Reverse proxy is now the recommended block-page mode**, and its guide is
  clearer. The settings mode list shows _Behind your own reverse proxy_ first and marked
  recommended. The guide uses a dedicated subdomain (`block.justfuture.at`), spells out that
  the path doesn't matter (so the proxy just forwards the whole host — no `/blockpage`
  routing needed), warns against reusing the admin-UI host, and adds a "reuse an existing
  host" path using an NGINX Proxy Manager custom location.
- Added: **A test locks in that the block-page settings sync across a cluster.** The
  block-page address, mode, enable flag, title and message are shared across HA nodes; only
  the per-node server IP stays local (each node points blocked names at itself). This was
  already the behaviour; the test guards it from here on.

## [0.21.5] - 2026-11-09

- Changed: **The log verbosity switch reacts instantly, and the log shows it's working.**
  Picking Normal / Debug / Trace now highlights the chosen level at once instead of waiting
  for the next status poll, and while the engine applies it the diagnostic log shows a small
  spinner and a thin progress bar so you can see something is happening. If the change fails,
  the switch returns to the active level with the error.
- Changed: **Clearer reverse-proxy block-page help.** The reverse-proxy mode is only a TLS
  front: the JustDNS node still serves the page, and the proxy forwards to that same node and
  provides the certificate. The settings help and the block-page guide now say this plainly —
  blocked names keep pointing at the node (Server IPv4), and the proxy sits only in front of
  the landing hostname.

## [0.21.4] - 2026-11-09

- Added: **A new block-page mode — _Behind your own reverse proxy_.** If you already run
  NGINX Proxy Manager, Traefik, or another reverse proxy, it can terminate HTTPS and
  forward to JustDNS over HTTP; JustDNS no longer manages a certificate of its own. Turn
  it on, enter the public block-page address and this box's IP, and the settings page shows
  exactly what to configure on the proxy (including the one `X-Forwarded-Host` header NGINX
  Proxy Manager needs). The block-page guide has a step-by-step NPM walkthrough.
- Fixed: **The hosted block page could redirect in a loop over HTTP/2.** An HTTP/2 request
  carries no `Host` header — the hostname is in the `:authority` field — so the server read
  an empty host, never matched the landing host, and redirected endlessly. It now reads the
  authority (and, behind a trusted reverse proxy, `X-Forwarded-Host`), so the landing page
  is served instead of looping.

## [0.21.3] - 2026-11-09

- Fixed: **A blocked page could show "this site can't be reached" instead of the block
  page.** When the block page had no IPv6 address set, a blocked name answered its AAAA
  (IPv6) query with the all-zeros address `::`. Dual-stack browsers prefer IPv6 and
  rejected that outright (`ERR_ADDRESS_INVALID`) rather than falling back to the IPv4
  block page — so the page stayed invisible even with a correct IPv4. A blocked AAAA with
  no configured block-page IPv6 is now left empty (NODATA), so the browser uses the IPv4
  block page. Tip: the block page's **Server IPv4** must be the address of the JustDNS box
  itself (where ports 80/443 are published), not another host.

## [0.21.2] - 2026-11-09

- Fixed: **Uploaded certificates were lost on every update.** The certificate store
  defaulted to a path outside the `/data` volume, so anything beyond the built-in
  self-signed certificate vanished when the container was recreated. The store now lives
  at `/data/certs` and persists across updates and restarts. After updating, re-add any
  certificates once — they stay from then on.

## [0.21.1] - 2026-11-09

- Fixed: **The v0.21.0 image failed to build** — a stray quote in the German UI strings
  broke the type-check. This release builds and publishes cleanly.
- Changed: **Block-page ports are now published automatically on update.** The updater
  sidecar makes sure ports 80/443 are mapped — via a small, clearly-marked Compose
  override — so the block page is reachable after an update with nothing to change on the
  host. An install still running the pre-0.21.1 updater needs that sidecar refreshed once;
  see the block-page guide.

## [0.21.0] - 2026-11-09

- Changed: **The block page is much simpler to set up — and it actually loads.** The
  settings now start from one **mode** switch: *Host on your own domain (recommended)* or
  *Show on this box directly*. In hosted mode JustDNS redirects a blocked domain to your
  own hostname — e.g. `https://justdns.justfuture.at/blockpage` — with the blocked domain
  appended as `?d=`, and serves the page there with a certificate valid for your name, so
  it loads over HTTPS **with no warning** and can show which domain was blocked. Inline
  hints spell out the one-time rewrite and certificate steps.
- Fixed: **"Connection refused" when the block page was on.** The official Docker Compose
  now publishes ports **80** and **443**, so a blocked domain actually reaches the page
  instead of a refused connection. On your own host, add the two port lines once and
  recreate the service, or run the resolver with `network_mode: host`.

## [0.20.0] - 2026-11-09

- New: **Certificates now sync across a high-availability cluster.** The whole
  certificate store — including private keys — travels between your nodes over the
  authenticated sync, so a certificate you add, upload or renew on one node is present
  on all of them and a store id names the same certificate everywhere. The block page's
  certificate selection is shared too; the DoT/DoH selections and the block-page server
  IP stay node-local (each node serves its own hostname and points blocked names at
  itself). Keys never leave the cluster.

## [0.19.1] - 2026-11-09

- New: **The block page can redirect instead of serving its own page.** Set a
  **Redirect URL** under **Settings → Block page** and JustDNS answers blocked requests
  with an HTTP 302 to it — so you can host the explanation at a real hostname with a
  valid certificate (via a DNS rewrite onto JustDNS, or behind a reverse proxy) and not
  manage a certificate here at all.
- Changed: **A new look for the block page.** The served page is cleaner — a calm dark
  layout with a pulsing status dot, the blocked host as a mono chip, and a "protected by
  JustDNS" line.
- Fixed: **Long sidebar entries are no longer cut off.** Names like "System & diagnostics"
  and "High availability" now wrap instead of being truncated with an ellipsis.

## [0.19.0] - 2026-11-08

- New: **A custom block page.** Instead of a blocked name failing silently, JustDNS can
  now serve a page that explains *this site is blocked* — over HTTP and, with a stored
  certificate, over HTTPS. Turn it on under **Settings → Block page**, set the server IP
  blocked names should point at, choose a certificate and write your own heading and
  text. (Over HTTPS a certificate warning for the blocked domain is unavoidable without
  breaking the connection open — JustDNS never does that; over plain HTTP the page shows
  with no warning. The block page uses the host's ports 80 and 443, which your Compose
  must publish.)
- New: **A central certificate store.** Certificates now live in one place under
  **System → Certificates**: give each a name, **generate** a self-signed one or
  **upload** your own, then select it for DoT, DoH *and* the block page from a dropdown —
  no repeat upload. A **Renew** button replaces a certificate in place after checking the
  replacement is currently valid, so every feature that uses it keeps working. Your
  existing certificate is carried over automatically as **Standard**.
- Changed: **DoT and DoH now pick a certificate from the store** instead of a
  self-signed/uploaded switch of their own. Both start out on **Standard**, so nothing
  changes until you choose otherwise.
- Changed: **The sidebar is tidier.** Navigation is grouped into collapsible **Rules**
  and **System** sections, so the list stays short; the group holding your current page
  unfolds on its own.

## [0.18.0] - 2026-11-08

- New: **The System page shows what the DNS engine is actually doing.** Operational
  events that used to be invisible now reach the diagnostic log: an upstream resolver
  going unhealthy and recovering (once per change, not per query), each feed refresh
  with its entry count and download time, a **warning** when a feed suddenly loses most
  of its entries (a likely-broken source), a DNSSEC validation failure, and when
  settings or a schedule window change takes effect.
- New: **A verbosity switch on the System page.** Raise the live log level to *Debug*
  or *Trace* to watch a problem happen — no restart — then drop back to *Normal*. It is
  deliberately temporary: a restart returns to the normal level, so a forgotten debug
  session cannot keep filling the disk.
- New: **Filter the diagnostic log by category** (DNS & upstreams, Lists, Web & HA, …),
  so you can watch one subsystem without the rest scrolling past.

## [0.17.0] - 2026-11-08

- New: **Removing a cluster node is clean and sticks.** Removing a peer (the × on its
  card) or leaving the cluster now asks first, then tells the other nodes, so the
  removed node is dropped across the whole mesh instead of lingering as an unreachable
  "ghost" that every node keeps trying to reach. Under the hood a removal is a roster
  tombstone that travels with the sync, so another node's copy can no longer resurrect
  it; a node you remove by mistake can always rejoin by connecting again.
- Fixed: **The app updates to the new version instead of showing a stale one.** The web
  app — above all when installed to a phone's home screen — could keep showing a
  months-old version because the app shell and its service worker were cached
  indefinitely. They are now always revalidated, while the content-hashed assets stay
  cached, so a new release is picked up on the next visit. After updating to this version
  once, reload the app on the phone (or remove and re-add it) to clear the old cache;
  later updates then appear on their own.

## [0.16.1] - 2026-11-07

- Fixed: **No more "could not persist HA state" warning on installs without a data
  directory set.** High availability was the only store that defaulted its file to
  `/data` instead of the working directory, so an install that does not set
  `JUSTDNS_DATA_DIR` (a local or non-Docker run) logged a "No such file or directory"
  warning on every save. It now defaults to the working directory like the other stores;
  the Docker image sets `JUSTDNS_DATA_DIR=/data` explicitly, so a standard deployment is
  unaffected.

## [0.16.0] - 2026-11-07

- New: **Scheduled reports.** A new *Reports* page (admin only) turns the query log into
  readable daily or weekly summaries — total queries, block rate, average response time,
  the split by source, DNSSEC verdicts, and the top domains, blocked domains and devices.
  Both cadences are off by default and configurable (hour, plus weekday for the weekly
  one), in the timezone from Settings. Numbers are cluster-wide, like the query-log
  explorer. Reports are stored on the data volume, viewed in-app, generated on demand with
  *Generate now*, and downloaded as a printable HTML document or CSV — nothing is sent
  anywhere.
- New: **The query log shows which server answered, by hostname.** In a high-availability
  cluster, each row's node badge now reads as the serving node's hostname (the host part
  of its address) instead of a cryptic node-id prefix; your own node's rows are dimmed and
  marked as this node. On a single-node instance the badge stays hidden.
- Changed: **The Users page makes the two-factor requirement clearer.** A *Require
  two-factor for all accounts* banner now sits at the top of the page for the global
  requirement, and each account's own requirement moved into a dedicated *Required*
  column; while the global requirement is on, the per-account switch shows *Enforced*.

## [0.15.1] - 2026-11-07

- New: **The query-log explorer can filter by device or group.** A new *Device / group*
  filter in the explorer narrows the log to a single named device or a whole group. A
  device defined as an address range (CIDR) matches every observed address inside it,
  following the same "most specific device wins" resolution as the log's device column.
- Fixed: **The history view now shows the date next to the time.** When a filter is set
  and the explorer reads the full stored history, entries span several days, so the Time
  column now carries the date as well; the live tail stays time-only.

## [0.15.0] - 2026-11-07

- New: **High availability now syncs the query log and statistics across the cluster
  (Stage C).** Each node pulls the other nodes' query-log entries, so the *Query log*
  explorer on any node shows the whole cluster's traffic over the retention window, and
  the lifetime statistics no longer reset to zero after you fail over to another node.
  Each log row carries a small node badge showing which node it came from. Only the
  explorer and the running totals union — the live dashboard (recent queries, today's
  activity, per-device and per-resolver views) stays node-local.
- New: **A guided "Add another node" panel for the third and fourth node.** The cluster
  list now walks you through adding a node — turn HA on, set the same shared key, enter
  one seed address — with a live node count and a note when the cluster is full at four.
- New: **You can reveal the shared sync key to copy it to a new node.** An admin can now
  show and copy the key from the HA page (a key pinned by an environment variable is
  never shown and stays managed outside the app). The reveal is recorded in the audit log.
- Changed: **The query-log retention setting is now enforced.** *Settings → Query log →
  retention* (seven days by default) previously had no effect — the log grew without
  bound. It now actually prunes older entries on every node, and the same window bounds
  what high availability syncs. If you kept more than the configured number of days of
  history, raise the retention before updating to keep it.
- Fixed: **A peer's version is now shown correctly after it updates.** The cluster list
  froze each peer's version at the moment it was paired, so a node that updated without
  re-pairing kept showing its old version. The version now refreshes every sync cycle.

## [0.14.0] - 2026-11-07

- New: **High availability now spans up to four nodes, as a self-assembling mesh.**
  Instead of pairing two nodes, you set each node's own address and join a new node to
  any one existing node; it then discovers the rest from a gossiped roster and meshes
  with every node automatically. Every node syncs directly with every other, so one
  node going down never interrupts the sync among the rest, and a returning node catches
  up on its own. The HA page becomes a cluster node list — each node with its health,
  address, version and last sync — with per-node remove and a "leave cluster" action.
- Fixed: **A high-availability sync can no longer switch off two-factor authentication.**
  When nodes first began syncing users, an account's whole record was resolved by
  newest-wins — so a node whose copy carried a later timestamp but had no MFA could strip
  MFA from every node. Confirmed MFA is now reconciled on its own: if either node has it
  for an account, both keep it and converge on "MFA on". Re-enroll MFA once on the node
  that should have it and it propagates and stays. Removing MFA is now deliberate per
  node — clear it on each node (see the HA guide).

## [0.13.1] - 2026-11-07

- Fixed: **A node's name no longer leaks across a high-availability pair.** The
  instance display name and the DoT/DoH certificate hostname were part of the synced
  settings, so a change on one node overwrote the other and both ended up with the same
  name. Both are now node-local and never cross the sync. Rename each node once after
  updating; from then on the names stay put.

## [0.13.0] - 2026-11-07

- New: **High availability now syncs users and logins too (Stage B).** In an HA pair
  every account — its role, password and second factor (TOTP secret and recovery
  codes) — travels to both nodes, so the same logins and the same enforced MFA apply
  everywhere. On a name clash the winner you pick at the first join decides; after that
  the newer change wins. Login sessions stay node-local on purpose (you sign in on each
  node separately).
- Security: **The sync key is now as sensitive as the password database.** The snapshot
  a node pulls now carries password hashes and TOTP secrets, so the wizard requires a
  key of at least 16 characters, and the guide recommends pairing only over TLS and
  resetting every password if the key may have been exposed.

## [0.12.1] - 2026-11-07

- Fixed: **High-availability pairing could not be established.** The node-to-node
  handshake (`/api/ha/hello`) was gated behind an already-existing pairing, so the
  very first connect — when neither node has a partner yet — always failed with a
  404. The handshake now answers as soon as high availability is enabled and the
  shared key is set (the key is the real guard), so two fresh nodes can pair.
- Changed: **The pairing step now shows this node's own address to copy.** On
  *Connect the partner* you see this instance's address with a copy button, so you
  only swap two URLs across — copy each node's address into the other's partner field.

## [0.12.0] - 2026-11-07

- Added: **High availability — keep two servers in sync (Stage A).** Run a second
  JustDNS that stays in sync with the first, so either one can answer and you can
  take one down to update or reboot without the network noticing. The two nodes are
  equal — both writable, changes flow both ways and are merged (newer change wins) —
  so there is no fixed primary. A new admin page, *High availability*, walks you
  through it: turn it on, set a shared key, and connect the partner by its address.
  The first time two separately-set-up nodes connect, you make one choice — keep this
  node or the partner for any genuine conflict — and everything unique to either side
  is always kept. This stage syncs your settings, lists, rewrites, devices, groups and
  schedules; users and statistics follow in later stages. It is **off by default** and
  does nothing until you enable and pair it. JustDNS syncs and shows the partner's
  health but does not switch traffic over for you — point your clients at both nodes
  (two resolver IPs, or an external virtual IP). See the new
  [High availability guide](docs/guides/high-availability.md).

## [0.11.0] - 2026-11-07

- Added: **Filter and search the whole query history.** The *Protokoll* page gains a
  filter rail: narrow the log by source (blocked, cache, upstream, local, rewrite),
  record type, response code, resolver and a from/to time range, and page back through
  the **entire stored history** with "Load older" — not just the last hundred entries
  held in memory. The live tail works exactly as before when no filter is set; the
  moment you set one, live updates pause and the view reads straight from the durable
  store on disk. The search box still matches the queried name and the client address
  (and, in the live view, the device name).

## [0.10.3] - 2026-11-07

- Changed: **Old images are cleaned up automatically after an update.** Each update now
  keeps exactly two images on the host — the version you just installed and the one
  before it — and removes anything older on its own. A one-step rollback stays
  available while disk use no longer creeps up, so the manual "delete the old image?"
  prompt after an update is gone. Nothing to decide, nothing to click.

## [0.10.2] - 2026-11-07

- Added: **Public container images on GitHub Container Registry (ghcr.io).** A GitHub
  Actions workflow now builds and publishes the JustDNS image (and the updater
  sidecar) to `ghcr.io` on every release, and the official Compose install pulls from
  there by default — so it works out of the box anywhere, with no registry login,
  matching the public GitHub tags the in-app update already checks. Self-hosters can
  still point `JUSTDNS_IMAGE` at their own registry.

## [0.10.1] - 2026-11-07

- Changed: **The update dialog shows the instance's real data path.** The backup-path
  hint now names the actual data directory the instance uses, instead of a fixed
  `/data`, so the "must be persistent" note is accurate on installs that mount their
  data elsewhere.

## [0.10.0] - 2026-11-07

- Added: **In-app updates.** JustDNS now checks GitHub for new releases on its own
  and shows one on the **System & diagnostics** page — with the changelog and an
  **Update now** button — plus a quiet banner for admins. One click takes a
  configuration backup (to a pre-filled, persistent path on the data volume, and a
  restore file you can download), pulls the new version and restarts the container;
  afterwards it asks whether to delete the old image. No SSH or `docker` commands.
  The update source is public GitHub tags, so it works out of the box with no token.
  One-click updates are driven by a small, isolated **updater sidecar** in the new
  official `deploy/docker-compose.yml`; the DNS process never gets the Docker socket.
  An install without the sidecar shows a ready-to-paste command instead. Update
  checks are on by default and can be turned off; the whole feature is admin-only and
  audited. See the [update guide](docs/guides/updates.md).

## [0.9.1] - 2026-11-06

- Changed: **Blocked services now show real brand logos.** Each service card
  carries the actual brand mark (Facebook, TikTok, YouTube, WhatsApp, …) instead
  of a coloured initial, so the catalogue reads at a glance. The logos are bundled
  into the app — no external icon service is contacted at runtime. A service name
  now always sits on one line, with its switch on its own row. (Disney+ keeps a
  lettered tile, as its mark is not available in the icon set used.)

## [0.9.0] - 2026-11-06

- Added: **Blocked services.** A new **Blocked services** page (under Rules) offers a
  curated catalogue of well-known products — social networks, video and music
  services, messengers, game stores — each a single switch that blocks the whole
  service by bundling the domains it relies on, instead of hunting those domains down
  by hand. The switch on that page sets the **global default** for the whole network;
  in the device, group and [schedule](docs/guides/schedules.md) editors each service
  gets the same three-state **inherit / on / off** control as a block list, so you can
  block TikTok everywhere but allow it on your own phone, or block a service for one
  group only during set hours. An **allow always wins** over a blocked service, and the
  [query log](docs/guides/query-log.md) names the service as the reason for a block.
  Services are **off by default**. See the
  [Blocked services guide](docs/guides/blocked-services.md).

## [0.8.0] - 2026-11-06

- Added: **Prometheus metrics endpoint.** JustDNS can now expose its operational
  figures — query counts, cache hits, block decisions, DNSSEC verdicts, cache and
  list sizes, listener state, active devices and query latency — at `GET /metrics`
  in the Prometheus text format, so you can scrape them into an existing monitoring
  and alerting stack. The endpoint is served on the same web port as the dashboard,
  so no extra port needs opening. It is **off by default** and switches on only when
  you set a bearer token (`JUSTDNS_METRICS_TOKEN`): without one it answers 404, and
  with one a scraper must present the token or it gets 401. A scrape reads only the
  in-memory figures the dashboard already keeps, so it never touches the query
  database. See the [Prometheus metrics guide](docs/guides/metrics.md).

## [0.7.1] - 2026-11-06

- Fixed: **The System page's version tile now shows the running release.** It was
  reporting the internal build version (`0.1.0`) instead of the actual release
  (for example `v0.7.0`), because the container image did not pass the version
  through to the server process. The dashboard footer was already correct; the
  diagnostics tile now matches it.

## [0.7.0] - 2026-11-06

- Added: **System & diagnostics page.** A new **System** page (admin only, under
  System) gives a technician the "is everything running, and if not, where is it
  stuck?" view without reaching for `docker logs`. At the top, **runtime status**
  tiles: the running version and uptime, which DNS listeners are up (UDP, TCP, DoT,
  DoH), the last block/allow-list refresh (flagged red if it failed), the response-
  cache size, the loaded list sizes and the number of active devices. Below that, a
  live **diagnostic log** of the server's own operational events — listener and
  certificate trouble, feed errors, and the like — that you can filter by severity
  (all / warnings+ / errors only), search, and **download as a .txt**. Any error in
  view raises a red banner and is highlighted in the stream.
- Added: **Diagnostic logs now persist.** The server's operational log was only ever
  written to standard output, so it vanished on every restart. It is now also kept
  in memory (for the new page) and written to a **rotating file on disk** (under
  `/data/logs` in the container), keeping the **last 7 days** by default. The
  retention is configurable (`JUSTDNS_SYSLOG_RETENTION`), and the file can be turned
  off entirely (`JUSTDNS_SYSLOG=off`) while the in-dashboard log stays. This is the
  **system/diagnostic** log (how the server itself is doing); the per-query log of
  what your devices looked up is unchanged.
- Added: **Health endpoint.** An unauthenticated `GET /api/health` reports that
  JustDNS is serving (status and uptime only — no version or internals at the open
  endpoint), and the container now carries a Docker **HEALTHCHECK** that uses it, so
  an orchestrator or uptime monitor can tell a healthy instance from a stuck one.
  See the [System & diagnostics guide](docs/guides/system-logs.md).

## [0.6.1] - 2026-11-06

- Fixed: **DNSSEC no longer rejects legitimate domains.** With validation switched
  on, some perfectly good sites — unsigned domains and large content networks among
  them — were wrongly judged **Bogus** and refused with SERVFAIL, so they stopped
  loading. JustDNS now trusts the validating upstream resolver's verdict (asking for
  it with the DNSSEC "DO" signal and confirming a real failure with a "CD" retry)
  instead of re-checking the signature chain itself over a forwarder, which could not
  be done reliably. An unsigned domain is now correctly reported **Insecure** and
  resolves normally, while a genuinely tampered or badly signed answer is still
  refused. See the [DNSSEC guide](docs/guides/dnssec.md).

## [0.6.0] - 2026-11-06

- Added: **DNSSEC validation.** JustDNS can now check the cryptographic signatures
  on DNS answers itself and **refuse a tampered or badly signed reply** with a
  SERVFAIL instead of handing back a forged address. A new **DNSSEC** page (under
  Monitoring) shows the verdict for recent lookups — **Secure**, **Insecure**,
  **Bogus** or **Indeterminate** — with four counter cards you can click to filter
  the list, a red alert banner whenever a tampered answer has been refused, and
  bogus rows highlighted so they stand out. Validation is off by default and is
  switched on in the server configuration (`JUSTDNS_DNSSEC`). See the
  [DNSSEC guide](docs/guides/dnssec.md).

## [0.5.2] - 2026-11-06

- Changed: **a clearer two-factor column on the Users page.** The "active" status
  and the "required" control used to sit on top of each other and read as one lump.
  Now the status is a quiet badge and "required" is a proper on/off switch with room
  to breathe — in the users table and in the add-user dialog alike.

## [0.5.1] - 2026-11-06

- Changed: **more breathing room on wide screens.** The admin pages used too narrow
  a column, so the Overview, Users and Audit log felt cramped with wasted space
  beside them. The content area is now wider, and the Users and Audit-log tables use
  the full width so their columns no longer squeeze together — while the Account
  forms stay at a comfortable reading width.

## [0.5.0] - 2026-11-06

- Changed: **clearer navigation and a tidier admin.** The top menu is now a
  grouped **sidebar** — Monitoring, Rules and System — so user management and the
  audit log have their own visible place instead of hiding behind your name. On a
  phone the sidebar slides in from a menu button, while the most-used pages stay one
  tap away on the bottom bar. New accounts are created through a dialog from a
  single **+ Add user** button, so the Users page stays a clean account list, and
  the Account page's change-password form is laid out in one tidy column.

## [0.4.0] - 2026-11-06

- New: **users, roles & two-factor sign-in.** JustDNS is no longer a single
  admin. You can now add more accounts and give each one a **role**:
  *Administrator* (full control), *Editor* (change filtering content — lists,
  rewrites, devices, groups, schedules) or *Viewer* (read-only). An account from
  before this change stays a full administrator. Sign-ins can be protected with
  **two-factor authentication**: scan a QR code into an authenticator app, keep
  the ten one-time recovery codes, and enter a 6-digit code after your password
  from then on. An administrator can **require** two-factor for everyone (in
  Settings) or for individual people (on the new **Users** page), and reset a
  password or a lost second factor. A new **Audit log** records the recent
  sign-ins and account and settings changes — never any secret, just who did what
  and when. Repeated wrong passwords or codes are rate-limited, and the last
  administrator can never be locked out. See the
  [guide](docs/guides/users-roles-mfa.md). Accounts and two-factor are kept out of
  configuration backups by design.

## [0.3.0] - 2026-11-06

- New: **schedules.** A new **Schedules** page lets you filter by the clock. A
  schedule is a recurring weekly window — pick the weekdays and a start/end time —
  that, while it is open, either **pauses** its target completely ("internet off")
  or switches on extra lists and blocks or allows a few domains. Point it at
  everyone, at a device group, or at a single device. Windows may run past midnight
  (22:00–07:00 covers the night), and they are read in a **time zone you choose**
  under Settings, so "bedtime" stays local across daylight-saving changes. A pause
  still lets your always-allow list through, so a school portal or an emergency
  number stays reachable. Disable a schedule for the holidays with one toggle,
  without deleting it. See the [guide](docs/guides/schedules.md). Schedules are
  included in your backup.

## [0.2.5] - 2026-11-06

- Improved: **a tidier overview.** The overview is now laid out as clear cards —
  a headline card for the all-time totals, the live rate with its 24-hour chart,
  a row of last-24-hours figures, the busiest devices and the resolver response
  times side by side, and the recent lookups below. Every figure from before is
  still there; the all-time and last-24-hours numbers are now labelled, so they
  no longer look alike.

## [0.2.4] - 2026-11-06

- Maintenance: **security update to the upstream DNS library.** JustDNS uses the
  hickory-dns library to talk to the resolvers it forwards to, including over DoT
  and DoH. It has been updated to close three reported security issues: one where
  a failed DNSSEC validation could be hidden instead of reported, one where
  unrelated records in a reply could be followed, and one where a crafted reply
  could pin the CPU while a response was being assembled. Nothing changes in how
  you use JustDNS.

## [0.2.3] - 2026-11-06

- New: **resolver response times.** The **Overview** now shows how each upstream
  resolver you forward to is performing — listed by name, busiest first, with the
  **median** answer time (hover for the 95th percentile) and how many answers it
  gave over the last 24 hours. Only forwarded lookups count, so you can tell at a
  glance which resolver is actually answering and whether one is consistently
  slower than the others. The figures survive a restart like the rest of the
  statistics.

## [0.2.2] - 2026-11-06

- New: **top domains per device.** On the **Devices** page, every row now has a
  **Top domains** button that opens the most-looked-up domains of the last 24
  hours for that one device, busiest first and with counts — so you can see at a
  glance what a single device talks to most. The figures survive a restart like
  the rest of the statistics.

## [0.2.1] - 2026-11-05

- Changed: **the encryption settings are now in one place.** "Encrypted client
  DNS" and upstream encryption have moved into a single **Encryption** category
  (under Expert mode), split into the two directions: your devices reaching
  JustDNS (DoT/DoH) and JustDNS reaching the upstream resolvers.

- Improved: **the encrypted client DNS page now guides you.** Set the hostname
  first — DoT and DoH can't be switched on without one (the field is highlighted
  instead). The certificate download/upload and the copy-ready endpoint
  addresses appear only once an endpoint is actually running, so the page stays
  focused.

- Fixed: **easier-to-read settings text.** Descriptions no longer look cramped.

- Maintenance: security updates to the base image (Debian) and a web dependency
  (react-router).

## [0.2.0] - 2026-11-05

- New: **encrypted client DNS.** JustDNS can now be reached by your own devices
  over an encrypted connection, not just in the clear on port 53. Turn on
  **DNS-over-TLS (DoT)** on port 853 — what Android calls "Private DNS" — and/or
  **DNS-over-HTTPS (DoH)** at `/dns-query`, under **Settings → Encrypted client
  DNS**. Your per-device and per-group rules apply to encrypted clients exactly as
  before.

- New: **certificate handling, your choice.** By default JustDNS mints its own
  certificate; download the small CA from the settings page and install it once on
  each device. Or upload your own certificate and key (e.g. from an internal CA or
  ACME). The page shows the certificate's SHA-256 fingerprint and copy-ready
  endpoint addresses for setting up a phone or a browser. The certificate lives on
  the data volume, so it survives updates and restarts; a step-by-step guide
  (including installing the CA on Windows, macOS, iOS, Android and Linux) is in the
  handbook.

## [0.1.32] - 2026-11-05

- Changed: **the overview's response time now shows the median only.** The large
  figure under "Last 24 hours" is the typical lookup time (the median), which on a
  cache-heavy setup is reassuringly low — the average, which a single slow upstream
  could inflate, and the extra percentiles are no longer shown, so the number is
  unambiguous at a glance.

## [0.1.31] - 2026-11-05

- Fixed: **editing a group no longer opens a blank page.** Opening the editor of a
  freshly created group (one with no rules yet) could leave the page empty. It now
  opens correctly every time.

- Changed: **the Groups section now sits at the top of the Devices page**, and the
  list of devices is **collapsed by default** behind a labelled toggle with a count.
  Groups are reachable without first scrolling past every device, and you expand the
  device list only when you need it.

- Improved: **the per-device and per-group list controls explain themselves.** The
  three-way switch next to each block list now carries a short explanation of
  **Inherit / On / Off** (in particular that "Off" keeps a list off for this scope
  even when it is on above). When only the two built-in objects are available, a hint
  points you to where more lists come from — enable a block list under the curated
  lists, or add your own by URL, and it appears here to assign per device or group.

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
