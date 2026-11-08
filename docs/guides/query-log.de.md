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
- **Server** — in einem [Hochverfügbarkeits](high-availability.de.md)-Verbund zeigt ein
  kleines Abzeichen, welcher Knoten die Anfrage beantwortet hat, und zwar per
  **Hostname** (dem Host-Teil der Knoten-Adresse) statt einer kryptischen ID. Die Zeilen
  des eigenen Knotens sind gedämpft und als dieser Knoten markiert. Auf einer
  Einzelinstanz gibt es nur einen Server, daher bleibt das Abzeichen verborgen.

## Suche

Tippe ins Suchfeld, um das Protokoll zu filtern. Es trifft auf den **Namen** und auf
das **Gerät** — einschließlich des sprechenden Gerätenamens, sodass die Suche nach
*„Tablet“* alles findet, was das von dir „Kinder-Tablet“ genannte Gerät gefragt hat.

## Filtern und die gesamte Historie

Links neben der Liste liegt eine **Filterschiene**. Damit grenzt du das Protokoll ein
nach:

- **Quelle** — eine oder mehrere von Geblockt, Cache, Upstream, Lokal oder Rewrite
  (mehrere anhaken zeigt alle davon).
- **Record-Typ** — A, AAAA, HTTPS und so weiter.
- **Antwortcode** — NOERROR, NXDOMAIN, SERVFAIL, …
- **Resolver** — welcher Upstream geantwortet hat (es erscheinen nur die, die zuletzt
  geantwortet haben).
- **Gerät / Gruppe** — ein einzelnes [benanntes Gerät](devices.de.md) oder eine ganze
  [Gruppe](policies.de.md). Es werden nur benannte Geräte angeboten; eine Gruppe trifft
  alle Geräte darin. Ein als Adressbereich (CIDR) definiertes Gerät trifft dabei jede
  beobachtete Adresse in seinem Bereich — nach derselben „spezifischstes Gerät gewinnt“-
  Zuordnung wie in der Gerätespalte.
- **Von / Bis** — ein Zeitraum.

Sobald du einen Filter setzt, wechselt die Seite vom Live-Verlauf auf die **gesamte
gespeicherte Historie**: Sie liest direkt aus dem dauerhaften Protokoll auf der Platte,
du bist also nicht mehr auf die jüngsten, im Arbeitsspeicher gehaltenen Einträge
beschränkt. Weil diese Ansicht über mehrere Tage reicht, zeigt die **Zeit**-Spalte hier
zusätzlich das **Datum**. Die Live-Aktualisierung pausiert, solange gefiltert wird (es
gibt nichts zu tailen, wenn du in die Vergangenheit schaust). Mit **Ältere laden** unten
blätterst du weiter zurück, mit **Filter zurücksetzen** löschst du alles und kehrst zum
Live-Verlauf zurück.

Das Suchfeld wirkt zusammen mit den Filtern — so zeigst du etwa jede **geblockte**
Abfrage nach *„doubleclick“* über einen bestimmten Nachmittag.

## An- oder ausschalten

Das Protokoll hängt davon ab, dass **Anfrage-Protokoll** unter
[Einstellungen](settings.de.md) aktiv ist. Das ist ein echter Schalter an der
Aufzeichnung selbst: Schaltest du ihn aus, schreibt diese Node keine neuen Zeilen mehr
— Live-Ansicht, Explorer und die Dashboard-Statistik bekommen dann keine neuen Einträge
(das bereits Gespeicherte bleibt, bis die Aufbewahrung es entfernt). Wieder an, und die
Aufzeichnung läuft ab der nächsten Anfrage weiter — ohne Neustart. Dort stellst du auch
ein, **wie viele Tage** Verlauf behalten werden; ältere Einträge werden automatisch
entfernt.

## Live-Ansicht vs. ganzer Cluster

Die **Live**-Ansicht zeigt nur die Anfragen, die **diese Node** beantwortet hat — ein
schneller Strom des eigenen Verkehrs aus dem Speicher. Sobald du **filterst**, liest die
Seite stattdessen die volle gespeicherte Historie, und im
[Cluster](high-availability.de.md) spannt die über **jede** Node: jede Node holt die
Einträge ihrer Partner, und ein **Server-Abzeichen** zeigt, welche Node die jeweilige
Zeile beantwortet hat. Nutzt ein Gerät also eine andere Node für DNS, siehst du es hier
nicht in der Live-Ansicht — filtere (oder öffne das Protokoll auf jener Node), dann
tauchen seine Anfragen auf, mit ihrem Node-Abzeichen.

---

## Für die technisch Neugierigen

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

**Zwei Lesepfade — warum.** Der Live-Verlauf liefert aus einem kleinen In-Memory-Ring
der jüngsten Ereignisse; das macht ihn sofort verfügbar und erlaubt das Polling alle
paar Sekunden, ohne die Platte anzufassen. Die gefilterte Ansicht führt stattdessen
eine echte Abfrage gegen den DuckDB-Store aus, der die gesamte aufbewahrte Historie
hält, nicht nur den Ring. Das sind zwei wirklich verschiedene Quellen — deshalb schaltet
ein gesetzter Filter den Live-Verlauf ab: Der Ring kann „jede geblockte AAAA-Abfrage am
letzten Dienstag“ nicht beantworten, und der Store hängt den allerneuesten Ereignissen
um bis zu ein paar Sekunden hinterher (der Writer sammelt Zeilen und schreibt sie in
kurzem Intervall gebündelt weg), ist also die falsche Quelle für einen sofortigen Tail.
Die gefilterten Lesezugriffe nutzen eine zweite, nur lesende Datenbankverbindung, die
von der des Writers abgezweigt ist, sodass ein langer Scan den Strom neuer Ereignisse
nie blockiert.

**Das Server-Abzeichen** löst den Ursprung einer Zeile — die Knoten-ID, mit der der
Cluster-Abgleich jeden übernommenen Eintrag markiert — zur Anzeigezeit zu einem Hostnamen
auf, anhand des HA-Rosters (Knoten-ID → beworbene Adresse je Knoten). Angezeigt wird der
Host-Teil dieser Adresse, also `dns-02` statt einer 16-stelligen ID. Das Roster kommt über
einen kleinen Endpunkt `/api/nodes`, den jeder angemeldete Nutzer aufrufen darf (er trägt
nur Knoten-IDs und Hostnamen, nie den Sync-Schlüssel oder den Peer-Zustand), damit der
Explorer Zeilen auch für Betrachter beschriften kann, nicht nur für Admins. Ein Knoten ohne
beworbene Adresse zeigt schlicht kein Abzeichen.

**Blättern.** „Ältere laden“ nutzt *Keyset*-Paginierung statt eines Offsets: Jede Seite
merkt sich `(Zeit, Zeile)` ihres letzten Eintrags, und die nächste Seite fragt nach
Zeilen, die strikt älter sind. Das bleibt korrekt und schnell auch über Millionen von
Zeilen und wenn viele Ereignisse dieselbe Millisekunde teilen — wo ein schlichtes
`OFFSET` driften oder langsam würde. Der Block-Grund eines Eintrags wird weiterhin bei
Bedarf geholt, wenn du ihn aufklappst, sodass das Filtern kein Schema und keine Kosten
pro Zeile hinzufügt.
