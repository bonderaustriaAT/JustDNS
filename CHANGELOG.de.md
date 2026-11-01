# Änderungsverlauf

Was sich in JustDNS getan hat, in einfacher Sprache. Eine englische Fassung
dieser Datei liegt daneben: [CHANGELOG.md](CHANGELOG.md). Datumsangaben im
Format Jahr-Monat-Tag.

## [0.1.14] - 2026-11-01

- Die obere Menüleiste bricht nicht mehr in eine zweite Zeile um und wirkt
  aufgeräumter — auch auf Deutsch, wo die Beschriftungen länger sind.

## [0.1.13] - 2026-11-01

- Die gesamte Oberfläche gibt es jetzt auf **Deutsch und Englisch**. Ein
  DE/EN-Schalter oben in der Leiste wechselt die Sprache sofort, und deine Wahl
  bleibt beim nächsten Öffnen erhalten.

## [0.1.12] - 2026-11-01

- **Schnellere Antworten.** Häufig genutzte Seiten werden im Hintergrund schon
  aufgefrischt, bevor ihr Eintrag abläuft — so bekommst du durchgehend sofortige
  Antworten statt ab und zu einer langsamen. Sind deine DNS-Anbieter kurz nicht
  erreichbar, antwortet JustDNS weiter aus den letzten Ergebnissen, damit das
  Surfen nicht stockt.

## [0.1.11] - 2026-11-01

- Das Protokoll hebt jetzt die Namen hervor, die du selbst umleitest, und zeigt
  zu jeder Anfrage die aufgelöste IP-Adresse — so siehst du auf einen Blick,
  wohin ein Gerät tatsächlich geschickt wurde.

## [0.1.10] - 2026-11-01

- Umleitungen lassen sich jetzt bearbeiten: umbenennen, mehrere Namen auf
  dieselbe Adresse zeigen lassen und einzelne abschalten, ohne sie zu löschen.
- Ein neuer Knopf „DNS-Cache leeren" unter Einstellungen sorgt dafür, dass eine
  geänderte Adresse oder eine neue Umleitung sofort greift.

## [0.1.9] - 2026-11-01

- Neu: „Sichere Suche erzwingen". Eingeschaltet halten Google, Bing, YouTube und
  DuckDuckGo für alle Geräte im Netzwerk ihren jugendsicheren Modus.

## [0.1.8] - 2026-11-01

- Du kannst einem Namen jetzt eine eigene feste Adresse geben — etwa um ein Gerät
  zu Hause über einen gut merkbaren Namen wie `nas.home` statt über seine IP zu
  erreichen.

## [0.1.7] - 2026-11-01

- Wird etwas geblockt, nennen die Anfrage-Details jetzt die genaue Liste, aus der
  die Sperre stammt — so erkennst du, warum sie gestoppt wurde.

## [0.1.6] - 2026-11-01

- Die Felder für Erlauben und Blockieren erklären jetzt, dass ein Eintrag auch
  seine Subdomains abdeckt — so ist klarer, was ein einzelner Eintrag erfasst.

## [0.1.5] - 2026-11-01

- Der Bereich „Listen" heißt jetzt „Block/Allow", und deine eigenen Erlaub- und
  Blockeinträge liegen zusammen auf einer Seite.

## [0.1.4] - 2026-11-01

- Der Listen-Bereich ist in übersichtliche Unterseiten aufgeteilt, die du über
  ein Menü erreichst — so wird keine Seite zu lang.

## [0.1.3] - 2026-11-01

- Deine per URL hinzugefügten Listen sitzen jetzt in einem aufgeräumten,
  aufklappbaren Abschnitt.

## [0.1.2] - 2026-11-01

- Der Katalog der fertigen Listen hat jetzt einen Kategorie-Filter, mit dem du
  schnell Listen für Werbung, Sicherheit, Telemetrie und mehr findest.

## [0.1.1] - 2026-11-01

- Bestimme selbst, was gefiltert wird: eigene Immer-erlauben- und
  Immer-blockieren-Listen führen, Listen per URL abonnieren und kuratierte
  fertige Listen mit einem Klick aktivieren.

## [0.1.0] - 2026-11-01

- Erste Version. JustDNS filtert Werbung und Schadsoftware für jedes Gerät im
  Netzwerk, mit:
  - einem Übersichts-Dashboard mit Schutzstatus, aufgelösten und geblockten
    Anfragen, aktiven Geräten und einem 24-Stunden-Aktivitätsverlauf;
  - einem Live-Protokoll, das du nach Domain oder Gerät durchsuchen kannst;
  - Einstellungen, um den Schutz an- oder auszuschalten, die Nutzung der
    Upstream-Anbieter zu wählen und zu entscheiden, ob Antworten verschlüsselt
    werden;
  - einer geführten Ersteinrichtung, die dein Administrator-Login anlegt.
