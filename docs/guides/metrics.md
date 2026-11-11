# Prometheus metrics

*A German version of this guide is in [metrics.de.md](metrics.de.md).*

JustDNS can expose its operational figures — query counts, cache and list sizes,
DNSSEC verdicts, listener state and latency — at **`GET /metrics`** in the
[Prometheus](https://prometheus.io/) text format, so you can scrape them into an
existing monitoring and alerting stack. This is the machine-readable sibling of the
[System & diagnostics](system-logs.md) page.

The endpoint is served on the **web port** (the same `8080` the dashboard uses), so
no extra port needs opening.

## Turning it on

The endpoint is **off by default**. The simplest way to switch it on is in the
dashboard: open **Settings → Netzwerk**, find **Metrics token (Prometheus)**, press
**Generate token**, copy it, and it saves automatically. From then on `/metrics` is
served and the scraper must present that token. Clearing the field turns the endpoint
off again. The token is stored with the rest of your configuration, encrypted at rest.

With no token set, `/metrics` answers **404** (as if it did not exist). With one set,
a scraper must present it as `Authorization: Bearer <token>`, or it gets **401**. A
Prometheus scraper cannot hold a login session, which is why the endpoint uses a
token of its own rather than the dashboard's accounts.

### Setting it by environment variable instead

If you would rather not keep the token in the dashboard, set it as an environment
variable on the container:

```
JUSTDNS_METRICS_TOKEN=<a long random string>
```

Pick a long, random value (for example `openssl rand -hex 32`). A token set in the
dashboard takes precedence; the environment variable applies only while no dashboard
token is set, so existing installs keep working unchanged. Keep the token secret —
anyone with it can read your operational figures.

## Scraping it from Prometheus

```yaml
scrape_configs:
  - job_name: justdns
    metrics_path: /metrics
    authorization:
      type: Bearer
      credentials: "<the same token>"
    static_configs:
      - targets: ["your-justdns-host:8080"]
```

## What is exposed

All names carry a `justdns_` prefix.

| Metric | Type | Meaning |
| --- | --- | --- |
| `justdns_build_info{version}` | gauge | Always 1; the running version is the label. |
| `justdns_uptime_seconds` | gauge | Seconds since the server started serving. |
| `justdns_queries_total` | counter | DNS queries handled in total. |
| `justdns_cache_hits_total` | counter | Queries served from the response cache. |
| `justdns_queries_upstream_total` | counter | Queries forwarded to an upstream. |
| `justdns_queries_local_total` | counter | Queries answered locally (rewrites, suppressed records). |
| `justdns_queries_blocked_total` | counter | Queries refused by the blocking engine. |
| `justdns_responses_nxdomain_total` | counter | Answers returned as NXDOMAIN. |
| `justdns_responses_servfail_total` | counter | Answers returned as SERVFAIL. |
| `justdns_dnssec_verdicts_total{verdict}` | counter | DNSSEC verdicts by outcome (`secure`/`insecure`/`bogus`/`indeterminate`). |
| `justdns_cache_entries` | gauge | Entries currently in the response cache. |
| `justdns_active_clients` | gauge | Clients that queried within the active window. |
| `justdns_list_entries{list}` | gauge | Loaded list entries by origin (`ads`/`malware`/`allow`/`custom`). |
| `justdns_listener_up{listener}` | gauge | Whether a listener is serving, `1`/`0` (`udp`/`tcp`/`dot`/`doh`). |
| `justdns_feed_last_refresh_timestamp_seconds` | gauge | Unix time of the last list refresh. |
| `justdns_feed_last_refresh_success` | gauge | Whether the last list refresh succeeded, `1`/`0`. |
| `justdns_query_duration_avg_ms` | gauge | Mean query handling time over the last day. |
| `justdns_query_duration_ms{quantile}` | gauge | Median and 95th-percentile handling time over the last day. |
| `justdns_resolver_queries_total{resolver}` | gauge | Answers per upstream over the last day. |
| `justdns_resolver_duration_ms{resolver,quantile}` | gauge | Per-upstream median/p95 handling time over the last day. |

---

## For the technically curious

**Cheap to scrape.** Every figure comes from the same in-memory handles the
dashboard reads — the query store's running totals, the day's latency summary, the
cache and blocker handles, and the runtime-status handle — so a scrape never runs a
DuckDB query. The counters are process-lifetime totals, rehydrated from the log on
start, so they survive a restart rather than resetting to zero (keep that in mind
when using `rate()` — it handles counter resets, but there is no reset here on a
clean restart).

**Latency is a summary, not a histogram.** JustDNS keeps the day's mean, median and
p95 rather than raw buckets, so the duration metrics are plain gauges carrying those
figures (`quantile="0.5"` / `"0.95"`), not a Prometheus `histogram`. That means you
read the quantiles directly and do **not** apply `histogram_quantile()` to them. A
true bucketed histogram would need per-query accounting on the hot path and is a
possible later step.

**The token.** It is compared in constant time, so a wrong token cannot be guessed
from the response timing. The endpoint is the only place the version is exposed to an
unauthenticated-session caller, which is why it is gated — the open
[`/api/health`](system-logs.md) deliberately omits the version.
