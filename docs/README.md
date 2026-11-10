# JustDNS Handbook

Step-by-step guides for everything JustDNS can do, in plain language. A German
version of this page is kept alongside it in [README.de.md](README.de.md).

Each guide explains **what a feature is**, **how to use it click by click**, and —
where a feature is more technical — adds a clearly marked **"For the technically
curious"** section with the mechanism and the edge cases, pitched at someone
comfortable with networking and DNS.

New to JustDNS? Start with [First-run setup](guides/setup.md), then
[The overview](guides/overview.md).

## Guides

| Guide | What it covers |
| --- | --- |
| [First-run setup](guides/setup.md) | The setup wizard, your admin account, logging in |
| [The overview](guides/overview.md) | The live dashboard: counters, graph, top clients |
| [The query log](guides/query-log.md) | What was asked, by which device, how it was answered |
| [DNSSEC validation](guides/dnssec.md) | Check answer signatures and refuse tampered replies |
| [Block & allow lists](guides/lists.md) | Block ads/malware, subscribe to feeds, allow or block single domains |
| [Blocked services](guides/blocked-services.md) | Block whole services (TikTok, YouTube, …) with one switch, globally or per scope |
| [Block page](guides/block-page.md) | Show a blocked device an explaining page over HTTP and HTTPS, instead of an empty error |
| [Access requests](guides/access-requests.md) | Let a blocked device ask for a site to be unblocked, and release it per device, group or everyone |
| [Devices](guides/devices.md) | Give your network's clients friendly names |
| [Per-device & per-group policies](guides/policies.md) | Give a device or a group its own filtering rules |
| [Schedules](guides/schedules.md) | Pause or tighten filtering on a recurring weekly timetable |
| [DNS rewrites & address objects](guides/rewrites.md) | Answer your own names from fixed addresses |
| [Encrypted client DNS (DoT & DoH)](guides/encrypted-dns.md) | Let your devices reach JustDNS over DNS-over-TLS and DNS-over-HTTPS |
| [Certificates](guides/certificates.md) | The named TLS certificate store that DoT, DoH and the block page draw from |
| [Settings](guides/settings.md) | Resolvers, speed, block response, Safe Search, logging and more |
| [Users, roles & two-factor](guides/users-roles-mfa.md) | Add accounts, set what each may change, and protect sign-ins with MFA |
| [System & diagnostics](guides/system-logs.md) | Runtime status and the server's live diagnostic log, with a health endpoint |
| [Prometheus metrics](guides/metrics.md) | Expose operational figures at `/metrics` for an external monitoring stack |
| [Scheduled reports](guides/reports.md) | Daily or weekly summaries of DNS activity, viewed in-app and downloaded as HTML or CSV |
| [Notifications & alerts](guides/notifications.md) | Webhook alerts for a node going down, an update, an expiring certificate or a device spike |
| [Backup & restore](guides/backup.md) | Save your whole configuration to a file and bring it back |
| [Updating JustDNS](guides/updates.md) | Check GitHub and update in one click, with an automatic backup |
| [High availability](guides/high-availability.md) | Run a second node that stays in sync, so either one can answer |
| [Encryption at rest](guides/secrets-at-rest.md) | Your accounts and secret settings are encrypted on disk, automatically — a stolen volume or backup is useless without the key |

## How filtering fits together

JustDNS answers every DNS question your network asks. For each question it walks a
short, fixed pipeline:

1. **Local rewrites** — if you defined your own answer for a name (see
   [rewrites](guides/rewrites.md)), it is returned straight away.
2. **Safe Search** — search engines are steered to their filtered endpoints.
3. **Filtering** — the name is checked against the policy that applies to the
   client that asked. An **allow** always wins over a **block**. Which lists and
   rules apply depends on the client (see
   [policies](guides/policies.md)); by default every client gets the
   **global default policy** you set under [Settings](guides/settings.md) and
   [Lists](guides/lists.md).
4. **Cache & upstream** — anything not answered above is looked up at your
   resolvers and the answer is cached for next time.

The cache only ever holds real upstream answers, which are the same for everyone,
so per-device filtering costs nothing in speed.
