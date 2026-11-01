# Changelog

What's new in JustDNS, in plain language. A German version of this file is kept
alongside it in [CHANGELOG.de.md](CHANGELOG.de.md). Dates are year-month-day.

## [Unreleased]

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
