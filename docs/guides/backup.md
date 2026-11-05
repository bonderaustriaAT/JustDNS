# Backup & restore

*A German version of this guide is in [backup.de.md](backup.de.md).*

A **backup** saves your whole JustDNS configuration to a single file you can keep
safe, move to another machine, or restore after a reinstall. Backup and restore live
under [Settings](settings.md) (and restore is also offered during
[first-run setup](setup.md)).

## What's in a backup

Everything you configured in the dashboard:

- Your settings (resolvers, Safe Search, block response, logging, …)
- Your allow and block lists and your feed subscriptions
- Your DNS rewrites and address objects
- Your named devices, groups and their [policies](policies.md)

**What's left out, on purpose:** the query log (it's history, and it regenerates) and
your admin login (so the file carries no password). That means a backup is safe to
store, but it is *not* a way to copy your login to another box — you set a fresh admin
account on the new machine.

## Make a backup

1. Open **Settings**.
2. Click **Download backup**.
3. A single `.json` file is saved. Keep it somewhere safe.

Do this after any big change, and keep a recent copy before upgrading.

## Restore a backup

You can restore in two places:

- **During first-run setup** — choose *Restore from backup*, create your admin
  account, then upload the file.
- **On a running instance** — under **Settings**, choose restore and upload the file.

Restore replaces your current settings, lists and rewrites with the file's contents.
JustDNS checks that the file is actually a JustDNS backup and refuses anything else.

## Good to know

- A backup from an older version still restores on a newer one.
- Everything takes effect live after a restore — no restart needed.

---

## For the technically curious

The backup is a single versioned JSON envelope (`app`/`kind`/`version`) so a restore
can reject a foreign or future-format file before touching anything. New sections —
address objects, devices, groups — were added with defaults, so an older backup that
lacks them still restores (the missing parts simply come back empty). On restore,
rewrites are applied first because they validate as they are stored, groups are
restored before the devices that reference them, and each piece takes effect live via
the same on-change path the UI uses. The admin account and the query log on the target
are left untouched. All of this persists on the `/data` volume, which is the only
state that survives an image update — so a backup is your off-box copy of that state.
