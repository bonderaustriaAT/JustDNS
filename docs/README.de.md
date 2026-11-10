# JustDNS-Handbuch

Schritt-für-Schritt-Anleitungen für alles, was JustDNS kann, in einfacher Sprache.
Eine englische Fassung dieser Seite liegt daneben: [README.md](README.md).

Jede Anleitung erklärt **was ein Feature ist**, **wie man es Klick für Klick
bedient** und — wo ein Feature technischer ist — ergänzt einen klar
gekennzeichneten Abschnitt **„Für die technisch Neugierigen“** mit Funktionsweise und
Sonderfällen, gedacht für jemanden, der sich mit Netzwerk und DNS auskennt.

Neu bei JustDNS? Beginne mit [Ersteinrichtung](guides/setup.de.md), danach
[Die Übersicht](guides/overview.de.md).

## Anleitungen

| Anleitung | Worum es geht |
| --- | --- |
| [Ersteinrichtung](guides/setup.de.md) | Der Einrichtungs-Assistent, dein Admin-Konto, das Anmelden |
| [Die Übersicht](guides/overview.de.md) | Das Live-Dashboard: Zähler, Diagramm, aktivste Clients |
| [Das Protokoll](guides/query-log.de.md) | Was gefragt wurde, von welchem Gerät, wie es beantwortet wurde |
| [DNSSEC-Validierung](guides/dnssec.de.md) | Antwort-Signaturen prüfen und manipulierte Antworten abweisen |
| [Sperr- & Erlauben-Listen](guides/lists.de.md) | Werbung/Schadsoftware blocken, Feeds abonnieren, einzelne Domains erlauben oder sperren |
| [Blockierte Dienste](guides/blocked-services.de.md) | Ganze Dienste (TikTok, YouTube, …) mit einem Schalter sperren, global oder je Ebene |
| [Block-Seite](guides/block-page.de.md) | Einem geblockten Gerät eine erklärende Seite über HTTP und HTTPS zeigen statt einer leeren Fehlermeldung |
| [Freigabe-Anfragen](guides/access-requests.de.md) | Ein geblocktes Gerät kann eine Freigabe anfragen; freigeben pro Gerät, Gruppe oder für alle |
| [Geräte](guides/devices.de.md) | Gib den Clients im Netzwerk sprechende Namen |
| [Richtlinien pro Gerät & Gruppe](guides/policies.de.md) | Gib einem Gerät oder einer Gruppe eigene Filterregeln |
| [Zeitpläne](guides/schedules.de.md) | Filterung nach wiederkehrendem Wochenplan pausieren oder verschärfen |
| [DNS-Rewrites & Adress-Objekte](guides/rewrites.de.md) | Eigene Namen aus festen Adressen beantworten |
| [Verschlüsseltes Client-DNS (DoT & DoH)](guides/encrypted-dns.de.md) | Geräte erreichen JustDNS per DNS-over-TLS und DNS-over-HTTPS |
| [Zertifikate](guides/certificates.de.md) | Der benannte TLS-Zertifikatsspeicher, aus dem DoT, DoH und die Block-Seite schöpfen |
| [Einstellungen](guides/settings.de.md) | Resolver, Tempo, Block-Antwort, Safe Search, Protokollierung und mehr |
| [Benutzer, Rollen & Zwei-Faktor](guides/users-roles-mfa.de.md) | Konten anlegen, festlegen, was jeder ändern darf, und Anmeldungen per MFA absichern |
| [System & Diagnose](guides/system-logs.de.md) | Laufzeit-Status und das laufende Diagnose-Protokoll des Servers, mit Health-Endpunkt |
| [Prometheus-Metriken](guides/metrics.de.md) | Betriebskennzahlen unter `/metrics` für ein externes Monitoring-System bereitstellen |
| [Geplante Berichte](guides/reports.de.md) | Tägliche oder wöchentliche Zusammenfassungen des DNS-Geschehens, in der App angesehen und als HTML oder CSV heruntergeladen |
| [Benachrichtigungen & Alerts](guides/notifications.de.md) | Webhook-Alerts bei Node-Ausfall, Update, ablaufendem Zertifikat oder Geräte-Spike |
| [Sicherung & Wiederherstellung](guides/backup.de.md) | Die ganze Konfiguration in eine Datei sichern und zurückholen |
| [JustDNS aktualisieren](guides/updates.de.md) | Auf GitHub prüfen und mit einem Klick updaten, mit automatischer Sicherung |
| [Hochverfügbarkeit](guides/high-availability.de.md) | Einen zweiten Knoten betreiben, der synchron bleibt, sodass beide antworten können |
| [Verschlüsselung im Ruhezustand](guides/secrets-at-rest.de.md) | Konten und geheime Einstellungen liegen automatisch verschlüsselt auf der Platte — ein geklautes Volume oder Backup ist ohne den Schlüssel wertlos |

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
