# DNS rewrites & address objects

*A German version of this guide is in [rewrites.de.md](rewrites.de.md).*

A **rewrite** tells JustDNS to answer a name itself, from a fixed address you choose,
instead of asking the internet. It is how you point your own names at your own
machines — `nas.home` → `192.168.20.5`, `blog.home` → your reverse proxy — so they
work everywhere on your network without editing files on each device. Open the
**DNS-Rewrites** page.

## Add a rewrite

1. Enter one or more **names** that should share the same answer (e.g. `nas.home`).
2. Enter one or more **IP addresses**. IPv4 addresses become **A** records, IPv6
   become **AAAA**.
3. Save.

From now on, anyone on your network who looks up that name gets your address,
answered instantly by JustDNS.

- A rewrite can be switched **off** without deleting it — it stays in the list but is
  no longer answered locally.
- You can **rename** a rewrite or change its addresses at any time.

## Address objects (reusable addresses)

If the same address is used by many names — say your reverse proxy at
`192.168.20.9` serves ten hostnames — you don't want to type that address ten times.
An **address object** gives the address a name of its own:

1. Under **Address objects**, create one, e.g. `reverse-proxy` = `192.168.20.9`.
2. When adding a rewrite, point it at the **object** instead of typing the address.
3. Change the object's address once, and **every rewrite that uses it updates
   automatically**.

An address object that is still used by a rewrite cannot be deleted — JustDNS tells
you which rewrite still points at it, so you never leave a rewrite dangling.

## Good to know

- Rewrites and address objects are part of your [backup](backup.md).
- A rewrite answers the exact name (and is a local answer, so it is fast and never
  leaves your network).

---

## For the technically curious

Rewrites run **first** in the query pipeline — before Safe Search, before filtering,
before the cache. A matching rewrite is answered locally and the query never goes
upstream. Because it runs before filtering, a rewrite is global by design: it is the
same for every client (there is no per-client rewrite). A rewrite matches the **exact**
name; it does not wildcard subdomains the way block/allow entries do.

Address objects are resolved into a rewrite's concrete address set when the rewrite is
answered, so editing an object fans out to every referencing rewrite with no
duplication. The reference is validated on save — you cannot point a rewrite at an
object that doesn't exist, and you cannot delete an object a rewrite still references.
Everything persists in `rewrites.json` on the `/data` volume.
