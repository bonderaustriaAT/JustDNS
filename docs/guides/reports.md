# Scheduled reports

*A German version of this guide is in [reports.de.md](reports.de.md).*

The **Reports** page turns the raw [query log](query-log.md) into regular, readable
summaries of what your network's DNS looked like over a day or a week: how many lookups
there were, how many were blocked, which domains and devices were busiest, and how fast
answers came back. It is the page you open to answer "what did last week look like?"
without scrolling through thousands of log rows — and the one you hand to someone who
just wants the headline numbers.

Reports are **never sent anywhere**. They are generated on the server, stored on the
data volume, viewed here, and downloaded as a file when you want one. There is no email,
no webhook, and no account to connect.

## What a report contains

Every report covers one window — the previous full **day** or the previous full
**week** — and shows:

- **Headline numbers** — total queries, how many were blocked and the **block rate**,
  and the average response time.
- **By source** — the split across cache, upstream, local, rewrite and blocked answers.
- **Top domains**, **top blocked domains** and **top devices**, each a ranked list. The
  device list shows the friendly [device name](devices.md) where one matched when the
  report was made.
- **DNSSEC verdicts** — the counts of secure, unsigned, bogus and indeterminate answers,
  shown only when [validation](dnssec.md) recorded any.

The numbers are **cluster-wide**: in a [high-availability](high-availability.md) set they
cover every node's traffic over the window, exactly like the query-log explorer — not
just the node you happen to be looking at.

## Turning on the schedule

Both cadences are **off by default**. Under **Schedule** you can switch on:

- **Daily report** — pick the hour it runs at. It covers the previous full day.
- **Weekly report** — pick the weekday and the hour. It covers the previous full week.

The clock used is the time zone from [Settings](settings.md), so "06:00" means six in the
morning your local time, across daylight saving. A report for a window is generated once;
leaving the app closed does not miss it, as long as the server is running when the hour
comes around.

## Generating one now

You do not have to wait for the schedule. **Generate now** produces a report straight
away for the last full day or the last full week — useful the first time you open the
page, or when you want this week's numbers before the weekly run. The new report opens
immediately and joins the list below.

## Viewing and downloading

The **Generated reports** list shows every stored report, newest first, with its window
and headline figures. Click **View** to open the formatted report with its numbers and
the three top-lists. From there:

- **HTML (printable)** downloads a self-contained page you can open in any browser and
  print — including "print to PDF" if you want a PDF to file or forward.
- **CSV** downloads the top-lists as a spreadsheet-friendly file.

Older reports are pruned automatically, so the list stays a useful recent history rather
than growing without bound.

---

## For the technically curious

A report is a **frozen aggregate**, not a live view. When it is generated, JustDNS runs a
handful of `GROUP BY` queries against the DuckDB query-log store over the half-open window
`[start, end)` — the same store and the same cluster-wide union the
[explorer](query-log.md) reads — and stores the resulting totals and top-lists as JSON on
the `/data` volume. Because it is frozen, a report keeps the device names as they were
when it ran, and reading it later costs nothing: there is no re-query.

**The scheduler** is a background thread, in the same family as the retention prune and
the HA sync loop. It wakes periodically, works out each cadence's due window in the
configured time zone (using the same `chrono`/`chrono-tz` machinery as the
[schedules](schedules.md) feature), and generates a window only if it is due and not
already stored. Dedupe is by the window's start, so a given day or week is written at most
once however often the thread wakes — and with both cadences off the thread does nothing.

**Storage is capped** per cadence (the most recent daily and weekly reports are kept, the
oldest fall off), written atomically by the same tmp-file-then-rename pattern the other
JSON stores on the data volume use. The cadence settings themselves live in the main
[settings](settings.md), so in a cluster they sync like any other setting — every node
agrees on the schedule, and any node can serve the stored reports it has generated.

**The download formats are deliberately simple.** HTML is a self-contained document with
inline styling, which prints cleanly and needs no server round-trip to render — "print to
PDF" covers the PDF case without pulling in a heavyweight PDF library. CSV carries the
three top-lists for anyone who wants to pivot the numbers in a spreadsheet.
