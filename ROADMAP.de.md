# Fahrplan

Wohin sich JustDNS entwickelt, in einfacher Sprache. Eine englische Fassung
dieser Datei liegt daneben: [ROADMAP.md](ROADMAP.md).

JustDNS ist dafür gemacht, dass es jede und jeder selbst betreiben kann
(AGPL-3.0) — die Richtung bevorzugt deshalb Funktionen, die im eigenen Netz
wirklich nützlich und robust sind. Die Reihenfolge unten ist unsere aktuelle
Absicht, kein Versprechen — sie kann sich ändern. Fertige Funktionen stehen im
[Änderungsverlauf](CHANGELOG.de.md); die neueste Version ist v0.1.24
(konfigurierbare Block-Antwort).

## Phase 1 — Einblick & schnelle Gewinne

- [ ] **Statistik-Reiter.** Eine eigene Seite mit: einer Live-Rate der Anfragen
  (pro Sekunde oder pro Minute, umschaltbar); Kennzahlen der letzten 24 Stunden
  (Anfragen gesamt, von Filtern geblockt, durchschnittliche Antwortzeit); und den
  Top-5-Client-IPs mit ihrem Anteil an den Anfragen.
- [x] **Konfigurierbare Block-Antwort.** Wähle, was ein gesperrter Name
  zurückgibt — eine Null-IP (`0.0.0.0` / `::`, jetzt Standard) oder NXDOMAIN. Eine
  kleine Sperrseite kann später noch folgen.
- [x] **Sichern & Wiederherstellen.** Die gesamte Konfiguration — Einstellungen,
  Listen, Abos und Rewrites — in eine einzige Datei exportieren und wieder
  einspielen. Das legt zugleich die Grundlage für den späteren Zwei-Server-Sync
  (Phase 5).
- [ ] **Wiederverwendbare Adress-Objekte (Aliase).** Lege ein benanntes Objekt
  an — etwa `reverse-proxy` —, das für eine IP-Adresse steht, und verwende es in
  DNS-Rewrites, statt die Adresse jedes Mal einzutippen. Änderst du die Adresse
  des Objekts einmal, werden alle Rewrites, die darauf zeigen, sofort angepasst —
  so musst du dieselbe IP nie an einem Dutzend Stellen ändern.
- [x] **Installierbare Web-App (PWA).** Eine mobil-optimierte, installierbare
  Version des Dashboards — zum Home-Bildschirm des Handys hinzufügen und JustDNS
  unterwegs verwalten. Vorgezogen als früher Gewinn; sie baut auf dem schon
  vorhandenen Dashboard auf, sodass vollwertige native Apps später bleiben können
  (Phase 6).

## Phase 2 — Geräte & Richtlinien

- [ ] **Clients erkennen & benennen.** Eine Client-IP einem Gerätenamen zuordnen
  und sehen, welche Geräte aktiv sind — damit Protokoll und Statistik in Namen
  statt in Zahlen sprechen.
- [ ] **Pro-Client-/Gruppen-Richtlinien.** Einem Gerät oder einer Gruppe eine
  eigene Schutz-Stärke und eigene Listen geben, wie bei AdGuard Home — der größte
  einzelne Schritt zu echter Haushaltskontrolle.
- [ ] **Tiefere Statistik.** Durchschnittliche Antwortzeit je Resolver (welcher
  Upstream geantwortet hat und wie schnell) und die Top-Domains je Client.

## Phase 3 — Zeit & Zugang

- [ ] **Zeitpläne.** Bestimmte Dienste oder Listen in Zeitfenstern sperren, je
  Gerät oder Gruppe — Kinderzeiten, Fokuszeit, Schlafenszeit.
- [ ] **Benutzer & Rollen.** Mehr als ein Administrator, Nur-Lese-Konten und eine
  einfache Aufzeichnung, wer was geändert hat.

## Phase 4 — Verschlüsselung & Reichweite

- [ ] **Verschlüsseltes Client-DNS.** JustDNS selbst als DNS-over-TLS- und
  DNS-over-HTTPS-Endpunkt betreiben, damit sich die eigenen Geräte verschlüsselt
  verbinden — nicht nur die Upstream-Resolver.

## Phase 5 — Hochverfügbarkeit

- [ ] **Zwei-Server-Betrieb.** Eine zweite JustDNS-Instanz betreiben, die sich
  synchron hält — Listen, Konfiguration, Rewrites und (optional) Statistiken —
  mit automatischem Failover, damit DNS weiterläuft, während ein Knoten aus ist
  oder aktualisiert wird.

## Phase 6 — Apps

- [ ] **Native Android- & iOS-Apps.** Auf der Web-App aufbauend eigene Apps für
  ein erstklassiges mobiles Erlebnis und engere Betriebssystem-Integration. Die
  installierbare Web-App (Phase 1) kommt zuerst und deckt die mobile Nutzung bis
  dahin ab.
