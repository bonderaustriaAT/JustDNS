# Users, roles & two-factor

*A German version of this guide is in [users-roles-mfa.de.md](users-roles-mfa.de.md).*

JustDNS started with a single administrator. From this release you can add **more
accounts**, give each one a **role** that decides what it may change, and protect
sign-ins with **two-factor authentication (MFA)** — optionally **required** for
everyone or for individual people.

You'll find these in two places: your own **Account** sits under the account menu
in the top bar, and — for administrators only — **Users** and the **Audit log**
live in the **System** group of the sidebar (on a phone the sidebar slides in from
the menu button in the top-left corner).

## Roles

Every account has one of three roles, from most to least powerful:

| Role | May do |
| --- | --- |
| **Administrator** | Everything: user management, server settings, TLS, backups — plus all of the below. |
| **Editor** | Change filtering content: block/allow lists, DNS rewrites, devices, groups, schedules, and clear the cache. Not settings, users, TLS or backups. |
| **Viewer** | Read-only. See every page, change nothing. |

A higher role can always do what a lower one can. On a page your role may only
read, a short **read-only** note appears at the top, so you know a change would be
refused before you try it. The server enforces every role on its own — hiding a
button is a convenience, not the lock.

Your existing administrator is unchanged: an account from before this feature
loads as an **Administrator** with full access.

> **The last administrator is protected.** JustDNS refuses to demote or delete the
> only remaining administrator, so you can never lock yourself out of management.

## Two-factor authentication (MFA)

Two-factor adds a second step to signing in: after your password, a **6-digit
code** from an authenticator app on your phone (Google Authenticator, Microsoft
Authenticator, Aegis, and similar). Even someone who learns your password cannot
sign in without your phone.

When you set MFA up you also get **ten recovery codes**. Each works **once** in
place of the app code, for when you don't have your phone. They are shown **only
once** — save them somewhere safe.

### Turn it on for yourself

1. Open **Account** from the menu.
2. Under **Two-factor authentication**, click **Set up**.
3. **Scan the QR code** with your authenticator app (or type the key by hand).
4. **Save your recovery codes.**
5. Enter the 6-digit code the app now shows and click **Enable two-factor**.

From the next sign-in on, JustDNS asks for a code after your password.

### Turn it off, or get fresh recovery codes

On the **Account** page, with MFA on:

- **Regenerate recovery codes** — mints a new set and invalidates the old one.
  Confirm with your password.
- **Turn off** — removes MFA (confirm with your password). This is unavailable
  when MFA is **required** for your account.

## Requiring MFA (administrators)

As an administrator you can make MFA mandatory, two ways — they combine (the
server enforces the **or** of the two, so either one requires MFA for an account):

- **For everyone** — the **Require two-factor for all accounts** banner at the top
  of the **Users** page. Flip it on and every account must use MFA. While it is on,
  each row's per-person switch is moot and shows **Enforced** instead.
- **For one person** — on the **Users** page, flip the switch in the **Required**
  column next to their account.

When MFA is required for someone who has not set it up, their next sign-in walks
them through enrolment **before** they reach the dashboard — they cannot skip it,
and they cannot turn MFA off afterwards while it stays required.

## Managing users (administrators)

Open **Users** from the **System** group in the sidebar.

### Add someone

Click **+ Add user**. In the dialog, fill in a **username**, an initial
**password** (at least 8 characters), pick a **role**, optionally flip on **Require
two-factor**, and click **Create**. Tell them the password over a trusted channel;
they can change it on their own Account page.

### Change an existing account

Each row lets you:

- **Change the role** from the dropdown.
- Toggle the switch in the **Required** column to demand MFA for that person (hidden
  behind **Enforced** while the global requirement above is on).
- **Reset password** — set a new one (e.g. after someone is locked out).
- **Reset two-factor** — clear their MFA after a lost phone. If MFA is still
  required for them, they enrol again at the next sign-in.
- **Delete** the account. You cannot delete your own, and the last administrator
  is protected.

## The audit log (administrators)

**Audit log** in the sidebar's **System** group shows the recent security-relevant events,
newest first: who signed in (and failed sign-ins), sign-outs, account and MFA
changes, user management, settings changes and backup restores. Each row shows the
time, the **actor**, the **action**, the **target** and whether it **succeeded**.

It never contains a password, a secret or a code — only *that* something happened.
Use it to answer "who changed this, and when".

## Good to know

- Sessions are not stored on disk, so **restarting JustDNS signs everyone out** —
  they log straight back in. MFA and accounts themselves do persist.
- Repeated wrong passwords or codes are **rate-limited** for a few minutes, to slow
  down guessing.
- Accounts and MFA are **not** part of a configuration backup, by design — a backup
  you share should not carry password hashes. Restoring a backup never touches the
  account list.

---

## For the technically curious

**Where it is stored.** Accounts live in `auth.json` on the `/data` volume: a
username, an Argon2id password hash, the role, and — once enrolled — the MFA state
(a base32 TOTP secret, a confirmed flag, and Argon2 hashes of the unused recovery
codes). Every field added for this feature has a serde default, so an older
`auth.json` (a lone admin with no role or MFA) loads unchanged as a full
administrator. The server-wide *Require two-factor* switch is a field in
`config.json`.

**TOTP.** Codes are standard RFC 6238 TOTP: SHA-1, 6 digits, a 30-second step, with
a ±1 window for clock drift. The QR code is an `otpauth://` URI rendered to SVG by
the server, so the secret never leaves the box through a third-party QR service.
Recovery codes are ten `XXXXX-XXXXX` strings; only their Argon2 hashes are kept,
and a used one is removed on the spot.

**Sign-in flow.** The password check mints a short-lived *pending* session. If the
user has MFA, the session waits in `PendingMfa` until a valid TOTP or recovery code
promotes it; if MFA is required but not set up, it waits in `PendingEnroll` until
the user enrols. Only a fully promoted session passes the auth guard — a pending
cookie opens nothing. Full sessions last a week; pending ones expire in ten
minutes.

**Authorisation.** One middleware runs on every API request behind the guard: it
resolves the session to a user, looks up their role live (so a role change takes
effect at once), and compares it to the minimum the endpoint needs — reads are open
to any signed-in user, content changes need Editor, and settings, TLS, backups,
user management and the audit log need Administrator. Self-service account
endpoints are open to the holder and only ever act on themselves.

**Audit log.** Events are appended to `audit.log` on `/data` as one JSON object per
line, bounded to the most recent entries so the file cannot grow without limit. It
is rewritten atomically on each event and holds no secrets — only actor, action,
target, outcome and a short non-secret detail. It is deliberately excluded from
backups.

**Rate limiting.** Failed password and second-factor attempts are counted per
username in memory, over a rolling five-minute window; past a threshold, further
attempts are refused with `429` until the window elapses. It resets on restart —
appropriate for a LAN appliance, not a public server.
