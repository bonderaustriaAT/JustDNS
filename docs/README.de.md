# JustDNS-Handbuch

Schritt-für-Schritt-Anleitungen für alles, was JustDNS kann, in einfacher Sprache.
Eine englische Fassung dieser Seite liegt daneben: [README.md](README.md).

Jede Anleitung erklärt **was ein Feature ist**, **wie man es Klick für Klick
bedient** und — wo ein Feature technischer ist — ergänzt einen klar
gekennzeichneten Abschnitt **„Für Technik-Interessierte“** mit Funktionsweise und
Sonderfällen, gedacht für jemanden, der sich mit Netzwerk und DNS auskennt.

Neu bei JustDNS? Beginne mit [Ersteinrichtung](guides/setup.de.md), danach
[Die Übersicht](guides/overview.de.md).

## Anleitungen

| Anleitung | Worum es geht |
| --- | --- |
| [Ersteinrichtung](guides/setup.de.md) | Der Einrichtungs-Assistent, dein Admin-Konto, das Anmelden |
| [Die Übersicht](guides/overview.de.md) | Das Live-Dashboard: Zähler, Diagramm, aktivste Clients |
| [Das Protokoll](guides/query-log.de.md) | Was gefragt wurde, von welchem Gerät, wie es beantwortet wurde |
| [Sperr- & Erlauben-Listen](guides/lists.de.md) | Werbung/Schadsoftware blocken, Feeds abonnieren, einzelne Domains erlauben oder sperren |
| [Geräte](guides/devices.de.md) | Gib den Clients im Netzwerk sprechende Namen |
| [Richtlinien pro Gerät & Gruppe](guides/policies.de.md) | Gib einem Gerät oder einer Gruppe eigene Filterregeln |
| [DNS-Rewrites & Adress-Objekte](guides/rewrites.de.md) | Eigene Namen aus festen Adressen beantworten |
| [Einstellungen](guides/settings.de.md) | Resolver, Tempo, Block-Antwort, Safe Search, Protokollierung und mehr |
| [Sicherung & Wiederherstellung](guides/backup.de.md) | Die ganze Konfiguration in eine Datei sichern und zurückholen |

## Wie die Filterung zusammenspielt

JustDNS beantwortet jede DNS-Frage, die dein Netzwerk stellt. Für jede Frage läuft
eine kurze, feste Kette ab:

1. **Lokale Rewrites** — hast du für einen Namen eine eigene Antwort hinterlegt
   (siehe [Rewrites](guides/rewrites.de.md)), wird sie sofort zurückgegeben.
2. **Safe Search** — Suchmaschinen werden auf ihre gefilterten Endpunkte gelenkt.
3. **Filterung** — der Name wird gegen die Richtlinie geprüft, die für den
   fragenden Client gilt. Eine **Freigabe** gewinnt immer über eine **Sperre**.
   Welche Listen und Regeln gelten, hängt vom Client ab (siehe
   [Richtlinien](guides/policies.de.md)); standardmäßig bekommt jeder Client die
   **globale Standard-Richtlinie** aus [Einstellungen](guides/settings.de.md) und
   [Listen](guides/lists.de.md).
4. **Cache & Upstream** — alles, was oben nicht beantwortet wurde, wird bei deinen
   Resolvern nachgefragt und die Antwort fürs nächste Mal zwischengespeichert.

Der Cache enthält nur echte Upstream-Antworten, die für alle gleich sind — deshalb
kostet die Filterung pro Gerät kein Tempo.
