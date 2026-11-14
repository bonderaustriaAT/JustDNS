# Browser hardening

*A German version of this guide is in [browser-hardening.de.md](browser-hardening.de.md).*

Every page and every API response the dashboard serves carries a set of HTTP security
headers. They tell the browser to treat the dashboard strictly: run only code and styles
that came from JustDNS itself, never render the dashboard inside someone else's frame, and
hand out as little information as possible. It is on by default and there is nothing to
configure — in the spirit of the other invisible hardening, it costs you nothing.

## What it does

- **Content-Security-Policy.** The browser will only load scripts, styles, images, fonts
  and connections from JustDNS's own origin (plus the web-font hosts, see below). If a
  cross-site scripting hole ever let an attacker inject a `<script>` that points somewhere
  else, the browser refuses to run it.
- **Anti-clickjacking.** `X-Frame-Options: DENY` and the policy's `frame-ancestors 'none'`
  stop any other site from loading the dashboard inside an invisible frame to trick you
  into clicking.
- **No MIME sniffing.** `X-Content-Type-Options: nosniff` keeps the browser from
  second-guessing a file's type and running, say, an uploaded image as a script.
- **A tight referrer policy.** `Referrer-Policy: no-referrer` means the dashboard never
  leaks the page you were on (which can carry addresses or ids) to anywhere you click out
  to.
- **A locked-down permissions policy.** `Permissions-Policy` switches off the camera,
  microphone, geolocation and the ad-topics API for the page — the dashboard needs none of
  them.

## What it does *not* do (yet)

- **No HSTS.** There is deliberately no `Strict-Transport-Security` header. The dashboard
  is commonly reached over plain HTTP on a home network, and HSTS there would lock the
  browser into HTTPS for a host that may not serve it — a foot-gun. It belongs together
  with serving the dashboard over HTTPS by default, which is a separate step.

---

## For the technically curious

**The exact policy.** One middleware layer sets the headers on every response, public and
authenticated alike, so even the login screen and the health probe carry them. The policy
is:

```
default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none';
form-action 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'
https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com;
img-src 'self' data:; connect-src 'self'; worker-src 'self'; manifest-src 'self'
```

**Why `'unsafe-inline'` is on `style-src` but not `script-src`.** The dashboard is a
compiled single-page app: every script is an external, self-hosted module (`script-src
'self'` is enough, and inline script is refused — the strong half of the protection). The
UI does, however, set inline `style` attributes — a meter bar's width, for instance — and
those count as inline styles, so `style-src` has to allow them. Inline styles cannot
execute code, so the trade-off is sound.

**The Google Fonts exception.** The dashboard loads its web fonts from `fonts.googleapis.com`
(the stylesheet) and `fonts.gstatic.com` (the font files), so the policy names those two
hosts. Self-hosting the fonts would let both drop out of the policy and keep the browser
from talking to Google at all — a worthwhile follow-up for a privacy-minded product.

**Where it is applied.** The header layer wraps the whole dashboard router (the data API,
the auth endpoints and the embedded UI). It does not touch the DNS listeners or the
optional block page, which are served elsewhere.
