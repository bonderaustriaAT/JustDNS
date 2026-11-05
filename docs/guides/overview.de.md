# Die Übersicht

*Eine englische Fassung dieser Anleitung liegt in [overview.md](overview.md).*

Die **Übersicht** ist der Startbildschirm — ein Live-Dashboard dessen, was dein DNS
gerade und über den letzten Tag tut. Hier stellst du nichts ein; sie ist der Puls des
Systems.

## Was du siehst

- **Anfragen aufgelöst** — der große Zähler, wie viele Abfragen JustDNS beantwortet
  hat.
- **Live-Rate** — Anfragen pro Sekunde oder pro Minute, umschaltbar, aktualisiert
  sich mit dem Verkehr.
- **Anfragen pro Stunde** — ein Balkendiagramm der letzten 24 Stunden, damit du
  siehst, wann dein Netzwerk beschäftigt ist.
- **Aus dem Cache** — der Anteil der Antworten, die sofort aus dem Cache kamen statt
  aus einer frischen Upstream-Abfrage. Höher ist schneller.
- **Geblockt** — wie viele Abfragen gefiltert wurden.
- **Aktive Geräte** — wie viele Geräte gerade abfragen.
- **Letzte 24 Stunden** — Summen für Anfragen und Sperren, plus die **Antwortzeit**.
- **Aktivste Clients** — die geschäftigsten Geräte, mit Namen, wenn du sie
  [benannt hast](devices.de.md).

## Antwortzeit

JustDNS zeigt drei Zahlen, damit eine langsame Abfrage dich nicht täuscht:

- **Durchschnitt** über jede Abfrage (Cache-Treffer eingeschlossen).
- **Median** — die typische Abfrage. Bei einem Cache-lastigen Setup liegt dieser nahe
  null, weil die meisten Antworten direkt aus dem Cache kommen.
- **95. Perzentil (p95)** — der langsame Rand: 95 % der Abfragen waren mindestens so
  schnell.

Zusammen gelesen zeigen sie sowohl, wie schnell es sich im Alltag anfühlt (Median),
als auch, wie schlimm die gelegentliche langsame Abfrage wird (p95).

---

## Für Technik-Interessierte

Die Live-Rate wird auf dem Client aus aufeinanderfolgenden Abfragen des
Statistik-Endpunkts abgeleitet, nicht serverseitig gespeichert. Die 24-Stunden-Werte
und die Antwortzeit-Perzentile werden über aufgezeichnete Abfragen im Protokoll-Store
berechnet, hängen also davon ab, dass das [Protokoll](query-log.de.md) aktiv ist. Die
Antwortzeit-Statistik wird über **alle** Abfragen genommen, Cache-Treffer
eingeschlossen — deshalb liest sich eine Cache-lastige Instanz schnell; Median und p95
stehen gerade deshalb neben dem Mittelwert, damit eine einzelne langsame
Upstream-Abfrage die Schlagzahl nicht aus dem Verhältnis ziehen kann.
