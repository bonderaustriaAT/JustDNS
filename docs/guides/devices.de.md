# Geräte

*Eine englische Fassung dieser Anleitung liegt in [devices.md](devices.md).*

Dein Netzwerk spricht in IP-Adressen — `192.168.20.37`, `192.168.20.41` — die auf
einen Blick nichts sagen. Die Seite **Geräte** lässt dich diesen Adressen sprechende
Namen geben, sodass Übersicht und Protokoll *„Wohnzimmer-TV“* statt einer Zahl
zeigen. Ein Gerät zu benennen ist außerdem der erste Schritt, um ihm eigene
[Filterregeln](policies.de.md) zu geben.

## Die Geräteliste

Öffne **Geräte**. Du siehst zwei Arten von Zeilen:

- **Benannte Geräte**, die du angelegt hast.
- **Gesehene, aber unbenannte Clients** — Adressen, die zuletzt DNS-Abfragen gestellt
  haben, aber noch keinen Namen haben. Jede zeigt eine **Zuletzt gesehen**-Zeit, eine
  **Anfragezahl** und einen Punkt, der leuchtet, wenn der Client **gerade aktiv** ist.

So liest sich die Seite wie „Was ist gerade in meinem Netzwerk“.

## Ein Gerät benennen

1. Klick **Benennen** bei einem unbenannten Client (das füllt seine Adresse vor), oder
   tippe eine Adresse ins Feld **IP oder Bereich** oben.
2. Gib einen **Namen** ein (z. B. *Wohnzimmer-TV*).
3. Wähle eine **Art** (Handy, Laptop, Fernseher, …) — das setzt nur ein kleines Symbol.
4. Füge optional eine **Notiz** hinzu (z. B. *Kinderzimmer*).
5. Klick **Hinzufügen**.

Ab jetzt erscheint diese Adresse überall in JustDNS mit Namen, und auch die
Protokollsuche findet sie über den Namen.

## Einen ganzen Bereich benennen

Statt einer einzelnen Adresse kannst du einen **CIDR-Bereich** eintragen, z. B.
`192.168.30.0/24` mit Namen *Gast-WLAN*. Jede Adresse in diesem Bereich löst zum
Namen auf — praktisch für ein Gastnetz oder ein IoT-VLAN. Hat eine einzelne Adresse
*innerhalb* des Bereichs einen eigenen benannten Eintrag, gewinnt die exakte Adresse.

## Bearbeiten oder entfernen

- **Bearbeiten** lässt dich Adresse, Name, Art, Notiz, [Gruppe und Regeln](policies.de.md)
  ändern.
- **Entfernen** macht aus dem Gerät wieder eine unbenannte Sichtung — es stoppt die
  Abfragen nicht, es vergisst nur den Namen.

## Gut zu wissen

- „Gerade aktiv“ heißt, eine Abfrage wurde innerhalb eines Fensters gesehen, das du
  unter [Einstellungen](settings.de.md) ändern kannst (Standard 5 Minuten).
- Benannte Geräte stecken in deiner [Sicherung](backup.de.md).

---

## Für Technik-Interessierte

Gerätenamen sind eine **Lese-Angelegenheit**: Protokoll und Statistik lösen die
Quell-IP eines Clients erst bei der Anzeige zu einem Namen auf, über einen Resolver,
der aus einem Schnappschuss deiner Geräteliste gebaut wird. Es gibt keinen
Namens-Lookup pro Abfrage auf dem Hot-Path. Das *Ziel* eines Geräts wird normalisiert
und kanonisch gespeichert (eine maskierte CIDR-Basis, eine kanonische IP), sodass
Datei und UI genau das zurückspielen, was der Matcher nutzt. Die Zuordnung eines
Clients zu einem Gerät nutzt Longest-Prefix, also schlägt ein exaktes `/32` jeden
Bereich und ein `/24` schlägt ein `/16` — dieselbe Mechanik, die auch die
[Richtlinien](policies.de.md) nutzen, um zu entscheiden, welche Regeln gelten.
