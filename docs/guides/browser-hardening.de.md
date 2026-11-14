# Browser-Härtung

*Die englische Fassung dieses Leitfadens steht in [browser-hardening.md](browser-hardening.md).*

Jede Seite und jede API-Antwort des Dashboards trägt einen Satz HTTP-Sicherheits-Header.
Sie weisen den Browser an, das Dashboard streng zu behandeln: nur Code und Styles
auszuführen, die von JustDNS selbst kommen, das Dashboard nie im Rahmen einer fremden
Seite darzustellen und so wenig Information wie möglich preiszugeben. Das ist standardmäßig
an und es gibt nichts einzustellen – im Sinne der übrigen unsichtbaren Härtung kostet es
dich nichts.

## Was es tut

- **Content-Security-Policy.** Der Browser lädt Skripte, Styles, Bilder, Schriften und
  Verbindungen nur vom eigenen Ursprung von JustDNS (plus die Font-Hosts, siehe unten).
  Sollte je eine Cross-Site-Scripting-Lücke einem Angreifer erlauben, ein `<script>`
  einzuschleusen, das woanders hinzeigt, weigert sich der Browser, es auszuführen.
- **Anti-Clickjacking.** `X-Frame-Options: DENY` und `frame-ancestors 'none'` in der
  Policy verhindern, dass eine andere Seite das Dashboard in einem unsichtbaren Rahmen lädt,
  um dich zu einem Klick zu verleiten.
- **Kein MIME-Sniffing.** `X-Content-Type-Options: nosniff` hält den Browser davon ab, den
  Typ einer Datei zu erraten und z. B. ein hochgeladenes Bild als Skript auszuführen.
- **Strenge Referrer-Policy.** `Referrer-Policy: no-referrer` bedeutet, dass das Dashboard
  nie verrät, auf welcher Seite du warst (die Adressen oder IDs enthalten kann), wenn du
  irgendwohin klickst.
- **Abgeriegelte Permissions-Policy.** `Permissions-Policy` schaltet Kamera, Mikrofon,
  Standort und die Werbe-Themen-API für die Seite ab – das Dashboard braucht nichts davon.

## Was es (noch) *nicht* tut

- **Kein HSTS.** Es gibt bewusst keinen `Strict-Transport-Security`-Header. Das Dashboard
  wird im Heimnetz häufig über einfaches HTTP erreicht, und HSTS würde den Browser dort auf
  HTTPS festnageln für einen Host, der das vielleicht nicht ausliefert – ein Eigentor. Es
  gehört zusammen mit dem Ausliefern des Dashboards über HTTPS per Default, was ein eigener
  Schritt ist.

---

## Für die technisch Neugierigen

**Die genaue Policy.** Eine Middleware-Schicht setzt die Header auf jede Antwort, öffentlich
wie authentifiziert, sodass selbst der Login-Bildschirm und die Health-Probe sie tragen.
Die Policy lautet:

```
default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none';
form-action 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'
https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com;
img-src 'self' data:; connect-src 'self'; worker-src 'self'; manifest-src 'self'
```

**Warum `'unsafe-inline'` bei `style-src` steht, aber nicht bei `script-src`.** Das
Dashboard ist eine kompilierte Single-Page-App: jedes Skript ist ein externes, selbst
gehostetes Modul (`script-src 'self'` genügt, und Inline-Skript wird abgewiesen – die
starke Hälfte des Schutzes). Die Oberfläche setzt allerdings Inline-`style`-Attribute –
etwa die Breite eines Balkens – und die zählen als Inline-Styles, also muss `style-src` sie
erlauben. Inline-Styles können keinen Code ausführen, der Kompromiss ist also vertretbar.

**Die Google-Fonts-Ausnahme.** Das Dashboard lädt seine Schriften von `fonts.googleapis.com`
(das Stylesheet) und `fonts.gstatic.com` (die Font-Dateien), darum nennt die Policy diese
beiden Hosts. Die Schriften selbst zu hosten würde beide aus der Policy fallen lassen und
den Browser gar nicht erst mit Google reden lassen – ein lohnender Folgeschritt für ein
auf Privatsphäre bedachtes Produkt.

**Wo es greift.** Die Header-Schicht umhüllt den gesamten Dashboard-Router (die Daten-API,
die Auth-Endpunkte und die eingebettete Oberfläche). Die DNS-Listener und die optionale
Blockseite, die anderswo ausgeliefert werden, berührt sie nicht.
