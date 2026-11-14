# Privatsphäre-Dashboard

*Die englische Fassung dieses Leitfadens steht in [privacy-trackers.md](privacy-trackers.md).*

Das [Protokoll](query-log.de.md) beantwortet, *was* gefragt wurde. Die **Privatsphäre**-Seite
beantwortet, *wer dahintersteckt*: Sie nimmt jeden Namen, den deine Geräte aufgelöst haben,
und ordnet ihn der **Firma** zu, die ihn betreibt. So siehst du auf einen Blick, dass ein
Handy heute 240-mal Google kontaktiert hat, oder dass ein Smart-TV ständig mit einer
Analyse-Firma redet, von der du noch nie gehört hast. Aus einer trockenen Domain-Liste wird
eine Geschichte, die man auch jemandem zeigen kann, der keine DNS-Logs liest.

Du öffnest sie über **Privatsphäre** in der Seitenleiste (das Auge-im-Schild-Symbol, nahe
der Übersicht und dem Protokoll). Jeder angemeldete Benutzer darf sie lesen.

## Was du siehst

Oben drei Zahlen für das ganze Netzwerk über den gewählten Zeitraum:

- **Tracker-Kontakte** – wie viele Anfragen an eine Firma aus dem Katalog gingen.
- **Geblockt** – wie viele davon JustDNS abgewiesen hat, mit dem geblockten Anteil.
- **Durchgekommen** – wie viele beantwortet wurden, weil keine Liste gegriffen hat.

Darunter eine Schlagzeile in einem Satz (*„wohnzimmer-tv kontaktierte Google 1.204×"*),
die **meistkontaktierten Firmen** über alle Geräte und eine Liste **nach Gerät**. Jede
Firmenzeile trägt einen farbigen Punkt für ihre Kategorie und einen Balken, dessen grüner
Teil den geblockten Anteil zeigt – ein überwiegend grüner Balken heißt also, deine Listen
tun ihre Arbeit, ein überwiegend bernsteinfarbener ist eine Firma, die durchkommt.

Den Zeitraum – **24 Stunden**, **7 Tage** oder **30 Tage** – wählst du mit dem Schalter
oben rechts. Deine Wahl bleibt in diesem Browser gespeichert.

### In ein Gerät hineinschauen

Tippe auf ein Gerät, um nur die Firmen zu sehen, die *es* erreicht hat, die aktivste zuerst.
Das ist die Ansicht für den Moment, in dem ein Gerät ungewöhnlich gesprächig wirkt: Sie
nennt die Organisationen hinter dem Verkehr und zeigt, wie viel davon geblockt und wie viel
beantwortet wurde. Mit **‹ Alle Geräte** geht es zurück.

### Eine Firma von hier aus blocken

Tippe in der Liste **meistkontaktierte Firmen** auf eine Firma, um ihre eigene Ansicht zu
öffnen: welche Geräte sie erreicht haben und welche genauen Domains aufgelöst wurden. Wenn
du Listen bearbeiten darfst (Editor oder Admin), trägt jede Ansicht einen **Blocken**-Knopf:

- **Firma blocken** fügt *alle* Domains, die der Katalog zu dieser Firma kennt, zu deiner
  [Blockliste](lists.de.md) hinzu – nicht nur die bisher gesehenen – damit sie ab der
  nächsten Anfrage überall geblockt ist.
- **Blocken** neben einer einzelnen Domain fügt nur diesen einen Namen hinzu.

Das Blocken ist idempotent: Noch einmal gedrückt wird nichts doppelt hinzugefügt, und das
Ergebnis sagt dir, wie viele Einträge neu waren. Die Einträge landen in deiner eigenen
Blockliste (mit dem Firmennamen markiert), du kannst sie also jederzeit auf der Seite
[Sperr- & Erlauben-Listen](lists.de.md) prüfen oder rückgängig machen. Das ist die einzige
Aktion, die das Dashboard ausführt; alles andere ist nur Ansicht.

## Kategorien

Jede Firma im Katalog gehört zu einer von vier Familien, jede mit eigener Farbe:

| Kategorie | Was sie bedeutet |
| --- | --- |
| **Werbung** | Werbenetzwerke und Börsen, die Anzeigen ausliefern oder versteigern. |
| **Analyse** | Messung, Attribution und Sitzungsaufzeichnung. |
| **Social Media** | Tracking- und Teilen-Widgets sozialer Plattformen. |
| **Fingerprinting** | Geräte-Fingerprinting, Bot-Erkennung und Betrugsbewertung. |

## Was es *nicht* zeigt

- **Nur node-lokale Anfragen.** Das Dashboard zählt Anfragen, die diese Node beantwortet
  hat. In einem [Hochverfügbarkeits-Cluster](high-availability.de.md) zeigt jede Node ihren
  eigenen Verkehr; sie holt nichts von einer anderen Node.
- **Nur Namen aus dem Katalog.** Eine Anfrage, die zu keiner katalogisierten Firma gehört,
  wird hier schlicht nicht gezählt – das ist kein „unbekannter Verkehr", sondern Verkehr,
  den diese Funktion nicht zuordnet. Ein ruhiges Dashboard kann ein ruhiges Netzwerk heißen
  oder Listen, die die meisten Tracker schon vorher geblockt haben.
- **Blocken wirkt global.** Der **Blocken**-Knopf fügt zur netzwerkweiten Blockliste hinzu,
  nicht für ein einzelnes Gerät. Um eine Firma nur für ein Gerät zu blocken, nutze eine
  [Geräte-Richtlinie](policies.de.md); um einen ganzen Dienst mit einem Schalter zu sperren,
  die [Blockierten Dienste](blocked-services.de.md).

---

## Für die technisch Neugierigen

**Woher die Zuordnung kommt.** Der Firmenkatalog ist in JustDNS einkompiliert, wie die
Liste der blockierbaren Dienste – eine kuratierte Auswahl der großen Werbe-, Analyse-,
Social- und Fingerprinting-Netzwerke und der registrierbaren Domains, die sie nutzen. Er
ist bewusst nicht vollständig, steckt im Binary, holt also keinen Feed und funkt nichts
nach Hause. Es ist nicht DuckDuckGos Tracker Radar: dieser Datensatz steht unter
CC BY-NC-SA (nicht-kommerziell), was nicht zu einem AGPL-Produkt passt – der Katalog hier
ist eigenständig und von Hand gepflegt.

**Wie ein Name zugeordnet wird.** Die Zuordnung läuft über die Elternlabels des Namens,
genau wie die Blockier-Engine: `px.ads.doubleclick.net` probiert den ganzen Namen, dann
`ads.doubleclick.net`, dann `doubleclick.net` – was im Katalog steht – und hört auf,
zugeschrieben an Google. Ein Name, der ohne Treffer oben ankommt, bleibt unzugeordnet.
Jede registrierbare Domain gehört genau einer Firma, eine Anfrage wird also nie doppelt
gezählt.

**Wie es berechnet wird.** Ein gruppierter DuckDB-Lesevorgang über den Zeitraum liefert je
Gerät und je Name, wie viele Anfragen es gab und ob sie geblockt waren; der Handler ordnet
jeden Namen über den Katalog zu und summiert die Treffer je Firma und je Gerät. Der Scan
ist gedeckelt, und das Ganze ist eine Abfrage pro Seitenaufruf – es läuft nicht im Hot-Path,
bremst also das Auflösen nicht.
