# Das Protokoll

*Eine englische Fassung dieser Anleitung liegt in [query-log.md](query-log.md).*

Das **Protokoll** ist die detaillierte Aufzeichnung jeder DNS-Abfrage: was gefragt
wurde, von welchem Gerät, wann, wie es beantwortet wurde und worauf es auflöste. So
beantwortest du „Warum lädt diese Seite nicht?“ oder „Mit wem redet dieser Smart-TV
eigentlich?“.

## Eine Zeile lesen

Jeder Eintrag zeigt:

- **Zeit** der Abfrage.
- **Gerät / Client** — der Gerätename, wenn du es [benannt hast](devices.de.md), sonst
  die IP-Adresse.
- **Name**, der abgefragt wurde.
- **Wie es beantwortet wurde** — eines von:
  - **Cache** — aus dem lokalen Cache geliefert.
  - **Upstream** — bei deinen Resolvern nachgefragt.
  - **Lokal** — von JustDNS selbst beantwortet.
  - **Rewrite** — von einem deiner [Rewrites](rewrites.de.md) beantwortet.
  - **Geblockt** — von einem Filter gestoppt.
- **Adressen**, auf die es auflöste (leer bei einer Sperre oder einem
  Nicht-Adress-Record).

## Suche

Tippe ins Suchfeld, um das Protokoll zu filtern. Es trifft auf den **Namen** und auf
das **Gerät** — einschließlich des sprechenden Gerätenamens, sodass die Suche nach
*„Tablet“* alles findet, was das von dir „Kinder-Tablet“ genannte Gerät gefragt hat.

## An- oder ausschalten

Das Protokoll hängt davon ab, dass **Anfrage-Protokoll** unter
[Einstellungen](settings.de.md) aktiv ist. Dort stellst du auch ein, **wie viele
Tage** Verlauf behalten werden; ältere Einträge werden automatisch entfernt.

---

## Für Technik-Interessierte

Einträge werden in einen DuckDB-Store auf dem `/data`-Volume geschrieben. Die
Aufbewahrung wird durchgesetzt, indem Einträge älter als das eingestellte Fenster
entfernt werden. Ein „Geblockt“-Eintrag hält die Kategorie und die konkrete Liste oder
Regel fest, die traf (dieselbe Reihenfolge, die die
[Richtlinien-Engine](policies.de.md) anwendet: zuerst Erlauben, dann Schadsoftware,
deine eigene Sperrliste, Ad-hoc-Sperren von Gruppe und Gerät, deine Feeds, dann
Werbung — der erste Treffer gewinnt). Weil Sperren und Rewriting vor dem Cache
passieren, werden diese Quellen getrennt von Cache- und Upstream-Antworten
protokolliert, sodass das Protokoll genau widerspiegelt, welche Stufe jede Antwort
erzeugt hat. Das gezeigte Gerät wird aus der Quell-IP des Clients bei der Anzeige über
deine Geräteliste aufgelöst, sodass das Umbenennen eines Geräts auch vergangene Zeilen
aktualisiert.
