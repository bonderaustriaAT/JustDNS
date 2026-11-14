# Fahrplan

Wohin sich JustDNS entwickelt, in einfacher Sprache. Eine englische Fassung
dieser Datei liegt daneben: [ROADMAP.md](ROADMAP.md).

JustDNS ist dafür gemacht, dass es jede und jeder selbst betreiben kann
(AGPL-3.0) — die Richtung bevorzugt deshalb Funktionen, die im eigenen Netz
wirklich nützlich und robust sind. Die Reihenfolge unten ist unsere aktuelle
Absicht, kein Versprechen — sie kann sich ändern. Fertige Funktionen stehen im
[Änderungsverlauf](CHANGELOG.de.md); die neueste Version ist v0.11.0, die das
Protokoll zu einem vollen **Explorer** macht — die gesamte gespeicherte Historie nach
Quelle, Typ, Antwortcode, Resolver und Zeitraum filtern und über den Live-Verlauf
hinaus zurückblättern.

**So liest sich der Fahrplan:** ein angehaktes Kästchen (`[x]`) heißt, die
Funktion ist fertig — Details stehen im [Änderungsverlauf](CHANGELOG.de.md). Ein
offenes Kästchen (`[ ]`) ist noch geplant. Die Phasen gehen grob von bald zu
langfristig.

---

## Phase 1 — Einblick & schnelle Gewinne _(fertig)_

- [x] **Live-Statistik.** Eine Live-Rate der Anfragen (pro Sekunde oder pro
  Minute, umschaltbar), Kennzahlen der letzten 24 Stunden (Anfragen gesamt, von
  Filtern geblockt, durchschnittliche Antwortzeit) und die Top-5-Client-IPs mit
  ihrem Anteil an den Anfragen — auf der Übersicht statt auf einer eigenen Seite.
- [x] **Konfigurierbare Block-Antwort.** Wähle, was ein gesperrter Name
  zurückgibt — eine Null-IP (`0.0.0.0` / `::`, jetzt Standard) oder NXDOMAIN. Die
  optionale Sperrseite kam später, in Phase 4.
- [x] **Sichern & Wiederherstellen.** Die gesamte Konfiguration — Einstellungen,
  Listen, Abos und Rewrites — in eine einzige Datei exportieren und wieder
  einspielen. Das legt zugleich die Grundlage für den späteren Zwei-Server-Sync
  (Phase 6).
- [x] **Wiederverwendbare Adress-Objekte (Aliase).** Lege ein benanntes Objekt
  an — etwa `reverse-proxy` —, das für eine oder mehrere IP-Adressen steht, und
  zeige DNS-Rewrites darauf, statt die Adresse jedes Mal einzutippen. Änderst du
  die Adresse des Objekts einmal, werden alle Rewrites, die darauf zeigen, sofort
  angepasst — so musst du dieselbe IP nie an einem Dutzend Stellen ändern.
  Verwaltet im Abschnitt „Adressobjekte“ auf der Rewrites-Seite.
- [x] **Installierbare Web-App (PWA).** Eine mobil-optimierte, installierbare
  Version des Dashboards — zum Home-Bildschirm des Handys hinzufügen und JustDNS
  unterwegs verwalten. Vorgezogen als früher Gewinn; sie baut auf dem schon
  vorhandenen Dashboard auf, sodass vollwertige native Apps später bleiben können
  (Phase 7).

## Phase 2 — Geräte & Richtlinien _(in Arbeit)_

- [x] **Clients erkennen & benennen.** Eine Client-IP (oder einen CIDR-Bereich)
  einem Gerätenamen zuordnen und sehen, welche Geräte aktiv sind — damit Protokoll
  und Statistik in Namen statt in Zahlen sprechen. Verwaltet auf der Seite
  „Geräte“.
- [x] **Pro-Client-/Gruppen-Richtlinien.** Einem Gerät oder einer Gruppe eigene
  Sperrlisten und Erlauben/Sperren-Regeln zusätzlich zur globalen Vorgabe geben,
  wie bei AdGuard Home — der größte einzelne Schritt zu echter Haushaltskontrolle.
  Die Regeln stapeln sich (global → Gruppe → Gerät); verwaltet auf der Seite
  „Geräte“.
- [x] **Tiefere Statistik.** Durchschnittliche Antwortzeit je Resolver (welcher
  Upstream geantwortet hat und wie schnell) und die Top-Domains je Client.
