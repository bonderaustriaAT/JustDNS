# First-run setup

*A German version of this guide is in [setup.de.md](setup.de.md).*

The first time you open JustDNS, a short **setup wizard** runs. It creates your admin
login and gets DNS working, so you can start using the dashboard right away. You only
see it once — on a fresh install with no account yet.

## Before you start

Have JustDNS reachable in a browser (the address of the machine or container it runs
on). If you already have a [backup](backup.md) from another install, keep the `.json`
file handy — the wizard can restore from it.

A language switcher sits at the top of the wizard, so you can run it in English or
German.

## Step 1 — Choose how to start

- **Fresh setup** — set everything up from scratch with a few sensible defaults.
- **Restore from backup** — bring back the settings, lists, rewrites, devices and
  groups from a backup file.

## Step 2 — Create your admin account

Pick a **username** and **password** (at least 8 characters, entered twice). This is
the login you'll use for the dashboard from now on. On a fresh setup you can also give
the instance a **name** here — handy if you run more than one. There is no password
recovery, so store it somewhere safe.

The account is created the moment you move on from this step, and its credentials
can't be changed later in the wizard.

## If you chose "Fresh setup"

**Step 3 — Upstream resolvers.** Choose how JustDNS looks up names it doesn't answer
itself:

- **Privacy** — encrypted resolvers that don't log.
- **Balanced** — a sensible mix of speed and privacy (the default).
- **Speed** — the fastest public resolvers.

You can change this later under [Settings](settings.md).

**Step 4 — Finish.** Pick how many **days of query history** to keep (7, 30, 90 or
365), review the summary, and finish. You land on the dashboard, signed in.

## If you chose "Restore from backup"

**Step 3 — Upload the file.** Select your backup `.json`. JustDNS checks it's a real
JustDNS backup and refuses anything else. On success, all your settings, lists,
rewrites, devices and groups come back and you land on the dashboard.

Note: a backup never carries a login, which is why you still create an admin account
in step 2 even when restoring.

## What next

- Point a device's DNS at JustDNS and watch the [Overview](overview.md) come alive.
- Name your devices so the [query log](query-log.md) reads in plain language — see
  [Devices](devices.md).
- Tune what gets blocked globally, then refine per device or group with
  [policies](policies.md).

---

## For the technically curious

The wizard is gated on whether an admin account exists; once one does, the route is
closed and the normal login takes over. Both branches create the account first,
because every later call (saving settings, restoring a backup) is authenticated —
the restore in particular runs as an authenticated request against the same endpoint
the Settings page uses. "Fresh" writes only the fields the wizard exposes (instance
name, speed profile, log retention) on top of the stored defaults, so untouched
settings keep their default values rather than being overwritten. The speed profile
maps to a resolver set; retention is the window the [query log](query-log.md) store
prunes to. Everything the wizard writes lands on the `/data` volume, so it survives
image updates and restarts.
