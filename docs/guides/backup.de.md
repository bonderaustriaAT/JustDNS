# Sicherung & Wiederherstellung

*Eine englische Fassung dieser Anleitung liegt in [backup.md](backup.md).*

Eine **Sicherung** speichert deine ganze JustDNS-Konfiguration in eine einzige Datei,
die du sicher aufbewahren, auf eine andere Maschine bringen oder nach einer
Neuinstallation zurückspielen kannst. Sicherung und Wiederherstellung liegen unter
[Einstellungen](settings.de.md) (und die Wiederherstellung wird auch bei der
[Ersteinrichtung](setup.de.md) angeboten).

## Was in einer Sicherung steckt

Alles, was du im Dashboard eingestellt hast:

- Deine Einstellungen (Resolver, Safe Search, Block-Antwort, Protokollierung, …)
- Deine Erlauben- und Sperrlisten und deine Feed-Abos
- Deine DNS-Rewrites und Adress-Objekte
- Deine benannten Geräte, Gruppen und ihre [Richtlinien](policies.de.md)

**Was bewusst fehlt:** das Anfrage-Protokoll (es ist Verlauf und wird neu erzeugt) und
dein Admin-Login (damit die Datei kein Passwort trägt). Eine Sicherung ist also sicher
aufzubewahren, aber *kein* Weg, deinen Login auf eine andere Kiste zu kopieren — auf
der neuen Maschine legst du ein frisches Admin-Konto an.

## Eine Sicherung erstellen

1. Öffne **Einstellungen**.
2. Klick **Sicherung herunterladen**.
3. Eine einzelne `.json`-Datei wird gespeichert. Bewahre sie sicher auf.

Mach das nach jeder großen Änderung, und halte eine aktuelle Kopie vor einem Upgrade
bereit.

## Eine Sicherung zurückspielen

Du kannst an zwei Stellen wiederherstellen:

- **Bei der Ersteinrichtung** — wähle *Aus Sicherung wiederherstellen*, lege dein
  Admin-Konto an und lade dann die Datei hoch.
- **Auf einer laufenden Instanz** — wähle unter **Einstellungen** die
  Wiederherstellung und lade die Datei hoch.

Die Wiederherstellung ersetzt deine aktuellen Einstellungen, Listen und Rewrites durch
den Inhalt der Datei. JustDNS prüft, dass die Datei wirklich eine JustDNS-Sicherung
ist, und weist alles andere ab.

## Gut zu wissen

- Eine Sicherung aus einer älteren Version lässt sich auf einer neueren
  zurückspielen.
- Nach einer Wiederherstellung wirkt alles live — kein Neustart nötig.

---

## Für die technisch Neugierigen

Die Sicherung ist ein einzelner versionierter JSON-Umschlag (`app`/`kind`/`version`),
sodass eine Wiederherstellung eine fremde oder zukünftige Formatdatei ablehnen kann,
bevor sie irgendetwas anfasst. Neue Abschnitte — Adress-Objekte, Geräte, Gruppen —
wurden mit Defaults ergänzt, sodass eine ältere Sicherung ohne sie trotzdem zurückkommt
(die fehlenden Teile kommen einfach leer zurück). Bei der Wiederherstellung werden
Rewrites zuerst angewendet, weil sie beim Speichern validiert werden, Gruppen vor den
Geräten, die sie referenzieren, und jedes Teil wirkt live über denselben
On-Change-Pfad, den die UI nutzt. Admin-Konto und Protokoll auf dem Ziel bleiben
unangetastet. All das liegt auf dem `/data`-Volume, dem einzigen Zustand, der ein
Image-Update übersteht — eine Sicherung ist also deine Kopie dieses Zustands außerhalb
der Kiste.