- [x] **Blockierte Dienste.** Ein kuratierter Katalog, der ein ganzes Produkt
  (TikTok, YouTube, einen Spiele-Store, …) mit einem Schalter sperrt, indem er seine
  Domains bündelt — wie AdGuard Home — setzbar als globaler Standard und je Gerät,
  Gruppe oder Zeitplan, mit derselben Erben/An/Aus-Steuerung wie eine Sperrliste.
- [x] **Updates in der App.** Jede Instanz prüft auf GitHub nach der neuesten
  Version und zeigt sie im Dashboard; ein Klick legt eine Sicherung an, lädt die neue
  Version und startet neu, danach kann das alte Image entfernt werden. Getrieben von
  einem isolierten Updater-Sidecar im offiziellen Compose-File, mit einem geführten
  Befehl als Ausweg.

## Phase 3 — Zeit & Zugang _(fertig)_

- [x] **Zeitpläne.** Wiederkehrende Wochenfenster, die für alle, eine Gruppe oder
  ein einzelnes Gerät die Filterung entweder komplett pausieren oder zusätzliche
  Listen und Domains einschalten — Kinderzeiten, Fokuszeit, Schlafenszeit. Fenster
  dürfen über Mitternacht laufen und werden in einer konfigurierbaren IANA-Zeitzone
  ausgewertet (sommerzeit-korrekt).
- [x] **Benutzer & Rollen.** Mehr als ein Administrator mit drei Rollen (Admin,
  Bearbeiter, Betrachter), Zwei-Faktor-Anmeldung (TOTP + Wiederherstellungscodes),
  die pro Benutzer oder für alle verpflichtend sein kann, und ein Audit-Log, wer
  was geändert hat.

## Phase 4 — Verschlüsselung & Reichweite _(fertig)_

- [x] **Verschlüsseltes Client-DNS.** JustDNS selbst als DNS-over-TLS- und
  DNS-over-HTTPS-Endpunkt betreiben, damit sich die eigenen Geräte verschlüsselt
  verbinden — nicht nur die Upstream-Resolver. Jeder Endpunkt wählt ein Zertifikat
  aus dem Speicher (unten); die Geräte-/Gruppenregeln gelten für verschlüsselte
  Clients unverändert.
- [x] **Benannter Zertifikatsspeicher.** Ein zentraler Katalog von TLS-Zertifikaten
  unter **System → Zertifikate**: selbstsignierte erzeugen oder eigene hochladen,
  benennen, nach einer Gültigkeitsprüfung an Ort und Stelle erneuern und für DoT,
  DoH und die Block-Seite aus einem Dropdown auswählen — ohne erneuten Upload.
- [x] **Eigene Block-Seite.** Für einen geblockten Namen eine erklärende Seite über
  HTTP und HTTPS (mit einem gespeicherten Zertifikat) ausliefern, statt der stillen
  Null-IP-Antwort. Die lange versprochene Sperrseite aus Phase 1.
- [x] **DNSSEC-Validierung.** Die Signaturen auf Upstream-Antworten gegen die
  Root-Trust-Anchors prüfen und eine manipulierte oder fehlerhaft signierte Antwort
  abweisen (SERVFAIL), statt gefälschte Daten zurückzugeben. Eine DNSSEC-Seite zeigt
  das Urteil je Abfrage (gesichert, ungesichert, manipuliert, unbestimmt).
  Standardmäßig aus, eingeschaltet in der Server-Konfiguration.

## Phase 5 — Betrieb & Beobachtbarkeit _(in Arbeit)_

- [x] **System & Diagnose.** Eine **System**-Seite (nur Admin) mit dem
  Laufzeit-Status des Servers — Version und Laufzeit, welche Dienste aktiv sind,
  letzter Listen-Abgleich, Cache- und Listengrößen, aktive Geräte — über einem
  laufenden **Diagnose-Protokoll** (den betrieblichen Ereignissen des Servers),
  das sich filtern, durchsuchen und herunterladen lässt. Das Protokoll wird
  zusätzlich in eine rotierende Datei geschrieben, damit es einen Neustart
  übersteht, und ein unauthentifizierter **Health-Endpunkt** lässt einen Container
  oder Uptime-Monitor prüfen, ob JustDNS bedient.
