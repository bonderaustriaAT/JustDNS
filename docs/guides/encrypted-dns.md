# Encrypted client DNS (DoT & DoH)

*A German version of this guide is in [encrypted-dns.de.md](encrypted-dns.de.md).*

By default your devices talk to JustDNS **in the clear** on port 53: anyone on the
same network (or your internet provider, if the traffic leaves your house) can see
and tamper with the names they look up. **Encrypted client DNS** lets your own
devices reach JustDNS over an encrypted channel instead:

- **DNS-over-TLS (DoT)** on port **853** — this is what Android calls
  **“Private DNS”**.
- **DNS-over-HTTPS (DoH)** at **`/dns-query`** (port 8443 by default) — what
  browsers and many operating systems support.

This encrypts the link between your devices and JustDNS only. What JustDNS does
with a query — filtering, your per-device and per-group rules, caching, the
upstream lookup — is exactly the same as for a plain port-53 query. JustDNS
already encrypts the hop to its *upstream* resolvers; this adds encryption on the
*client* side too.

You find it under **Settings → Encrypted client DNS**. Both endpoints are **off**
until you turn them on.

## Setting it up

1. **Pick a hostname.** Enter the name your devices will use to reach JustDNS, for
   example `dns.fritz.box` or `dns.home`. This name goes into the certificate, so
   it must be the exact name the devices dial.
2. **Make the hostname resolve to JustDNS** on your network. The simplest way is a
   [DNS rewrite](rewrites.md) that answers that name with JustDNS's own IP address;
   some routers also let you add a local hostname directly.
3. **Choose the certificate** (see below), and for the self-signed default,
   **download the CA** and install it on each device.
4. **Turn on DoT and/or DoH.** The endpoints start immediately — no restart.
5. **Point your devices at the endpoint** shown under *Endpoints to set up*.

## The certificate

An encrypted connection needs a certificate the device trusts. You have two
choices:

- **Self-signed (the default).** JustDNS mints its own small certificate
  authority (CA) and a certificate for your hostname. Because no public authority
  signed it, each device must be told to trust the CA once — **download the CA
  certificate** from this page and install it (steps per platform below). Use
  **Regenerate certificate** if you change the hostname or simply want a fresh one
  (you then re-install the CA).
- **Upload my own.** If you already have a certificate — from an internal company
  CA, or a public one issued through ACME — paste the certificate chain and the
  private key (both PEM). JustDNS validates the pair before using it. Devices that
  already trust that issuer need no extra setup.

The **fingerprint (SHA-256)** shown on the page identifies the exact certificate
in use. Some clients let you pin or verify it; it is also how you confirm, on a
device, that you are talking to *your* JustDNS and not an impostor.

## Pointing a device at JustDNS

The page shows the two ready-made endpoint addresses — copy them with the **Copy**
button:

- **DoT** — `your-hostname:853`
- **DoH** — `https://your-hostname:8443/dns-query`

**Android (Private DNS):** Settings → Network & internet → Private DNS → *Private
DNS provider hostname* → enter your hostname. Android uses DoT on 853. With a
self-signed certificate you must install the CA first (below).

**A browser (Firefox, Chrome):** in the browser's secure-DNS / DoH setting, choose
a custom provider and paste the DoH URL.

**Whole-system DoH (Windows 11, macOS, iOS):** these accept a DoH URL in their
network settings or via a configuration profile. On a self-signed setup the system
must trust the CA first.

---

## For the technically curious

**What the endpoints are.** DoT is just DNS-over-TCP wrapped in TLS: JustDNS reuses
its length-framed TCP DNS handler over the decrypted stream. DoH is RFC 8484 — a
single `GET`/`POST /dns-query` endpoint speaking `application/dns-message`, served
over HTTP/1.1 or HTTP/2. Both terminate TLS inside JustDNS itself; there is no
reverse proxy, so the **client IP seen by the policy engine is the real device
address** and per-device/per-group rules apply unchanged. DoH responses carry a
`Cache-Control: max-age` equal to the smallest record TTL in the answer.

**Ports.** DoT listens on `0.0.0.0:853` and DoH on `0.0.0.0:8443` by default
(override with `JUSTDNS_DOT_LISTEN` / `JUSTDNS_DOH_LISTEN`). The container image
exposes both; your deployment maps them (`-p 853:853/tcp -p 8443:8443/tcp`). 443
is often taken by other services, which is why DoH defaults to 8443; put it behind
your own 443 only if nothing else there needs it.

**Where the certificate lives.** The certificate and key live in
`JUSTDNS_TLS_DIR` (`/data/tls` in the image): `cert.pem` (leaf + CA chain),
`key.pem` (the private key, mode `0600`), and for the self-signed case `ca.pem` /
`ca-key.pem`. Because `/data` is the persistent volume, the certificate **survives
image updates and restarts** — the fingerprint stays the same across upgrades, so
you install the CA once. The key is **not** included in a
[configuration backup](backup.md) (a backup carries the on/off settings and the
hostname, not the server's private identity); after restoring onto a fresh box,
regenerate or re-upload the certificate.

**Installing the self-signed CA.**

- **Windows:** double-click `justdns-ca.pem` → *Install Certificate* → *Local
  Machine* → *Place all certificates in the following store* → **Trusted Root
  Certification Authorities**.
- **macOS:** open it in **Keychain Access** (System keychain), then set it to
  *Always Trust*.
- **iOS/iPadOS:** install the file as a profile (Settings → General → VPN & Device
  Management), then enable it under **General → About → Certificate Trust
  Settings**.
- **Android:** Settings → Security → *Encryption & credentials* → *Install a
  certificate* → **CA certificate**. Note that modern Android trusts user-installed
  CAs for the system resolver (Private DNS), but apps may not — this is fine for
  Private DNS.
- **Linux:** copy to `/usr/local/share/ca-certificates/justdns-ca.crt` and run
  `update-ca-certificates`.

**Verifying from the command line.**

```bash
# DoT
kdig +tls @your-hostname -p 853 example.com
# DoH (POST a wire-format query)
curl --http2 -H 'content-type: application/dns-message' \
  --data-binary @query.bin https://your-hostname:8443/dns-query | xxd
```

**Security notes.** The private key is written owner-only (`0600`). An uploaded
certificate/key pair is validated before it replaces the live one. Because JustDNS
terminates TLS, it never trusts an `X-Forwarded-For` header for the client IP — it
uses the real TLS peer address.
