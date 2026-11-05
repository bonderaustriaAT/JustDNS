# Ersteinrichtung

*Eine englische Fassung dieser Anleitung liegt in [setup.md](setup.md).*

Beim ersten Öffnen von JustDNS läuft ein kurzer **Einrichtungs-Assistent**. Er legt
deinen Admin-Login an und bringt DNS zum Laufen, sodass du das Dashboard sofort nutzen
kannst. Du siehst ihn nur einmal — bei einer frischen Installation ohne Konto.

## Bevor du startest

Erreiche JustDNS im Browser (die Adresse der Maschine oder des Containers, auf dem es
läuft). Hast du schon eine [Sicherung](backup.de.md) aus einer anderen Installation,
halte die `.json`-Datei bereit — der Assistent kann daraus wiederherstellen.

Oben im Assistenten liegt ein Sprachumschalter, du kannst ihn also auf Deutsch oder
Englisch durchlaufen.

## Schritt 1 — Wähle, wie du startest

- **Frische Einrichtung** — alles von Grund auf mit sinnvollen Vorgaben.
- **Aus Sicherung wiederherstellen** — Einstellungen, Listen, Rewrites, Geräte und
  Gruppen aus einer Sicherungsdatei zurückholen.

## Schritt 2 — Admin-Konto anlegen

Wähle **Benutzername** und **Passwort** (mindestens 8 Zeichen, zweimal eingegeben).
Mit diesem Login meldest du dich künftig am Dashboard an. Bei einer frischen
Einrichtung kannst du der Instanz hier auch einen **Namen** geben — praktisch, wenn du
mehrere betreibst. Es gibt keine Passwort-Wiederherstellung, verwahre es also sicher.

Das Konto wird in dem Moment angelegt, in dem du diesen Schritt verlässt; seine
Zugangsdaten lassen sich im Assistenten danach nicht mehr ändern.

## Wenn du „Frische Einrichtung" gewählt hast

**Schritt 3 — Upstream-Resolver.** Wähle, wie JustDNS Namen nachschlägt, die es nicht
selbst beantwortet:

- **Datenschutz** — verschlüsselte Resolver ohne Protokollierung.
- **Ausgewogen** — eine sinnvolle Mischung aus Tempo und Datenschutz (Vorgabe).
- **Tempo** — die schnellsten öffentlichen Resolver.

Das kannst du später unter [Einstellungen](settings.de.md) ändern.

**Schritt 4 — Abschluss.** Wähle, **wie viele Tage** Verlauf behalten werden (7, 30,
90 oder 365), prüfe die Zusammenfassung und schließe ab. Du landest angemeldet auf dem
Dashboard.

## Wenn du „Aus Sicherung wiederherstellen" gewählt hast

**Schritt 3 — Datei hochladen.** Wähle deine Sicherungs-`.json`. JustDNS prüft, dass es
eine echte JustDNS-Sicherung ist, und weist alles andere ab. Bei Erfolg kommen all
deine Einstellungen, Listen, Rewrites, Geräte und Gruppen zurück und du landest auf dem
Dashboard.

Hinweis: Eine Sicherung trägt nie einen Login — deshalb legst du auch beim
Wiederherstellen in Schritt 2 ein Admin-Konto an.

## Wie es weitergeht

- Richte das DNS eines Geräts auf JustDNS und sieh die [Übersicht](overview.de.md)
  lebendig werden.
- Benenne deine Geräte, damit das [Protokoll](query-log.de.md) in Klartext liest —
  siehe [Geräte](devices.de.md).
- Stell global ein, was geblockt wird, und verfeinere es dann je Gerät oder Gruppe mit
  [Richtlinien](policies.de.md).

---

## Für Technik-Interessierte

Der Assistent hängt daran, ob ein Admin-Konto existiert; sobald eines da ist, wird die
Route geschlossen und der normale Login übernimmt. Beide Zweige legen zuerst das Konto
an, weil jeder spätere Aufruf (Einstellungen speichern, Sicherung wiederherstellen)
authentifiziert ist — die Wiederherstellung läuft insbesondere als authentifizierte
Anfrage gegen denselben Endpunkt, den die Einstellungs-Seite nutzt. „Frisch" schreibt
nur die Felder, die der Assistent zeigt (Instanzname, Tempo-Profil,
Protokoll-Aufbewahrung) über die gespeicherten Defaults, sodass unberührte
Einstellungen ihre Vorgabewerte behalten statt überschrieben zu werden. Das Tempo-Profil
bildet auf einen Resolver-Satz ab; die Aufbewahrung ist das Fenster, auf das der
[Protokoll](query-log.de.md)-Store kürzt. Alles, was der Assistent schreibt, landet auf
dem `/data`-Volume und übersteht so Image-Updates und Neustarts.