- [x] **Prometheus-Metriken.** Ein `/metrics`-Endpunkt, der Abfrage-, Cache- und
  Listener-Kennzahlen im Prometheus-Format bereitstellt, sodass sich JustDNS in
  ein bestehendes Monitoring- und Alarmierungs-System einfügt. Token-geschützt und
  aus, bis ein Token gesetzt ist.
- [x] **Protokoll-Explorer.** Die Protokoll-Seite kann das Protokoll nach Quelle,
  Record-Typ, Antwortcode, Resolver und Zeitraum filtern und mit „Ältere laden“ durch
  die **gesamte gespeicherte Historie** blättern (nicht nur den In-Memory-Verlauf). Der
  Live-Verlauf bleibt unverändert, solange kein Filter gesetzt ist.
- [x] **Geplante Berichte.** Eine **Berichte**-Seite (nur Admin) verdichtet das Protokoll
  zu täglichen oder wöchentlichen Zusammenfassungen — Volumen, Block-Rate, mittlere
  Antwortzeit, die Aufteilung nach Quelle, DNSSEC-Urteile sowie die Top-Domains,
  geblockten Domains und Geräte — erzeugt nach Zeitplan (beide Takte standardmäßig aus)
  oder auf Abruf, in der App angesehen und als druckbares HTML oder CSV heruntergeladen.
  Die Zahlen gelten clusterweit; nichts wird extern verschickt. Siehe
  [den Leitfaden](docs/guides/reports.de.md).
- [x] **Tieferes Engine-Logging.** Der DNS-Dienst meldet seine Hintergrundarbeit ans
  Diagnose-Protokoll — Upstream-Zustandswechsel, Listen-Abgleich-Details mit
  Schrumpf-Warnung, DNSSEC-Verletzungen, Einstellungs- und Zeitplan-Änderungen — mit
  einem Laufzeit-**Ausführlichkeits-Schalter** (Normal/Debug/Trace, vorübergehend) und
  einem **Kategorie-Filter** auf der System-Seite. Siehe
  [den Leitfaden](docs/guides/system-logs.de.md).
- [x] **Benachrichtigungen & Alerts.** Aktive Alerts über einen **Webhook**
  (generisch/ntfy, Slack, Discord oder Telegram) für vier Ereignisse — eine Node fällt aus
  oder kommt zurück, eine neue Version steht bereit, ein Zertifikat läuft bald ab, ein Gerät
  wird ungewöhnlich oft geblockt (möglicher Malware-Beacon). Jede Node sendet ihr Eigenes,
  mit dem Instanznamen als Präfix; den clusterweit identischen „Update“-Alert sendet nur
  eine Node. Als Abschnitt in den Einstellungen konfiguriert, mit „Test senden“-Knopf. Siehe
  [den Leitfaden](docs/guides/notifications.de.md).
- [x] **Privatsphäre-Dashboard (Tracker-Attribution).** Eine Seite **Privatsphäre**, die die
  DNS-Anfragen jedes Geräts der **Firma** hinter dem Namen zuordnet — netzwerkweite Summen
  (Kontakte, geblockt, durchgekommen), die meistkontaktierten Firmen nach Kategorie und eine
  Aufschlüsselung je Gerät — über 24 Stunden, 7 Tage oder 30 Tage. Der Firmenkatalog ist
  kuratiert und einkompiliert, kein externer Feed. Für jeden angemeldeten Benutzer lesbar;
  node-lokal. Siehe [den Leitfaden](docs/guides/privacy-trackers.de.md).

## Phase 6 — Hochverfügbarkeit _(fertig)_

- [x] **Zwei-Server-Sync — Konfiguration (Stufe A).** Eine zweite JustDNS-Instanz
  betreiben, die sich synchron hält: Einstellungen, Listen, Rewrites, Geräte,
  Gruppen und Zeitpläne fließen in beide Richtungen zwischen zwei gleichwertigen
  Knoten (Aktiv/Aktiv, der jeweils neuere Stand gewinnt). Standardmäßig aus; du
  aktivierst sie und verbindest die Knoten über einen geführten Assistenten mit
  einem gemeinsamen Schlüssel, und die Seite zeigt den Zustand des Partners. Siehe
  [die Anleitung](docs/guides/high-availability.de.md).
