# Certificates

*A German version of this guide is in [certificates.de.md](certificates.de.md).*

Several JustDNS features serve TLS: the encrypted client endpoints
[DoT and DoH](encrypted-dns.md) and the HTTPS side of the
[block page](block-page.md). Rather than each one carrying its own certificate,
JustDNS keeps a single **certificate store** — a named catalogue you manage in one
place and then **pick from** wherever a certificate is needed, with no repeat
upload.

You find it under **System → Certificates** (admin only).

## What a stored certificate is

Each entry has a **name** you choose, a **type**, and the facts read from the
certificate itself — its subject, the names it is valid for, its expiry, and a
SHA-256 fingerprint. The **Used by** column shows which features currently serve
it (DoT, DoH, or the block page), so you can see at a glance what a change would
affect.

Two types:

- **Self-signed** — JustDNS minted it itself, together with a local certificate
  authority (CA). Download that CA and install it on each device that should trust
  the connection (steps per platform are in the
  [encrypted-DNS guide](encrypted-dns.md)).
- **Uploaded** — a certificate chain and private key you supplied, for example
  from an internal company CA or one issued through ACME. Devices that already
  trust that issuer need no extra setup.

A fresh install starts with a single self-signed certificate named **Standard**,
migrated from the pre-store era; DoT and DoH point at it until you choose
otherwise.

## Adding a certificate

In **Add certificate**, give it a name, then either:

- **Generate** — mint a new self-signed certificate. Leave the hostname empty to
  use your encrypted-DNS hostname, or set a specific one. JustDNS creates the CA
  and the certificate for you.
- **Upload** — paste a certificate chain and its private key (both PEM). The pair
  is validated before it is stored; a mismatch is rejected with a clear message.

The new certificate appears in the catalogue immediately and becomes selectable in
every feature's certificate dropdown.

## Renewing a certificate

A certificate is replaced **in place** with **Renew**: upload a fresh chain and
key, and if they are currently valid they take over the same entry — same name,
same id, so **every feature that already selected it keeps working** without you
touching their settings. The replacement is checked first: an expired or
not-yet-valid upload is refused and the old certificate stays. Renew is how you
rotate an uploaded certificate before it expires, or swap a self-signed one for a
real one later.

## Deleting a certificate

**Delete** removes a certificate from the store for good. A certificate that a
feature still selects **cannot be deleted** — reassign DoT, DoH and the block page
to another certificate first, then delete. The store always keeps at least one
certificate.

## Downloading the CA

For a self-signed certificate, **Download CA** gives you the local CA in PEM form.
Install it as a trusted authority on every device that should connect without a
warning. An uploaded certificate has no JustDNS CA, so the button does nothing for
it — those devices trust it through the issuer you uploaded.

---

## For the technically curious

**Where it lives.** The store is a directory, `JUSTDNS_CERTS_DIR`
(`/data/certs` in the image), with one sub-directory per certificate named by its
stable id. Each holds `cert.pem` (leaf + chain), `key.pem` (the private key, mode
`0600`), and `meta.json` (the name, the type, and when it was created); a
self-signed entry also keeps `ca.pem` / `ca-key.pem`. Because `/data` is the
persistent volume, the store **survives image updates and restarts**, so a
fingerprint stays the same across upgrades and the CA is installed once.

**Migration.** On first start after the upgrade, any single-certificate material
in the legacy `JUSTDNS_TLS_DIR` (`/data/tls`) is imported as the entry **Standard**
(a stable id of `standard`), and DoT/DoH keep serving it. Nothing is lost and no
re-install of the CA is needed.

**Validity checks.** Expiry, subject and SANs are parsed from the DER with
`x509-parser`. A renewal refuses a certificate whose `notBefore` is in the future
or whose `notAfter` is in the past, so a live feature is never swapped onto a
certificate that cannot complete a handshake.

**Hot-swap.** Each feature reads its TLS configuration from a shared slot
(`ArcSwap<ServerConfig>`) per connection. Picking a different certificate — or
renewing one in place — rebuilds that config and swaps it atomically; **no listener
is rebound and no connection is dropped.**

**Selection and the wire.** A feature stores the chosen **id**, not the material.
DoT and DoH keep theirs in the encrypted-DNS settings (`dotCertId` / `dohCertId`);
the block page keeps `certId`. An empty / absent selection means "the store
default", so a feature always has a certificate even if the one it named was
deleted.

**High availability.** The **certificate store is shared across a
[cluster](high-availability.md)**: a certificate you add, upload or renew on one
node is carried to every other node — including its **private key** — so a store id
names the same certificate everywhere. (The key material travels only over the
cluster's own authenticated sync, never to a client.) Because the ids match, the
**block page's** certificate selection is shared too. The **DoT/DoH** selections
stay **node-local**, though: each node answers on its own hostname, so each picks
the certificate issued for *its* name. Deleting a certificate on one node removes
it cluster-wide on the next sync.

**Private keys and backups.** Keys are written owner-only (`0600`) and are **not**
included in a [configuration backup](backup.md) — a backup carries the selections,
not the server's private identity. After restoring onto a fresh box, generate or
upload the certificates again.
