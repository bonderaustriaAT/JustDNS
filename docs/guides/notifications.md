# Notifications & alerts

*A German version of this guide is in [notifications.de.md](notifications.de.md).*

JustDNS can **reach out to you** when something in your network needs attention, instead
of waiting for you to notice it in the dashboard. Turn it on, point it at a webhook, and
JustDNS sends a short message when a cluster node goes down, a new version is ready, a TLS
certificate is about to expire, or a single device suddenly starts blocking a suspicious
amount of traffic.

Everything is configured in **Settings → Notifications**. There is no separate account to
create and no cloud service in the middle: JustDNS posts directly to the webhook you
choose, from each node.

## Choosing a channel

JustDNS delivers over a **webhook** — a URL that receives a message by HTTP POST. Pick the
**format** that matches your receiver:

- **Generic / ntfy (JSON)** — a plain `{ "title", "message" }` body. This is what
  [ntfy](https://ntfy.sh), [Gotify](https://gotify.net) and most self-hosted receivers
  accept. The simplest option, and a good default.
- **ntfy (plain text)** — the message as a plain-text body with the title as an
  `X-Title`/`Title` header, matching ntfy's own publish format.
- **Slack** and **Discord** — their *incoming webhook* format. Create an incoming webhook
  in the channel, paste its URL, and messages land in that channel.
- **Telegram** — talk to [@BotFather](https://t.me/BotFather) to create a bot, put its
  **token** in the token field, and the **chat ID** of the person or group to notify in
  the chat-ID field.

The URL (or Telegram token) often contains a secret, so the field is **masked** like a
password. It still stores and sends normally — the masking is only on screen.

Use **Send test** to post a one-off message with your saved settings. Save first (the form
saves automatically a moment after you stop typing), then send: the test uses the stored
configuration, not the unsaved form. If the webhook rejects the message you get the error
back inline, so you can fix the URL or format before relying on it.

## What JustDNS tells you about

Four events, each switched on or off on its own:

- **Node down / back up** — a node in your [high-availability](high-availability.md)
  cluster stops answering its peers. You get one message when it goes down and another when
  it returns, so a brief blip and a real outage look different.
- **Update available** — a new JustDNS version has been published and is ready to install
  from the [Updates](updates.md) page.
- **Certificate expiring** — a stored TLS [certificate](certificates.md) is within its
  **lead time** of expiring (14 days by default). It is the nudge to renew before DoT, DoH
  or the [block page](block-page.md) start serving an expired certificate.
- **Device traffic spike** — a single device is blocked an unusual number of times inside
  a short window. A device that suddenly makes hundreds of blocked lookups is a classic
  sign of malware beaconing to domains on your block lists. This one is a **heuristic**, so
  it is **off by default** and has two knobs: the **threshold** (blocks per device) and the
  **window** (minutes). Start loose and tighten once you know your network's normal.

## Who sends what, in a cluster

Every node evaluates on its own and sends its **own** alerts, and each message is prefixed
with the sending node's **instance name** so you can tell them apart. Node-down,
certificate-expiry and device-spike are things each node knows about its own world, so each
node reports its own. "Update available" is identical on every node, so to avoid getting it
once per node, **only one node sends it** — the others stay quiet.

---

## For the technically curious

**Delivery** is a single blocking HTTP POST per alert, sent with the same `ureq` client the
cluster uses for peer sync. Each format shapes the request differently: Generic and Slack
and Discord send a small JSON body (`{title, message}`, `{text}` and `{content}`
respectively); ntfy sends the body as plain text with a `Title` header; Telegram posts to
`https://api.telegram.org/bot<token>/sendMessage` with the chat ID and text. There is no
queue and no retry — a failed send is logged and the next evaluation tries again.

**The evaluator** is a background thread, in the same family as the update check, the HA
sync and the retention prune. It wakes about once a minute, and if notifications are on it
compares the current world against a small in-memory record of what it last alerted about,
so it fires on **transitions**, not on every tick:

- *Node down* comes from the HA peer roster — a peer counts as down when it has a recorded
  error **and** its last successful sync is older than roughly three sync intervals. The
  alert fires when a peer enters or leaves that set, not while it stays there.
- *Update* fires once per new version string, from the same update snapshot the Updates
  page reads.
- *Certificate expiry* is computed from each stored certificate's not-after time against
  the configured lead time; it alerts once per certificate per day while it stays inside
  the window, not every minute.
- *Device spike* runs a `GROUP BY client` count over the blocked rows in the window, node-
  local only (it reads this node's own rows, not the cluster-wide union), and alerts a
  device over the threshold with a per-device cooldown so one noisy device does not spam
  you.

**The "only one node sends the update alert" rule** is a simple deduplication: the node
with the lowest reachable node id in the HA roster is the one that sends cluster-identical
alerts. No election and no shared lock — just a deterministic choice every node can make
from the roster it already has, so exactly one sends and the rest skip it.

**The configuration** rides in the main [settings](settings.md), so in a cluster it syncs
like any other setting — every node agrees on the channel, the format and which events are
on. The webhook URL and Telegram token are stored in `config.json` on the `/data` volume in
clear, like the rest of the configuration; there is no separate secret store in this
version. The webhook URL is a write-only secret: `GET /api/settings` returns it only to an
**administrator**, lower roles get it blanked, and a **downloaded** backup omits it (re-enter
it once after a restore; the node-local pre-update backup on `/data` keeps it). Link-local /
cloud-metadata targets (such as `169.254.169.254`) are also refused as a destination, while a
self-hosted ntfy/Gotify on your own LAN stays allowed.