- [x] **Benutzer & Logins (Stufe B).** Konten, Rollen und zweite Faktoren (TOTP und
  Wiederherstellungscodes) synchronisieren, damit ein Login — inklusive erzwungener
  MFA — auf beiden Knoten funktioniert. Sitzungen bleiben knotenlokal. Siehe
  [die Anleitung](docs/guides/high-availability.de.md).
- [x] **Bis zu vier Knoten — selbst aufbauendes Mesh.** Aus der Zwei-Knoten-Paarung
  wurde ein vollständiges Mesh aus bis zu vier gleichwertigen Knoten: Du legst die eigene
  Adresse jedes Knotens fest und lässt einen neuen Knoten irgendeinem bestehenden
  beitreten; den Rest findet er über einen gegossipten Roster und vermascht sich von
  selbst mit allen. Ein ausgefallener Knoten bricht den Abgleich der übrigen nie.
  Bestätigte MFA wird so zusammengeführt, dass ein Abgleich die Zwei-Faktor-Anmeldung
  nie still abschalten kann.
- [x] **Statistik & Protokoll-Historie (Stufe C).** Das Append-only-Query-Log wird über
  den Cluster zusammengeführt: jeder Knoten zieht die Events der anderen (inkrementell,
  per Wasserzeichen je Peer, auf das Aufbewahrungsfenster begrenzt), sodass der Explorer
  die gesamte Cluster-Historie zeigt und Lebenszeit-Zahlen einen Wechsel zwischen Knoten
  überstehen. Jede Zeile trägt ihren Herkunftsknoten; das Live-Dashboard bleibt
  knotenlokal. Die Aufbewahrungs-Einstellung wird jetzt durchgesetzt und beschneidet das
  Log auf jedem Knoten.

JustDNS automatisiert bewusst **kein** Failover (kein Knoten reißt die Adresse des
anderen an sich). Clients nutzen beide Resolver-IPs oder eine externe virtuelle IP;
der Resolver-Rückfall des Betriebssystems ist das Failover.

## Phase 7 — Sicherheits-Härtung _(in Arbeit)_

Unsichtbare, automatische Härtung — Schutz, der den Betreiber nichts kostet und keine
Aktion braucht, im Geist der HA-Peer-Verschlüsselung.

- [x] **Verschlüsselung im Ruhezustand.** Die Secret-Dateien — die Kontenliste
  (Passwort-Hashes, TOTP-Secrets, Wiederherstellungscodes) und die geheimen
  Einstellungen (Benachrichtigungs-Webhook oder Bot-Token) — liegen verschlüsselt auf
  der Platte, sodass eine kopierte Daten-Partition, ein Backup oder ein VM-Snapshot ohne
  den Schlüssel wertlos ist. Vollautomatisch: Eine bestehende Installation rüstet sich
  beim ersten Start selbst um, der Schlüssel wird einmal erzeugt und in einem eigenen
  Volume getrennt von den Daten gehalten, und das Laden ist fail-closed. Siehe
  [den Guide](docs/guides/secrets-at-rest.de.md).
- [x] **Security-Header.** Eine strenge Content-Security-Policy plus Clickjacking-Schutz,
  No-Sniff, strenge Referrer-Policy und abgeriegelte Permissions-Policy auf jeder Antwort —
  standardmäßig an, nichts einzurichten. CSRF ist bereits durch das `SameSite=Strict`-
  Sitzungs-Cookie abgedeckt. Siehe [den Leitfaden](docs/guides/browser-hardening.de.md).
- [ ] **HTTPS fürs Dashboard ab Werk.** Das Dashboard standardmäßig über TLS ausliefern
  (mit einem Zertifikat aus dem Store) und dann HSTS ergänzen.
- [ ] **Login-Härtung.** Brute-Force-Drosselung mit selbstheilender Sperre und gehärtete
  Session-Cookies, für einen normalen Login unsichtbar.
- [ ] **Signierte Images.** Die Release-Images werden signiert und der Ein-Klick-Updater
  prüft die Signatur vor dem Anwenden, sodass eine kompromittierte Registry kein
  schädliches Image unterschieben kann.

## Phase 8 — Apps _(geplant)_

- [ ] **Native Android- & iOS-Apps.** Auf der Web-App aufbauend eigene Apps für
  ein erstklassiges mobiles Erlebnis und engere Betriebssystem-Integration. Die
  installierbare Web-App (Phase 1) kommt zuerst und deckt die mobile Nutzung bis
  dahin ab.
