# Custom block page

*A German version of this guide is in [block-page.de.md](block-page.de.md).*

By default, when JustDNS blocks a name it answers with the **null IP**
(`0.0.0.0` / `::`) or **NXDOMAIN** — the connection simply fails, often as a bare
*"connection refused"*, with no explanation. A **custom block page** replaces that
silent failure with a real web page that tells the user *this site is blocked*.

You find it under **Settings → Block page** (admin only). It is **off** by default.

## Two modes

When you turn the block page on, you pick a **mode**:

- **Behind your own reverse proxy (recommended).** If you already run a reverse proxy
  (NGINX Proxy Manager, Traefik, …), let it provide **only the certificate**: it
  terminates HTTPS and forwards the request to the **same JustDNS node**, which
  renders the page. You select **no** certificate in JustDNS, and your proxy renews it
  automatically (e.g. Let's Encrypt). The cleanest route to a warning-free HTTPS block
  page. See [Behind a reverse proxy](#behind-a-reverse-proxy).
- **Host on your own domain.** No proxy of your own? Then JustDNS serves the page
  itself at *your own* hostname — e.g. `https://block.justfuture.at` — with a
  certificate you upload into the [certificate store](certificates.md) that is **valid
  for that name**. A blocked domain is redirected there, so the visitor lands on a
  clean HTTPS page **with no warning** that can even name the domain that was blocked.
- **Show on this box directly.** The built-in page is rendered in place on the
  blocked domain's own address. Simplest to set up, but over HTTPS the browser
  shows a certificate warning first (see below) — there is no way around that when
  the page answers *as* a foreign domain.

Either way, nothing else about blocking changes: which names are blocked, your
per-device and per-group rules, and the logs all work exactly as before. Only the
*answer* for a blocked name changes, from a dead IP to one that serves (or
redirects to) the page.

## The server IP (both modes)

Both modes need **one** thing: the **server IPv4** — the address this JustDNS box
is reachable at on your LAN (e.g. `192.168.20.2`). Blocked `A` queries are answered
with it, so the browser connects to JustDNS instead of a dead address. **Leave it
empty and nothing is served** — blocked `A` queries fall back to the null IP and
you get "connection refused" again. Set the **server IPv6** too if you use IPv6;
otherwise `AAAA` blocks keep returning the null IP (`::`).

## Hosted mode, step by step

Say your domain is `justfuture.at` and you want the page at
`https://justdns.justfuture.at/blockpage`:

1. **Turn on _Show block page_** and choose mode **Host on your own domain**.
2. **Block-page address** → `https://justdns.justfuture.at/blockpage`.
3. **Server IPv4** → this box's LAN IP, e.g. `192.168.20.2`.
4. **Add a [DNS rewrite](rewrites.md)**: `justdns.justfuture.at → 192.168.20.2`, so
   that hostname resolves to this box.
5. **Upload a certificate valid for `justdns.justfuture.at`** into the
   [certificate store](certificates.md) (your Let's Encrypt or wildcard cert) and
   **select it** as the HTTPS certificate.
6. Publish ports **80** and **443** (see [Ports](#ports)).

Now a device that hits a blocked domain over HTTP is redirected to
`https://justdns.justfuture.at/blockpage?d=<the-blocked-domain>`; because the
browser is now asking for *your* hostname, the certificate matches and the page
loads cleanly, showing which domain was blocked.

## Local mode, step by step

1. **Turn on _Show block page_** and leave the mode on **Show on this box
   directly**.
2. **Server IPv4** → this box's LAN IP.
3. Optionally **choose an HTTPS certificate** and **write the heading and message**.
4. Publish ports **80** and **443**.

## Behind a reverse proxy

This is the **recommended** route if you already run a reverse proxy — **NGINX Proxy
Manager (NPM)**, Traefik, Caddy, plain nginx. The proxy provides **only the
certificate**; the block page is still served by **the JustDNS node itself**. The
proxy accepts the landing address over HTTPS, terminates TLS with *its* certificate,
and forwards the request to the **same node** over **HTTP**. The node reads the
forwarded **`X-Forwarded-Host`** header to see that the landing page is meant, and
renders it — with no redirect loop. The proxy is just the TLS front, not the
destination.

The flow in one line:

```
Browser ──HTTPS──▶ reverse proxy (holds the cert) ──HTTP + X-Forwarded-Host──▶ JustDNS node :80 ──▶ block page
```

### Set it in JustDNS

1. **Turn on _Show block page_** and choose mode **Behind your own reverse proxy**.
2. **Block-page address** → the public HTTPS address of the page. Use a **dedicated
   subdomain** just for the block page, e.g. `https://block.justfuture.at`.
3. **Server IPv4** → **this JustDNS node's** LAN IP (e.g. `192.168.20.5`). Blocked
   `A` queries point here, and the proxy forwards here.
4. Optionally adjust the **heading and message**.

You pick **no** certificate for the **landing page** here — the proxy handles it: it
terminates HTTPS for `block.justfuture.at` and forwards to the node over HTTP. The
node still binds **port 443 itself**, though, because blocked domains point straight
at it (`Server IPv4`) and bypass the proxy — open a blocked domain over `https://` and
you land on *this* node's `:443`. There it presents the store's default certificate;
the browser warns (we hold no certificate for a foreign domain) and then redirects to
the clean landing page. That warning on blocked `https://` domains is unavoidable in
**either** mode (see [The HTTPS certificate warning](#the-https-certificate-warning));
the proxy only removes it on the landing page itself.

> **The path doesn't matter.** The node serves the page on **any** path of the landing
> host, so `https://block.justfuture.at` is enough — a trailing `/blockpage` is not
> needed (though it does no harm). Your proxy therefore does **not** need to route a
> specific path; forwarding the whole host is enough.

> **Use a dedicated subdomain** (e.g. `block.justfuture.at`), **not** your admin UI's
> hostname: in the simple setup the proxy forwards the *whole* host to the block page,
> so sharing it with the admin UI would hide the admin UI behind it. (To reuse the
> same host anyway, see [Reuse an existing host](#reuse-an-existing-host).)

Blocked names therefore still resolve **straight to the JustDNS node** (Server IPv4).
The proxy only sits in front of the **landing hostname** and forwards to the same node.

### NGINX Proxy Manager, step by step

Add a **Proxy Host** in NPM that forwards the landing hostname to the JustDNS node:

- **Details → Domain Names:** `block.justfuture.at` (exactly the host from the
  address above).
- **Details → Scheme:** `http`, **Forward Hostname / IP:** **this JustDNS node's**
  LAN IP (e.g. `192.168.20.5`), **Forward Port:** the block page's HTTP port (default
  **80**; in a Docker setup, the node's published or container-internal port).
- **Details:** *Block Common Exploits* is fine to enable; *Websockets* is not needed.
- **SSL:** pick a valid certificate for `block.justfuture.at` (e.g. request one from
  Let's Encrypt directly in NPM), and enable **Force SSL** and **HTTP/2**.
- **Advanced → Custom Nginx Configuration:** pass the requested host through, so the
  node recognises the landing page:

  ```nginx
  proxy_set_header X-Forwarded-Host $host;
  ```

  (NPM sets `X-Forwarded-For`/`-Proto` itself; `X-Forwarded-Host` is the one header
  JustDNS relies on.)

Finally, the **landing hostname** (`block.justfuture.at`) must resolve **to the
proxy** (a public DNS record or a [JustDNS rewrite](rewrites.md) onto the proxy's IP)
so the proxy can terminate TLS — the actual page is served by the node behind it.
From then on every blocked request is sent to `https://block.justfuture.at/?d=…`, the
proxy serves clean HTTPS, and the JustDNS node renders the page.

**Quick test:** `http://<node-ip>/` (directly, with any `Host` header) must show the
block page, and `https://block.justfuture.at/` through the proxy as well — clean
certificate, no warning.

#### Reuse an existing host

If you'd rather **not** add a dedicated subdomain and want to use an existing host
(say your admin UI's), route **only one path** to the block page in NPM: Proxy Host →
**Custom locations** tab → location `/blockpage` → forward to `http://<node-ip>:80`
(with the `X-Forwarded-Host $host;` header as above). The rest of the host stays on
its current target. Then set the **block-page address** to
`https://<your-host>/blockpage`, so the redirect hits exactly that path.

> Other proxies behave the same: set `X-Forwarded-Host` to the requested host and
> forward over HTTP to the JustDNS node. In **Traefik**, for instance, the `Host(...)`
> rule already carries the header; only exotic setups need it set explicitly.

## The HTTPS certificate warning

This is the honest, unavoidable catch — and the reason hosted mode exists. When a
device opens a **blocked HTTPS** domain directly — say `https://ads.example.com` —
the browser expects a certificate issued for `ads.example.com`. JustDNS can only
present the certificate you selected, which is **not** issued for that foreign
domain, so the browser shows a **certificate warning** before anything else — even
before a redirect can happen, because the TLS handshake comes first. There is no
way around this without breaking open the encrypted connection, which JustDNS does
**not** do.

In practice:

- Over plain **HTTP**, both modes are clean — the page shows, or the redirect fires,
  with no warning.
- Over **HTTPS to the blocked domain**, the warning appears first in *either* mode.
- **The win is the *landing* page:** once redirected, the visitor is on your own
  valid-certificate hostname, so that page is clean. In reverse-proxy mode the proxy
  provides that certificate; in hosted mode the certificate JustDNS selects does. In
  both, the node binds its own `:443` for the direct hits on blocked domains — only
  there is the warning unavoidable.
- Most blocked requests today are ad and tracking endpoints loaded *inside* a page;
  those just fail quietly, which is the point. The visible block page matters most
  when someone navigates to a blocked site directly.

## Ports

The block page uses the host's ports **80** (HTTP) and **443** (HTTPS). If they are
not reachable, every blocked domain ends in "connection refused".

**You normally don't have to do anything.** A fresh install from the official
`deploy/docker-compose.yml` already publishes them, and on an **in-app update** the
updater sidecar publishes them for you — it drops a small, clearly-marked
`docker-compose.override.yml` next to your Compose file so the ports are added
without touching your own Compose (Compose concatenates `ports`, so your other ports
stay). Delete that override and recreate if you ever want the ports gone, or if
something else on the host needs 80/443.

**One-time exception:** an install whose updater predates this release is still
running the old sidecar, which cannot do this yet. Refresh it once —

```bash
cd /opt/justdns && docker compose pull updater && docker compose up -d updater
```

— and from then on every update handles the ports on its own. Alternatively, run the
resolver with **`network_mode: host`**, where 80/443 are always reachable and no
mapping is needed; the updater then leaves your Compose untouched.

## The browser shows "this site can't be reached" instead of the block page

Almost always one of these two:

- **Wrong Server IPv4.** It must be the LAN address of **this JustDNS box** — the one
  where ports 80/443 are published. If it points at another host, a blocked domain lands
  on a machine with no block page. Check: the blocked domain must resolve to exactly that
  IP, and `http://<that-ip>/` must show the block page.
- **Chrome reports `ERR_ADDRESS_INVALID`** (before v0.21.3). With no Server IPv6 set,
  JustDNS answered the IPv6 query with the all-zeros address `::`, which browsers reject
  outright. From **v0.21.3** that answer is left empty and the browser uses the IPv4 page —
  just update.
- **The block page kept switching itself off in a cluster** (before v0.22.1). Its on/off
  flag travelled inside the one settings record the nodes sync, so an unrelated settings
  change on one node could carry an older "off" state onto the other — the listeners came
  up at boot and went dark seconds later. From **v0.22.1** the block page syncs as its own
  record with its own timestamp, so it only changes across the cluster when you change the
  block page. Just update.

---

## For the technically curious

**The DNS side.** When the block page is on, the pipeline's block branch answers a
blocked `A` with the configured IPv4 and a blocked `AAAA` with the configured IPv6,
instead of the null IP. An address family with no configured IP falls back to the
previous behaviour (null IP / NXDOMAIN), so an IPv4-only setup still does the right
thing for `AAAA`. The target is held in a shared slot the settings update live, so
toggling the page needs no restart.

**The web side — one server, two jobs.** Two listeners run only while the page is
enabled: plain HTTP on `JUSTDNS_BLOCKPAGE_HTTP_LISTEN` (`0.0.0.0:80`) and TLS on
`JUSTDNS_BLOCKPAGE_HTTPS_LISTEN` (`0.0.0.0:443`). For each request the handler looks
at the `Host` header:

- **Local mode** (no landing URL): every request renders one self-contained,
  dark-themed HTML page with status **451 (Unavailable For Legal Reasons)** and
  `Cache-Control: no-store`; the requested `Host`, your heading and your message are
  HTML-escaped into it.
- **Hosted mode** (a landing URL is set): a request whose host is **not** the
  landing host (a blocked domain pointed here) gets `302 Found` with
  `Location: <landing-url>?d=<blocked-host>`; a request whose host **is** the
  landing host is served the page itself (`200 OK`), reading `?d=` to name the
  blocked domain. Matching ignores port and case, and *any* path under the landing
  host serves the page, so there is no redirect loop.

The handler reads the requested host from the `Host` header first; if it is absent
(every **HTTP/2** request carries the host in the `:authority` pseudo-header
instead), from the request URI. In **reverse-proxy mode** (`behindReverseProxy`) it
comes from `X-Forwarded-Host` — but only then, since without that explicit trust a
client could spoof the header. The `behindReverseProxy` switch governs **only** this
trust in `X-Forwarded-Host` — **not** which ports bind: the server holds `:80` **and**
`:443` whenever the block page is on, in every mode. This is needed because blocked
domains point straight at the node via `server_ipv4` and bypass the proxy, so an
`https://` visit to a blocked domain lands on this node's `:443`. The proxy terminates
TLS only for the landing host.

A supervisor starts and stops the listeners on a settings change, and the HTTPS
config is rebuilt from the [certificate store](certificates.md) whenever you pick a
different certificate or renew the current one — an atomic `ArcSwap` swap, no
rebind.

**No on-the-fly CA.** The HTTPS listener presents exactly the certificate you
selected. JustDNS deliberately does **not** mint per-domain certificates or act as
a MITM, which is why the foreign-domain warning is unavoidable — and why hosting the
page on your own name (where the certificate *does* match) is the clean path.

**High availability.** The on/off flag, mode (including the reverse-proxy switch),
heading, message, landing URL **and the chosen certificate id** are **shared** across
a [cluster](high-availability.md) —
the certificate store itself is synced (see
[certificates](certificates.md#high-availability)), so the id names the same
certificate on every node. Only the **server IPs** stay **node-local**: each node
points blocked names at itself, so a sync never carries them between nodes (you can
still set them per node). With a shared landing URL, every node redirects to the
same clean page.
