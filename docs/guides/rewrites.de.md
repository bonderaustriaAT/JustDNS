# DNS-Rewrites & Adress-Objekte

*Eine englische Fassung dieser Anleitung liegt in [rewrites.md](rewrites.md).*

Ein **Rewrite** sagt JustDNS, einen Namen selbst zu beantworten — aus einer festen
Adresse, die du wählst — statt das Internet zu fragen. So zeigst du eigene Namen auf
eigene Geräte — `nas.home` → `192.168.20.5`, `blog.home` → dein Reverse-Proxy —
sodass sie überall im Netzwerk funktionieren, ohne auf jedem Gerät Dateien zu
bearbeiten. Öffne die Seite **DNS-Rewrites**.

## Ein Rewrite anlegen

1. Gib einen oder mehrere **Namen** ein, die dieselbe Antwort teilen sollen (z. B.
   `nas.home`).
2. Gib eine oder mehrere **IP-Adressen** ein. IPv4-Adressen werden zu **A**-Records,
   IPv6 zu **AAAA**.
3. Speichern.

Ab jetzt bekommt jeder im Netzwerk, der diesen Namen abfragt, deine Adresse — sofort
von JustDNS beantwortet.

- Ein Rewrite kann **ausgeschaltet** werden, ohne es zu löschen — es bleibt in der
  Liste, wird aber nicht mehr lokal beantwortet.
- Du kannst ein Rewrite jederzeit **umbenennen** oder seine Adressen ändern.

## Adress-Objekte (wiederverwendbare Adressen)

Wird dieselbe Adresse von vielen Namen genutzt — etwa dein Reverse-Proxy auf
`192.168.20.9` bedient zehn Hostnamen — willst du diese Adresse nicht zehnmal
eintippen. Ein **Adress-Objekt** gibt der Adresse einen eigenen Namen:

1. Lege unter **Adress-Objekte** eines an, z. B. `reverse-proxy` = `192.168.20.9`.
2. Zeige ein Rewrite beim Anlegen auf das **Objekt**, statt die Adresse zu tippen.
3. Ändere die Adresse des Objekts einmal, und **jedes Rewrite, das es nutzt,
   aktualisiert sich automatisch**.

Ein Adress-Objekt, das noch von einem Rewrite genutzt wird, lässt sich nicht löschen
— JustDNS sagt dir, welches Rewrite noch darauf zeigt, sodass nie ein Rewrite ins
Leere läuft.

## Gut zu wissen

- Rewrites und Adress-Objekte sind Teil deiner [Sicherung](backup.de.md).
- Ein Rewrite beantwortet den exakten Namen (und ist eine lokale Antwort, also
  schnell und verlässt nie dein Netzwerk).

---

## Für die technisch Neugierigen

Rewrites laufen **zuerst** in der Abfrage-Kette — vor Safe Search, vor der Filterung,
vor dem Cache. Ein treffendes Rewrite wird lokal beantwortet und die Abfrage geht nie
nach draußen. Weil es vor der Filterung läuft, ist ein Rewrite bewusst global: es ist
für jeden Client gleich (es gibt kein Rewrite pro Client). Ein Rewrite trifft den
**exakten** Namen; es nutzt keine Subdomain-Wildcards wie Sperr-/Erlauben-Einträge.

Adress-Objekte werden beim Beantworten eines Rewrites in dessen konkrete
Adressmenge aufgelöst, sodass das Bearbeiten eines Objekts ohne Duplikate auf jedes
referenzierende Rewrite durchschlägt. Die Referenz wird beim Speichern geprüft — du
kannst ein Rewrite nicht auf ein nicht existierendes Objekt zeigen lassen und kein
Objekt löschen, das ein Rewrite noch referenziert. Alles liegt in `rewrites.json` auf
dem `/data`-Volume.
