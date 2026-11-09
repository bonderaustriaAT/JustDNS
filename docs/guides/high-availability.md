# High availability (node mesh)

*A German version of this guide is in [high-availability.de.md](high-availability.de.md).*

A single JustDNS is a single point of failure: when it reboots, updates or
crashes, name resolution on your network stops. High availability lets you run
**up to four JustDNS nodes** that stay in sync, so any one of them can answer on
its own and you can take one down without the network noticing.

All nodes are **equal** — every one is writable, and a change made on any node
flows to all the others. There is no fixed primary. JustDNS keeps the configurations
in sync and shows you each node's health; it does **not** switch traffic over for
you (see [Why there is no automatic failover](#why-there-is-no-automatic-failover)).

The nodes form a **full mesh**: every node syncs directly with every other, so one
node going down never interrupts the sync among the rest, and a node coming back
catches up automatically. You only ever connect a new node to **one** existing node
by hand — it discovers and meshes with the others itself.

The feature is **off by default**. Nothing syncs, and the node-to-node endpoint
stays closed, until you turn it on and join the nodes yourself.

## What gets synced

Everything you set in the dashboard:

- **Settings** — all server settings (resolvers, DNSSEC, speed profile, …).
- **Lists** — your allow and block entries, and your feed subscriptions.
- **Rewrites** — DNS rewrites and the reusable address objects they reference.
- **Devices and groups** — named clients, their groups and per-scope policies.
- **Schedules** — the recurring time windows.
- **Certificates** — the whole [certificate store](certificates.md), **including the
  private keys**, so a certificate you add, upload or renew on one node is present on
  all of them (its key travels only over this authenticated node-to-node sync, never
  to a client). The block page's certificate selection rides along; the DoT/DoH
  selections stay node-local, since each node serves its own hostname.
- **Users and logins** — every account with its role, password and second factor
  (TOTP secret and recovery codes). The same logins — enforced MFA included — apply
  on every node. Confirmed MFA is treated as sticky: once a node has it for an
  account, the sync keeps it and spreads it to the others, so a stray older copy can
  never silently switch two-factor off. The trade-off is deliberate — **to remove
  MFA from an account, clear it on each node**, or a node that still has it will
  re-add it on the next sync.

- **Query log / statistics history** — each node also pulls the **other nodes'
  query-log entries**, so the *Query log* explorer on any node shows the whole
  cluster's traffic over the retention window, and the lifetime statistics do not
  reset to zero after you fail over to another node. Only the explorer and the
  running totals union; the live dashboard (the recent ring, today's activity,
  per-device and per-resolver views) stays this node's own. Each log row carries a
  small **node badge** showing which node it came from. How far back this reaches
  is the **log retention** you set under *Settings → Query log* (seven days by
  default): that window now both prunes the local log and bounds what is synced.

What is **not** synced, on purpose: anything node-local — the node id, this node's
own address and display name, its DoT/DoH **hostname** and certificate selections,
the **block-page server IP** (each node points blocked names at itself), the mesh
membership, **login sessions** (you sign in on each node separately), and
operational state. Turning HA off on one node therefore never switches it off on the
others.

## Setting it up

You set up **each JustDNS instance the normal way first**, each on its own host.
Then, on **every** node:

1. **Open *High availability*** in the sidebar (admin only).
2. **Turn high availability on** (step 1 of the wizard).
3. **Set the shared key** (step 2) — a long, random secret. It must be
   **exactly the same on every node**; it is how they prove themselves to each
   other. You can also pin it with an environment variable (below).
4. **Set this node's own address** (step 3) — the web address the other nodes
   reach it at (e.g. `http://192.168.20.5:8080`). It is prefilled from the address
   you opened the page at. This is advertised across the cluster so newly joining
   nodes can mesh back to this one; without it, this node can pull from others but
   stays invisible to new joiners.
5. **Join the cluster** (step 4) — enter the address of **any one** node already in
   the cluster (e.g. `http://192.168.20.6:8080`). JustDNS checks the connection with
   the shared key and, on success, meshes with that node — then discovers and
   connects to all the others on its own.

The **first** node you set up has no one to join yet; it simply waits at step 4
until another node joins it, then switches to the cluster view by itself.

Once a node is meshed, the page becomes the **cluster list**: this node and every
peer, each with a health dot, address, version and last-sync time, plus buttons to
**sync now** and **leave cluster**, and a per-peer remove (the × on a peer's card).

### Removing a node

Removing a peer with the **×**, or **leaving the cluster** yourself, asks you to
confirm first and then **tells the remaining nodes**, so the node is dropped across the
whole mesh at once — it does not linger as an unreachable entry that every node keeps
trying to reach. The removal is sticky: another node's copy of the membership can no
longer bring it back. A node removed by mistake simply **rejoins by connecting again**
(its data on the other nodes is kept, so it converges cheaply). A node that is only
**temporarily offline** should be left in place — it catches up on its own when it
returns; remove it only when it is gone for good.

To add a **third or fourth** node, open **Add another node** on the cluster list.
It walks you through it and shows a live node count (e.g. *2/4 nodes*): set the new
node up, turn HA on, and then — on the new node — set the **same shared key** (you
can **reveal and copy** it here) and enter **one** seed address of this cluster (the
panel offers this node's address to copy). The new node meshes with all of them
automatically, up to four nodes in total; past four, the panel says the cluster is
full.

### Who wins when a node joins

Each node was set up separately, so it already has its own settings, its own admin,
perhaps overlapping list entries. When a node **joins** the cluster, you make **one
choice** for it: *keep the cluster* (the default — this node adopts the cluster's
configuration) or *keep this node* (this node's configuration wins). That choice
settles **every real conflict** at once — the same setting, list entry, group,
device name, user, and so on.

Anything that exists on **only one** side is always kept — a join is a union, never
a deletion. After the join the nodes converge, and from then on the ordinary rule
applies: **the more recent change wins**. (That is also why re-enrolling MFA on one
node propagates it everywhere — a fresh enrollment is simply the most recent change.)

### Pinning the key with an environment variable

Instead of typing the key into the wizard you can set it on both nodes:

```
JUSTDNS_SYNC_KEY=<a long random string>
```

When it is set this way the wizard shows the key as managed outside the app and
does not let you change it there. Two more optional variables tune the sync:

```
JUSTDNS_NODE_ID=<stable id>        # auto-generated and persisted if unset
JUSTDNS_SYNC_INTERVAL=30           # seconds between sync cycles (minimum 5)
```

### Security: treat the key like the password database

Now that users are synced, the snapshot a node pulls from its partner carries **every
account, password hash and TOTP secret**, plus the private certificate keys. That
payload travels **encrypted** — each node derives a symmetric key from the shared key
(HKDF-SHA256) and seals every peer response with ChaCha20-Poly1305. A sniffer on the
wire sees only ciphertext, and the shared key itself **never** travels — a node proves
it holds the key by sending a payload that opens under it. This holds even over an open
`http` link on the same network. The shared key is therefore still the crown jewel:
whoever holds it can derive the same key, read and change the snapshot (and so defeat
MFA). So:

- **Use a long, random key** (at least 16 characters; the wizard requires it). A short
  or guessable key hands over the credential database.
- **TLS is now optional hardening, not a requirement.** The payload is encrypted either
  way; an HTTPS address for the partner only lays a second layer over it.
- **If you suspect the key is compromised:** change the key on both nodes **and** reset
  every password — the old key could already have copied the hashes.

> **Upgrade both nodes together.** The peer encryption is a coordinated cut: a node on
> this version cannot open the sealed payloads of an old node, and vice versa. If one
> node still runs an older version, sync pauses until both are upgraded — resolution
> keeps running the whole time, only the reconciliation waits.

## Pointing your clients at both

Because JustDNS does not move traffic for you, your clients should be able to use
**either** node. Two common ways:

- **Two resolvers.** Hand out both JustDNS IPs by DHCP (or in each device's
  network settings). The operating system falls back to the second resolver when the
  first does not answer. Simple, and it works everywhere.
- **A shared virtual IP.** Run something like `keepalived` in front of the pair so
  one floating IP always points at a live node. More moving parts, but clients only
  ever see one address.

## Why there is no automatic failover

Automatic failover — one node deciding the other is dead and seizing its address —
is where high-availability setups usually go wrong: a brief network blip makes both
nodes think they are alone, both claim the address, and resolution breaks worse than
if nothing had failed over. Doing it safely needs quorum and fencing, which is a lot
of machinery for a home or small-office resolver.

JustDNS deliberately stops at **sync + status**. Keeping both nodes answering, with
your clients already able to reach either, gives you the resilience without the
split-brain risk. The operating-system resolver fallback *is* your failover, and it
is battle-tested.

---

## For the technically curious

**The model is last-writer-wins with tombstones.** Each node keeps a small *ledger*
in `/data/ha.json`: for every synced entity (keyed `"<collection>/<id>"`) it records
a content hash, the time its current state was set, and which node set it. Each sync
cycle a node pulls the partner's ledger and collections, and for every key the later
`(timestamp, node-id)` wins. A deletion leaves a **tombstone** — a dated marker that
the entity is gone — so "deleted on A" beats "still present on B" instead of B
re-creating it on the next round. Tombstones are pruned after 30 days.

**It converges because the merge is symmetric and commutative.** Every node runs the
same pull-and-merge against each of its peers every interval, and the merge is a pure
function over a *total* order of `(timestamp, node-id)` — the node-id breaks a tie
when two clocks read the same millisecond. `merge(A, B)` and `merge(B, A)` yield the
same result, and merging is idempotent, so all nodes settle on the identical
configuration regardless of who merged first or in what order. Because the snapshot
also carries the partner's ledger entries, a change propagates **transitively**: A's
edit reaches C through B even if A and C never spoke, which is why the same engine
works unchanged for two nodes or four.

**The mesh assembles itself from a gossiped roster.** Alongside the config, each
snapshot carries a *roster* — node-id → address — merged by the same last-writer-wins
(a node is authoritative for its own address). When a node pulls a peer it also
announces its own id and address in the request headers, so even a node that only
ever pulls becomes known. Each cycle a node connects to any roster member it is not
yet meshed with, up to three peers, so entering **one** seed address is enough for a
new node to find and mesh with the whole cluster. The roster carries **tombstones**
too: removing a node — or a node leaving — marks its roster entry as removed, stamped
at that moment, and that marker wins and gossips the same way, so a live copy on
another node cannot resurrect it and the auto-mesh stops reconnecting. The removing (or
leaving) node also posts the departure to the remaining peers directly
(`/api/ha/depart`), so the removal takes effect at once rather than waiting for the
next pull; a peer unreachable at that instant still learns it from the tombstone later.
A node is authoritative for **its own** liveness, so one that genuinely rejoins
re-advertises itself and overrides an old tombstone.

**A join is just a one-time timestamp bump.** The "keep the cluster / keep this node"
choice is not a separate mode: the joining node's whole ledger is stamped once — far
back to adopt the cluster, or to now to win — so ordinary last-writer-wins then
settles every genuine collision, while entries unique to any node have no competitor
and simply survive. Auto-discovered peers never restamp; the seed already decided it.

**MFA is reconciled so a sync can't switch it off.** A user record is resolved as a
whole by last-writer-wins, which — when nodes first began syncing users — could let a
node whose copy carried a later timestamp *without* a second factor strip MFA from
everyone. So MFA is merged on its own: if either side has a *confirmed* factor for an
account, the merged record keeps it. The pick is deterministic and symmetric, so the
nodes converge on "MFA on" rather than flapping. The deliberate, fail-safe cost is
that removing MFA no longer propagates — clear it on each node.

**The query log syncs as an append-only union.** A query event is never edited or
deleted, so there is no conflict to resolve: each node pulls the *other* nodes'
events and adds them to its own durable log. A row carries an **origin** — empty for
a locally answered query, the peer's node id for a pulled one — and a node serves
**only its own** rows, so in a full mesh nothing is re-forwarded and there is no
loop. The pull is incremental: each node keeps a per-peer **watermark** (the newest
`at_ms` it has ingested) and asks only for rows at or after it, re-pulling the
boundary millisecond, which a natural-key dedup absorbs. A background prune deletes
rows past the retention window on every node, so the log stays bounded; the pull is
clamped to the same window, so no node fetches rows it would immediately drop. The
pulled rows feed the durable log and the lifetime totals, never the live hot view —
which is why the dashboard stays node-local while the explorer unions.

**The hash is stable across builds.** It is a 64-bit FNV-1a over each entity's
canonical JSON, not Rust's `DefaultHasher` (whose output can change between builds
and must never be persisted). That is what lets a node tell "unchanged" from "edited"
across restarts and versions.

**The peer endpoints authenticate and encrypt with the shared key, not a login.** A
node holds no dashboard session. From the shared key both sides derive the same AEAD
key (HKDF-SHA256). A call carries an `X-Ha-Auth` header holding a sealed, time-stamped
token (node id and own address, encrypted with ChaCha20-Poly1305); that it opens is
itself the proof of authenticity — the key never travels as a token, and a stamp
outside a 60-second window is rejected, which bounds replays. The responses from
`/api/ha/hello`, `/api/ha/snapshot` and `/api/ha/queries` are sealed the same way, so
the payload — certificate keys, password hashes, MFA secrets — never crosses the wire
in clear. The endpoints answer **404** until HA is enabled *and* a key is set, so the
sync surface does not exist on an instance that is not using the feature. An existing
pairing is deliberately **not** required (the key is the guard), or two fresh nodes
could never connect the first time. The management endpoints (enable, set key, connect,
sync now) sit behind the normal admin login.

**Clock drift matters.** Last-writer-wins trusts the clocks, so keep both nodes on
NTP. A node-id tiebreak keeps things from flapping when two stamps land on the exact
same millisecond, but a badly wrong clock can still let a stale edit win — the usual
caveat for any LWW system.

**Settings merge as one unit.** The whole settings object is a single ledger entry,
so if two *different* settings are changed on the two nodes at almost the same
moment, the older overall snapshot loses both. For a home or small-office pair that
is an acceptable trade; finer-grained settings merging is a possible later step.
