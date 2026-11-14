# Weltkarte und beobachtete Netze

*Eine englische Fassung dieses Leitfadens liegt in [world-map.md](world-map.md).*

Die [GeoIP-Funktion](geoip.de.md) zeigt schon Land und Netz hinter jeder Antwort. Die
Weltkarte macht daraus ein Bild, und die Beobachtungsliste macht daraus eine Aktion: Du
kannst ein Land oder ein Netz als **beobachtet** markieren, und JustDNS hebt dann jede
Antwort hervor, die dorthin auflöst – fürs ganze Netz oder nur für ein Gerät bzw. eine
Gruppe.

Das ist **rein ein Lese-Marker**. Beobachten blockt nie etwas und berührt nie den
DNS-Hotpath; es markiert und – wenn du willst – alarmiert. Um ein Ziel wirklich zu sperren,
nimmst du wie gewohnt eine Geräte- oder Gruppen-[Richtlinie](policies.de.md).

## Die Karte

Auf dem **Privatsphäre**-Dashboard beginnt der Abschnitt **Wohin telefonieren deine Geräte**
jetzt mit einer Weltkarte. Jedes Land, das dein Netz im gewählten Zeitraum erreicht hat, ist
nach Kontaktvolumen eingefärbt – je mehr, desto kräftiger das Grün. Die Rangliste der Netze
(ASN) steht darunter.

Ein Klick auf ein Land hebt es hervor und filtert die Liste auf dessen Netze. Noch ein Klick,
ein Klick aufs Meer oder **Alle Länder zeigen** hebt den Filter wieder auf.

Die Karte ist ein gebündeltes, offline eingebettetes SVG – keine Map-Bibliothek, keine
Netzwerkaufrufe. Es ist die *Simple World Map* von Al MacDonald (Originalgrafik) und Fritz
Lekschas (Bearbeitung), genutzt unter
[CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/deed.de).

## Ein Land oder Netz beobachten

Als Administrator (auf dem Hauptknoten eines Clusters) zeigt ein ausgewähltes Land in der
Filterleiste einen Schalter **Land beobachten**, und jede Netz-Zeile eines fokussierten
Landes einen **beobachten**-Schalter. Diese bearbeiten die **globale** Beobachtungsliste –
sie gelten für jedes Gerät.

Für eine engere Beobachtung öffnest du auf der **Geräte**-Seite ein Gerät oder eine Gruppe
und nutzt in deren Richtlinie die Felder **Länder beobachten** und **Netze (ASN) beobachten**.
Die effektive Beobachtungsmenge eines Geräts ist die Vereinigung aus globaler Liste, der
Liste seiner Gruppe und seiner eigenen.

- Länder sind ISO-3166-1-Alpha-2-Codes, durch Leerzeichen oder Komma getrennt (z. B. `ru cn`).
- Netze sind ASN-Nummern, mit oder ohne `AS`-Präfix (z. B. `13335 15169`).

## Was Beobachten bewirkt

- **Im DNS-Protokoll** trägt eine beobachtete Zeile ein bernsteinfarbenes **beobachtet**-
  Abzeichen und einen dezenten bernsteinfarbenen Rand – so fällt ein Gerät auf, das mit einem
  markierten Land oder Netz spricht.
- **Als Alarm** (optional, standardmäßig aus): Schalte unter Benachrichtigungen
  **Beobachtetes Land/Netz kontaktiert** ein, und JustDNS meldet an deinen Webhook, wenn eine
  jüngste Antwort in ein beobachtetes Netz aufgelöst hat. Ausgewertet wird node-lokal, mit
  Pro-Netz-Abklingzeit (ein anhaltender Kontakt meldet höchstens stündlich) und unter
  Beachtung der effektiven Beobachtungsmenge je Client (global, Gruppe, Gerät).

## Für technisch Interessierte

- Die Beobachtungsmenge wird für die Protokoll-Markierung und den Alarm identisch aufgelöst,
  darum stimmen beide überein.
- An der Anfrage selbst wird nichts gespeichert: Die „beobachtet"-Markierung wird beim Lesen
  des Protokolls bestimmt, sodass eine geänderte Liste auch ältere Zeilen sofort neu markiert.
- Die globale Beobachtungsliste liegt in den synchronisierten Einstellungen und wird so im
  Cluster geteilt; Geräte- und Gruppen-Beobachtungen reiten auf der normalen Geräte-Richtlinie.
- Der Alarm feuert nie aus dem DNS-Hotpath – ein kleiner Hintergrund-Durchlauf scannt jüngste
  Antworten, genau wie der Alarm zu Geräte-Ausreißern.
