# Sperr- & Erlauben-Listen

*Eine englische Fassung dieser Anleitung liegt in [lists.md](lists.md).*

Hier entscheidest du, was JustDNS standardmäßig fürs ganze Netzwerk blockt und was
immer durchgelassen werden soll. Öffne die Seite **Block/Allow**.

Alles hier setzt die **globale Vorgabe** — die Basis, die jeder Client bekommt. Für
Ausnahmen bei einem Gerät oder einer Gruppe siehe [Richtlinien](policies.de.md).

## Die drei Dinge auf dieser Seite

### 1. Immer erlauben

Domains auf dieser Liste werden **nie** geblockt — auch dann nicht, wenn eine aktive
Sperrliste sie blocken würde. So behebst du einen *Fehlalarm* (false positive): eine
Seite, die du willst, die eine Liste aber fälschlich erwischt.

- Füge eine Domain hinzu (z. B. `meine-bank.example`), optional mit einer Notiz, warum.
- Ein Eintrag gilt automatisch für alle Subdomains: `example.com` erlaubt auch
  `www.example.com`. Um nur einen Host zu erlauben, trag genau diesen Host ein
  (`login.example.com`).

**Eine Freigabe gewinnt immer über eine Sperre.** Sie ist die stärkste Regel im System.

### 2. Immer sperren

Deine eigene manuelle Sperrliste — Domains, die weg sollen, egal welches Abo. Es gilt
dieselbe Subdomain-Regel (`tracker.example` sperrt auch `ads.tracker.example`).

### 3. Abos & der Katalog

Ein **Abo** ist eine externe Sperrliste (ein „Feed“), die JustDNS herunterlädt und
aktuell hält — zum Beispiel die kuratierten
[Hagezi](https://github.com/hagezi/dns-blocklists)-Listen. Der **Katalog** bietet eine
Reihe fertiger Feeds; schalte einen an und JustDNS holt ihn.

Jeder Katalog-Eintrag zeigt ein **Fehlalarm-Risiko** (1 = vernachlässigbar … 5 = hoch),
damit du vor dem Einschalten einschätzen kannst, wie aggressiv er ist. Du kannst auch
einen eigenen Feed per URL hinzufügen.

Klick **Alle Listen aktualisieren**, um jetzt jeden Feed neu zu laden; sonst
aktualisieren sie sich nach eigenem Zeitplan.

## Die eingebauten Kategorien

Zwei Sperrmengen stecken fest in JustDNS und werden über einfache Schalter unter
[Einstellungen](settings.de.md) gesteuert, nicht hier:

- **Werbung & Tracker**
- **Schadsoftware & Phishing**

Im [Richtlinien](policies.de.md)-Editor verhalten sie sich wie jede andere Liste und
erscheinen dort als `Werbung & Tracker` und `Schadsoftware`.

## Gut zu wissen

- Erlauben- und Sperr-Einträge, deine Abos und welche Katalog-Feeds an sind, stecken
  alle in deiner [Sicherung](backup.de.md).
- Änderungen gelten live für neue Abfragen.

---

## Für die technisch Neugierigen

**Matching.** Einträge sind Host-Regeln, per Domain-Suffix abgeglichen: ein Eintrag
für `example.com` trifft `example.com` und jedes `*.example.com`, aber nicht
`nichtexample.com`. Vollständige `*.example.com`-Wildcard-Syntax wird akzeptiert und
wie `example.com` behandelt; Teil-Wildcards wie `ads.*.com` nicht.

**Auflösungs-Reihenfolge.** Zur Rebuild-Zeit verschmelzen deine manuellen
Erlauben-Einträge und jeder aktive *Allow*-Feed zu einer Erlauben-Menge; deine
manuellen Sperr-Einträge und jeder aktive *Block*-Feed werden zu Sperrquellen; die
eingebauten Werbung/Schadsoftware-Mengen kommen je nach Schalter hinzu. Die
[Richtlinien-Engine](policies.de.md) prüft dann zuerst Erlauben (eine Freigabe bricht
ab), dann die Sperrquellen in fester Reihenfolge. Die erste treffende Sperrquelle ist
das, was das [Protokoll](query-log.de.md) als Grund nennt.

**Feeds auf der Platte.** Abos werden unter dem `/data`-Volume zwischengespeichert,
sodass der Blocker beim Start sofort und ohne Netzwerk-Runde gefüllt ist; ein
geplanter Refresh (und der Knopf **Alle aktualisieren**) holt sie neu. Deine
Listen-Konfiguration übersteht Neustarts und Image-Updates.
