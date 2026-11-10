# Encryption at rest

*A German version of this guide is in [secrets-at-rest.de.md](secrets-at-rest.de.md).*

JustDNS keeps a few secrets on its data volume: the account list (password hashes,
TOTP secrets, recovery codes) and the secret parts of your settings (a notification
webhook or bot token). From this release those files are **encrypted on disk**, so a
copied data volume, a backup or a VM snapshot is useless to whoever holds it without
the key.

There is **nothing to switch on and nothing to configure.** It happens by itself.

## What this protects — and what it doesn't

Encryption at rest defends your secrets when the **data** is taken: someone copies the
`/data` volume, lifts a backup archive, or clones the disk or VM. All they get is
ciphertext.

It does **not** defend a fully compromised, running server. A live JustDNS has to read
its own key unattended to start without you typing a passphrase, so an attacker who
already has root on the running host can read it too. Guarding against that would mean
entering a secret on every boot — which we deliberately avoid, because it would turn a
routine restart or one-click update into a lockout waiting to happen. The realistic
threat for a self-hosted box is a stolen volume or backup, and that is exactly what
this closes.

## The key, and the one thing to remember

The encryption key is a random value generated the first time this version starts. It
lives in **its own volume** (`/keys`), separate from your data — that separation is the
whole point, so that a copied data volume does not also hand over the key.

> **Back up the key, separately from your data.** If you lose the key, the encrypted
> files cannot be read — the account list and settings are gone, and you would start
> over with a fresh admin account. The official `docker compose` install keeps the key
> in a named volume (`justdns-keys`) that survives updates, so in normal operation you
> never touch it. Just include that volume in whatever you use to back the box up —
> and keep that backup somewhere apart from the data backup, or storing them together
> undoes the separation.

## Upgrading

The first time the new version starts, it reads your existing plaintext `auth.json` and
`config.json`, writes them back **encrypted**, and carries on. You do nothing, and
nothing about how you log in or use the dashboard changes.

This is a one-way step. An **older** JustDNS build cannot read the encrypted files, so
once a node is on this version, do not roll its image back without also restoring a
pre-upgrade backup. In a [high-availability](high-availability.md) cluster each node
encrypts its own files independently; the upgrade needs no coordination there.

## For the technically curious

The key is a 32-byte random master key, generated with the system CSPRNG on first boot
and written to `JUSTDNS_SECRET_KEY_PATH` (default `/keys/master.key`) with owner-only
permissions, flushed to disk before use. From it, each secret file derives its own
subkey with **HKDF-SHA256** under a per-file label (`auth-v1`, `config-v1`), so no two
files share the exact key. The payload is sealed with **ChaCha20-Poly1305**; the
on-disk envelope is `magic || nonce (12 bytes) || ciphertext || tag`, with a fresh
random nonce per write.

Loading is **fail-closed**: a file that carries the sealed magic but will not decrypt
(a missing or wrong key, or a tampered file) is reported as an error and the node
refuses to start, rather than being mistaken for "no accounts" and silently starting
empty — which would hand control to whoever reached the setup screen first. A legacy
plaintext file (no magic) is read once and immediately rewritten sealed, which is the
transparent upgrade above.

Everything is keyed off the file on disk, so the mechanism is independent of the HA
sync (whose peer transport has its own, separate encryption keyed by the cluster's
shared key).

If you run JustDNS without the official compose file — a bare `docker run`, say — mount
a persistent volume at `/keys` yourself (`-v justdns-keys:/keys`). If you don't, the key
is regenerated each time the container is recreated; because loading is fail-closed,
you then get a clear "key missing or does not match" error on the next start rather than
silent data loss, and mounting the original key volume recovers it.
