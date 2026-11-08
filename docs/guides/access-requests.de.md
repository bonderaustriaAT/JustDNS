# Freigabe-Anfragen

*Eine englische Fassung dieser Anleitung liegt in [access-requests.md](access-requests.md).*

Wenn jemand im Netzwerk auf einer gesperrten Seite landet, bietet die
[Block-Seite](block-page.de.md) jetzt einen Knopf — **Seite freigeben lassen** —, über
den er um Zugang bitten kann, statt nur gestoppt zu werden. Jede Anfrage landet in
einer Warteschlange, die du unter **System → Freigaben** abarbeitest (nur Admin). Ein
kleines Zeichen am Menüeintrag zeigt, wie viele warten.

## Was der Besucher sieht

Auf der Block-Seite steht unter der Erklärung ein Knopf **Seite freigeben lassen**.
Öffnet er ihn, erscheinen ein optionales Begründungsfeld (warum er die Seite braucht)
und ein Knopf **Anfrage senden**. Nach dem Senden bestätigt die Seite mit *Anfrage
gesendet* — kein Konto, kein Login, nichts zu installieren. Freigeben kann der Besucher
nichts selbst; er fragt nur an.

Der Knopf erscheint, sobald die Block-Seite weiß, welche Seite gesperrt wurde (Hosted-
und Reverse-Proxy-Modus, wo der geblockte Hostname mitgeführt wird). Für einen Namen,
der gar nicht gesperrt ist, tut er nichts — so lassen sich keine beliebigen Einträge
unterschieben.

## Was eine Anfrage festhält

Jede Karte in der Warteschlange zeigt:

- **Die Seite** — die exakt gesperrte Domain, als Überschrift.
- **Von** — das Gerät, von dem die Anfrage kam (sein Name aus deiner
  [Geräteliste](devices.de.md), und seine Gruppe, falls es eine hat). Eine Adresse,
  die JustDNS nicht als benanntes Gerät kennt, erscheint als *Unbekanntes Gerät*.
- **Wann** — wie lange die Anfrage her ist.
- **IP-Adresse** — die Client-Adresse, von der sie kam.
- **Blockiert durch** — *wodurch* die Seite gestoppt wurde: die passende eigene Liste
  bzw. das Feed, oder die Kategorie (Werbung & Tracker, Schadsoftware, eigene Liste),
  mit der genauen Trefferregel, wenn sie von der Domain abweicht.
- **Begründung** — die Notiz des Besuchers, falls er eine hinterlassen hat.

## Eine Seite freigeben

Jede Karte hat einen Auswahlschalter **Freigeben für** mit drei Reichweiten, dann
**Freigeben** und **Ablehnen**:

- **Dieses Gerät** — nur das Gerät, das gefragt hat, darf die Seite künftig erreichen.
  Die Domain kommt in die eigene Erlaubnisliste dieses Geräts (zusammengeführt mit dem,
  was dort schon steht). Das ist die Voreinstellung.
- **Ganze Gruppe** — alle Geräte der Gruppe des fragenden Geräts dürfen sie erreichen.
  Die Domain kommt in die Erlaubnisliste der Gruppe. Nur verfügbar, wenn das Gerät in
  einer Gruppe ist.
- **Alle Geräte** — die Seite wird dauerhaft fürs ganze Netzwerk freigegeben. Die Domain
  landet in der **globalen Erlaubnisliste**. Weil das die weiteste Reichweite ist, fragt
  JustDNS vorher nach einer Bestätigung.

Eine Erlaubnis gewinnt immer vor einer Sperre, also löst die Seite nach der Freigabe für
die gewählte Reichweite wieder normal auf — ab der nächsten Anfrage, ohne Neustart. Das
Freigeben oder Ablehnen entfernt die Karte aus der Warteschlange; der bleibende Nachweis
ist der Erlaubnis-Eintrag selbst (sichtbar und umkehrbar unter [Block/Allow](lists.de.md))
und das Audit-Protokoll.

**Ablehnen** räumt die Anfrage weg, ohne an der Filterung etwas zu ändern — für eine
Seite, die du nicht öffnen willst.

## Im Cluster

In einem [Hochverfügbarkeits-Cluster](high-availability.de.md) ist die Warteschlange
geteilt: Eine Anfrage, die auf der Node gestellt wurde, die die geblockte Anfrage
beantwortet hat, erscheint innerhalb eines Sync-Intervalls auf jeder Node, und eine
Freigabe oder Ablehnung propagiert genauso. Einmal freigeben, auf einer beliebigen Node.

## Für die technisch Neugierigen

Die Warteschlange ist ein kleiner JSON-Speicher (`access-requests.json`) auf dem
Datenträger, indexiert über eine deterministische Id aus Client-Adresse und Domain. So
frischt ein Besucher, der den Knopf malträtiert, nur einen Eintrag auf, statt Dubletten
zu stapeln. Die Block-Seiten-
Listener schreiben über eine Senke hinein, die zuerst die Policy-Engine
(`PolicyEngine::explain`) für genau diesen Client und Namen erneut befragt: Eine Anfrage
wird nur aufgezeichnet, wenn der Name für diesen Client wirklich gesperrt ist — daraus
werden auch Gerät, Gruppe, Kategorie und passende Liste gefüllt. Die Admin-API
(`/api/access-requests`, admin-gated) listet die Warteschlange und gibt einen Eintrag
frei oder weist ihn ab; eine Freigabe bildet die Reichweite auf den passenden
Erlaubnis-Mechanismus ab — globale Erlaubnisliste, `PolicyDelta` des Geräts oder der
Gruppe — und führt die Domain in die bestehende Policy ein, sodass nichts anderes
verloren geht. Der Speicher spiegelt die übrigen JustDNS-Speicher (atomares Schreiben,
On-Change-Hook) und reitet damit auf dem bestehenden HA-Ledger-Merge mit
Last-Writer-Wins und Tombstones.
