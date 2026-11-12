# Änderungsverlauf

Was sich in JustDNS getan hat, in einfacher Sprache. Eine englische Fassung
dieser Datei liegt daneben: [CHANGELOG.md](CHANGELOG.md). Datumsangaben im
Format Jahr-Monat-Tag.

## [0.25.0] - 2026-11-12

- Neu: **Privatsphäre-Dashboard.** Eine neue Seite **Privatsphäre** (in der Seitenleiste,
  neben Übersicht und Protokoll) ordnet die DNS-Anfragen deiner Geräte den **Firmen** hinter
  den Namen zu – statt einer Domain-Liste siehst du in einfacher Sprache, dass ein Handy
  heute 240× Google kontaktiert hat oder ein TV ständig mit einer Analyse-Firma redet.
  Gezeigt werden netzwerkweite Summen (Tracker-Kontakte, wie viele geblockt, wie viele
  durchgekommen sind), die meistkontaktierten Firmen mit einer Farbe je Kategorie und einem
  Balken für den geblockten Anteil sowie eine Aufschlüsselung je Gerät, in die du
  hineintippen kannst, um genau zu sehen, mit welchen Firmen ein Gerät redet. Wählbar sind
  24 Stunden, 7 Tage oder 30 Tage. Der Firmenkatalog ist kuratiert und in JustDNS
  einkompiliert – er deckt die großen Werbe-, Analyse-, Social- und Fingerprinting-Netzwerke
  ab, holt keinen Feed und funkt nichts nach Hause. Für jeden angemeldeten Benutzer lesbar;
  node-lokal wie die übrigen Statistiken.

## [0.24.1] - 2026-11-11

- Neu: **Prometheus-Metrics-Token im Dashboard.** Der Scrape-Endpunkt `GET /metrics`
  braucht zum Einschalten keine Umgebungsvariable mehr. Unter _Einstellungen → Netzwerk_
  gibt es jetzt ein Feld **Metrics-Token (Prometheus)**: Token mit einem Klick generieren,
  in deine Prometheus-scrape_config kopieren, und der Endpunkt ist an; Feld leeren schaltet
  ihn aus. Das Token liegt bei den übrigen Einstellungen, verschlüsselt im Ruhezustand, und
  wird für Nicht-Admins ausgeblendet. Die alte Umgebungsvariable `JUSTDNS_METRICS_TOKEN`
  funktioniert weiterhin und greift als Fallback, solange im Dashboard kein Token gesetzt
  ist — bestehende Installationen sind also nicht betroffen.
- Geändert: **Aufgeräumtes Backup-Passwortfeld.** Die Verschlüsselungspasswort-Felder beim
  Backup-Download sitzen jetzt in einer beschrifteten, breitenbegrenzten Gruppe mit kleinem
  Schloss-Icon und kürzerem Hilfetext, statt sich über die ganze Zeile zu ziehen.

## [0.24.0] - 2026-11-10

- Neu: **Verschlüsselung im Ruhezustand.** Die Dateien mit JustDNS' Geheimnissen — die
  Kontenliste (Passwort-Hashes, TOTP-Secrets, Wiederherstellungscodes) und die geheimen
  Teile deiner Einstellungen (Benachrichtigungs-Webhook oder Bot-Token) — liegen jetzt
  **verschlüsselt auf der Platte**, sodass eine kopierte Daten-Partition, ein Backup oder
  ein VM-Snapshot ohne den Schlüssel wertlos ist. Es ist vollautomatisch: Es gibt nichts
  einzuschalten, und eine bestehende Installation rüstet sich beim ersten Start selbst um
  (sie liest die alten Klartext-Dateien und schreibt sie versiegelt zurück). Der Schlüssel
  ist ein Zufallswert, beim ersten Start erzeugt und in einem **eigenen Volume** (`/keys`)
  gehalten, getrennt von den Daten, damit ein geklautes Daten-Volume ihn nicht
  mitentschlüsselt; die offizielle `docker compose`-Installation legt dieses Volume
  automatisch an. Jede Datei wird mit ChaCha20-Poly1305 unter einem je Datei aus dem
  Masterschlüssel abgeleiteten (HKDF-SHA256) Teilschlüssel versiegelt, und das Laden ist
  fail-closed — ein fehlender oder falscher Schlüssel stoppt den Knoten mit einem klaren
  Fehler, statt still ohne Konten zu starten. Das schützt Daten im Ruhezustand, nicht
  einen voll kompromittierten laufenden Host (der bewusste Preis dafür, ohne Passwort beim
  Start auszukommen). Neuer Guide: _Verschlüsselung im Ruhezustand_ (DE + EN). **Sichere
  das Schlüssel-Volume getrennt — bei Verlust sind die versiegelten Dateien unlesbar.**
- Neu: **Passwortverschlüsselte Sicherungen.** Eine Konfigurationssicherung ist keine
  Klartextdatei mehr. Beim Herunterladen legst du jetzt ein **Verschlüsselungspasswort**
  fest (zweimal eingegeben), und die Datei wird damit versiegelt — Argon2id, um das
  Passwort zu strecken, ChaCha20-Poly1305 zum Verschlüsseln — sodass eine Sicherung, die
  weiterhin Einstellungen wie Resolver-Adressen oder eine Webhook-URL enthält, nie im
  Klartext lesbar ist. Dasselbe Passwort schützt die Sicherung, die das Ein-Klick-**Update**
  vor dem Start anlegt, sodass auch die Vor-Update-Kopie auf der Platte versiegelt ist. Die
  Wiederherstellung fragt nach dem Passwort und meldet ein falsches klar; ältere
  unverschlüsselte Sicherungen lassen sich weiterhin zurückspielen (Passwort leer lassen).
  **Behalte das Passwort — es gibt keine Wiederherstellung, und ohne es lässt sich eine
  Sicherung nicht zurückspielen.**

## [0.23.0] - 2026-11-08

- Neu: **Benachrichtigungen & Alerts.** JustDNS kann sich jetzt über einen **Webhook** bei
  dir melden, wenn im Cluster etwas Aufmerksamkeit braucht, statt darauf zu warten, dass du
  es im Dashboard bemerkst. Wähle ein Format — generisch/ntfy, Slack, Discord oder
  Telegram — und schalte beliebige der vier Ereignisse ein: eine **Node fällt aus** (und
  kommt zurück), eine neue **Version verfügbar**, ein **Zertifikat läuft bald ab** und ein
  **Gerät wird ungewöhnlich oft geblockt** im Zeitfenster (klassisches Malware-Beacon-
  Zeichen; standardmäßig aus, mit einstellbarer Schwelle und Fenster). Jede Node sendet ihre
  eigenen Alerts, mit dem Instanznamen als Präfix; den clusterweit identischen
  „Update verfügbar“-Alert sendet nur eine Node, damit du ihn nicht doppelt bekommst.
  Konfiguriert als neuer Abschnitt **Einstellungen → Benachrichtigungen**, mit einem Knopf
  **Test senden**, der eine einzelne Nachricht verschickt, damit du den Kanal prüfen kannst.
  Webhook-URL und Telegram-Token werden wie die übrige Konfiguration gespeichert und im UI
  maskiert. Neuer Leitfaden: _Benachrichtigungen & Alerts_ (DE + EN).

## [0.22.2] - 2026-11-08

- Behoben: **Der Schalter „Anfrage-Protokoll" wirkt jetzt wirklich.** Der Umschalter unter
  Einstellungen wurde angezeigt und gespeichert, aber nie ausgewertet — Anfragen wurden
  immer aufgezeichnet. Jetzt ist es ein echter Schalter an der Aufzeichnung: Ausgeschaltet
  schreibt diese Node keine neuen Zeilen mehr, Live-Protokoll, Abfrage-Explorer und
  Dashboard-Statistik bekommen keine neuen Einträge (Gespeichertes bleibt bis zur
  Aufbewahrungsgrenze); wieder an, läuft die Aufzeichnung ab der nächsten Anfrage weiter —
  ohne Neustart.
- Geändert: **Das Protokoll sagt, wessen Anfragen es zeigt.** Eine kurze Notiz unter dem
  Titel stellt klar, dass die **Live**-Ansicht nur den Verkehr dieser Node zeigt, während
  das **Filtern** die volle Historie liest — die im Cluster über jede Node spannt, jede
  Zeile mit dem Abzeichen der Node, die geantwortet hat. Das klärt „warum ist die geblockte
  Anfrage meines Geräts nicht hier?", wenn das Gerät eine andere Node für DNS nutzt. Der
  Guide hat einen passenden Abschnitt bekommen.

## [0.22.1] - 2026-11-08

- Behoben: **Ein Cluster schaltet die Block-Seite nicht mehr von selbst ab.** Der
  An/Aus-Schalter der Block-Seite (samt Text, Modus und Adresse) hing bisher in dem einen
  Einstellungs-Datensatz, den die Knoten synchronisieren. Dadurch konnte eine *beliebige*
  fremde Einstellungsänderung auf einem Knoten einen älteren Stand „Block-Seite aus" auf
  den anderen tragen — die Listener kamen beim Start hoch und gingen wenige Sekunden
  später, nach dem ersten Sync, wieder aus. Die Block-Seite wird jetzt als eigener
  Datensatz mit eigenem Zeitstempel synchronisiert und ändert sich clusterweit nur noch,
  wenn du die Block-Seite wirklich änderst. Jeder Knoten behält weiter seine eigene
  Server-IP.

## [0.22.0] - 2026-11-08

- Neu: **„Seite freigeben lassen" auf der Block-Seite.** Wenn jemand auf einer gesperrten
  Seite landet, bietet die Block-Seite jetzt einen Knopf, um Zugang zu erbitten — mit
  optionaler Begründung — statt ihn nur zu stoppen. Jede Anfrage landet in der neuen
  Warteschlange unter **System → Freigaben** (nur Admin, mit Zähler-Zeichen) und zeigt,
  welches Gerät gefragt hat, wann, welche Seite und *wodurch* sie blockiert wurde (die
  passende Liste oder Kategorie). Gib die Seite fürs einzelne Gerät, die ganze Gruppe oder
  für alle frei (eine dauerhafte globale Erlaubnis, vorher bestätigt), oder lehne die
  Anfrage ab. Eine Erlaubnis gewinnt vor einer Sperre, also löst eine freigegebene Seite ab
  der nächsten Anfrage wieder normal auf. Im Cluster ist die Warteschlange geteilt, du gibst
  also einmal auf einer beliebigen Node frei. Neue Anleitung: _Freigabe-Anfragen_ (DE + EN).
- Behoben: **Eine geblockte Domain ohne brauchbare Block-Seiten-IP antwortet nicht mehr mit
  `0.0.0.0`.** Diese Antwort ließ Browser `ERR_ADDRESS_INVALID` zeigen; ist die Block-Seite
  an, aber ihre Server-IP leer oder ungültig, liefert die A-Anfrage jetzt NODATA (keine
  Adresse) — genau wie bei AAAA.
- Geändert: **Die Server-IPs der Block-Seite tragen sich selbst ein.** Schaltest du die
  Block-Seite ohne Server-IPv4/-IPv6 an, leitet JustDNS sie jetzt aus der eigenen
  Cluster-Adresse der Node ab, sodass ein geblockter Name ohne manuelles Eintippen auf die
  richtige Node zeigt (ein falscher Wert — etwa eine eingefügte URL — war die übliche Ursache
  für `ERR_ADDRESS_INVALID`).

## [0.21.7] - 2026-11-08

- Behoben: **Geblockte HTTPS-Domains erreichen hinter einem Reverse-Proxy wieder die
  Sperrseite.** Im Reverse-Proxy-Modus hörte die Node nicht mehr auf Port 443 — ein
  geblockte Seite über `https://` endete in „Verbindung abgelehnt" statt in der
  Weiterleitung, weil geblockte Domains direkt auf die Node zeigen und den Proxy umgehen.
  Die Node bedient jetzt **immer** Port 80 **und** 443, solange die Sperrseite an ist; der
  Reverse-Proxy-Schalter steuert nur noch, ob dem `X-Forwarded-Host`-Header vertraut wird.
  Die unvermeidbare Zertifikatswarnung bei einer geblockten `https://`-Domain bleibt gleich
  (sie erscheint in jedem Modus); die Landing-Seite selbst liefert der Proxy weiterhin mit
  sauberem Zertifikat.

## [0.21.6] - 2026-11-09

- Geändert: **Der Reverse-Proxy ist jetzt der empfohlene Block-Seiten-Modus**, und seine
  Anleitung ist klarer. Die Modus-Liste in den Einstellungen zeigt _Hinter eigenem
  Reverse-Proxy_ zuerst und als empfohlen. Die Anleitung nutzt eine eigene Subdomain
  (`block.justfuture.at`), stellt klar, dass der Pfad egal ist (der Proxy reicht einfach den
  ganzen Host weiter — kein `/blockpage`-Routing nötig), warnt davor, den Admin-UI-Host
  mitzubenutzen, und ergänzt einen Weg „bestehenden Host mitbenutzen" über eine NGINX-Proxy-
  Manager-Custom-Location.
- Neu: **Ein Test sichert ab, dass die Block-Seiten-Einstellungen im Cluster synchronisieren.**
  Adresse der Sperrseite, Modus, An/Aus-Schalter, Überschrift und Text werden über HA-Knoten
  geteilt; nur die knotenlokale Server-IP bleibt lokal (jeder Knoten zeigt geblockte Namen auf
  sich selbst). Das war schon so — der Test hält es ab jetzt fest.

## [0.21.5] - 2026-11-09

- Geändert: **Der Ausführlichkeits-Umschalter reagiert sofort, und der Log zeigt, dass er
  arbeitet.** Wählst du Normal / Debug / Trace, wird die gewählte Stufe jetzt sofort
  hervorgehoben, statt auf die nächste Status-Abfrage zu warten; während der Dienst sie
  übernimmt, zeigt das Diagnose-Protokoll einen kleinen Spinner und einen dünnen Ladebalken,
  damit sichtbar ist, dass etwas passiert. Schlägt die Änderung fehl, springt der Umschalter
  mit der Fehlermeldung auf die aktive Stufe zurück.
- Geändert: **Klarere Hilfe zum Reverse-Proxy-Modus.** Der Reverse-Proxy ist nur der
  TLS-Vorbau: Die JustDNS-Node liefert die Seite weiterhin selbst, der Proxy reicht an dieselbe
  Node weiter und stellt nur das Zertifikat bereit. Einstellungs-Hilfe und Block-Seiten-Guide
  sagen das jetzt deutlich — geblockte Namen zeigen weiter auf die Node (Server-IPv4), der
  Proxy sitzt nur vor dem Landing-Hostnamen.

## [0.21.4] - 2026-11-09

- Neu: **Ein neuer Block-Seiten-Modus — _Hinter eigenem Reverse-Proxy_.** Betreibst du
  schon NGINX Proxy Manager, Traefik oder einen anderen Reverse-Proxy, darf dieser HTTPS
  terminieren und per HTTP an JustDNS weiterreichen; JustDNS verwaltet dann kein eigenes
  Zertifikat mehr. Modus einschalten, die öffentliche Block-Seiten-Adresse und die IP
  dieser Box eintragen — die Einstellungsseite zeigt genau, was am Proxy einzurichten ist
  (inklusive des einen `X-Forwarded-Host`-Headers, den NGINX Proxy Manager braucht). Die
  Block-Seiten-Anleitung enthält eine Schritt-für-Schritt-NPM-Konfiguration.
- Behoben: **Die gehostete Block-Seite konnte über HTTP/2 in einer Schleife weiterleiten.**
  Eine HTTP/2-Anfrage trägt keinen `Host`-Header — der Hostname steht im `:authority`-Feld
  —, der Server las also einen leeren Host, traf nie den Landing-Host und leitete endlos
  weiter. Er liest jetzt die Authority (und hinter einem vertrauten Reverse-Proxy
  `X-Forwarded-Host`), sodass die Landing-Seite ausgeliefert wird statt zu schleifen.

## [0.21.3] - 2026-11-09

- Behoben: **Eine geblockte Seite zeigte „Website ist nicht erreichbar" statt der
  Block-Seite.** War für die Block-Seite keine IPv6 gesetzt, beantwortete JustDNS die
  AAAA-Anfrage (IPv6) einer geblockten Domain mit der Nulladresse `::`. Dual-Stack-Browser
  bevorzugen IPv6 und lehnen `::` sofort ab (`ERR_ADDRESS_INVALID`), statt auf die
  IPv4-Block-Seite auszuweichen — die Seite blieb also unsichtbar, selbst bei korrekter
  IPv4. Eine geblockte AAAA-Anfrage ohne konfigurierte Block-Seiten-IPv6 bleibt jetzt leer
  (NODATA), damit der Browser die IPv4-Block-Seite nutzt. Hinweis: Die **Server-IPv4** der
  Block-Seite muss die Adresse des JustDNS-Geräts selbst sein (dort sind die Ports 80/443
  veröffentlicht), nicht die eines anderen Hosts.

## [0.21.2] - 2026-11-09

- Behoben: **Hochgeladene Zertifikate gingen bei jedem Update verloren.** Der
  Zertifikatsspeicher lag standardmäßig außerhalb des `/data`-Volumes, sodass alles außer
  dem eingebauten selbstsignierten Zertifikat beim Neu-Erzeugen des Containers verschwand.
  Der Speicher liegt jetzt unter `/data/certs` und übersteht Updates und Neustarts. Nach dem
  Update die Zertifikate einmalig neu anlegen — danach bleiben sie erhalten.

## [0.21.1] - 2026-11-09

- Behoben: **Das v0.21.0-Image ließ sich nicht bauen** — ein verirrtes Anführungszeichen
  in den deutschen UI-Texten brach die Typprüfung. Diese Version baut und veröffentlicht
  sauber.
- Geändert: **Die Block-Seiten-Ports werden beim Update jetzt automatisch veröffentlicht.**
  Der Updater-Sidecar stellt die Ports 80/443 sicher — über eine kleine, klar
  gekennzeichnete Compose-Override — sodass die Sperrseite nach einem Update erreichbar ist,
  ohne am Host etwas zu ändern. Eine Installation mit dem alten Updater vor 0.21.1 muss den
  Sidecar einmalig auffrischen; siehe Block-Seiten-Guide.

## [0.21.0] - 2026-11-09

- Geändert: **Die Block-Seite ist viel einfacher einzurichten — und sie lädt jetzt
  wirklich.** Die Einstellungen beginnen nun mit einem **Modus**-Schalter: *Auf eigener
  Domain hosten (empfohlen)* oder *Direkt auf dieser Box anzeigen*. Im Hosted-Modus leitet
  JustDNS eine geblockte Domain auf deinen eigenen Hostnamen weiter — z. B.
  `https://justdns.justfuture.at/blockpage` — mit der geblockten Domain als `?d=` angehängt,
  und liefert die Seite dort mit einem für deinen Namen gültigen Zertifikat aus. So lädt sie
  über HTTPS **ohne Warnung** und kann anzeigen, welche Domain gesperrt wurde. Hinweise im
  Formular erklären die einmaligen Rewrite- und Zertifikats-Schritte.
- Behoben: **„Verbindung abgelehnt", wenn die Block-Seite an war.** Das offizielle
  Docker-Compose veröffentlicht jetzt die Ports **80** und **443**, sodass eine geblockte
  Domain die Seite tatsächlich erreicht statt einer abgelehnten Verbindung. Auf deinem Host
  die zwei Port-Zeilen einmalig nachtragen und den Dienst neu erzeugen — oder den Resolver
  mit `network_mode: host` betreiben.

## [0.20.0] - 2026-11-09

- Neu: **Zertifikate werden jetzt über einen Hochverfügbarkeits-Cluster
  synchronisiert.** Der ganze Zertifikatsspeicher — inklusive privater Schlüssel —
  reist über den authentifizierten Abgleich zwischen deinen Knoten, sodass ein
  Zertifikat, das du auf einem Knoten anlegst, hochlädst oder erneuerst, auf allen
  vorhanden ist und eine Speicher-Id überall dasselbe Zertifikat meint. Die
  Zertifikatsauswahl der Block-Seite wird mitgeteilt; die DoT/DoH-Auswahl und die
  Block-Seiten-Server-IP bleiben knotenlokal (jeder Knoten bedient seinen eigenen
  Hostnamen und zeigt geblockte Namen auf sich selbst). Schlüssel verlassen den Cluster
  nie.

## [0.19.1] - 2026-11-09

- Neu: **Die Block-Seite kann weiterleiten, statt selbst auszuliefern.** Trage unter
  **Einstellungen → Block-Seite** eine **Weiterleitungs-URL** ein, und JustDNS beantwortet
  geblockte Anfragen mit einem HTTP 302 dorthin — so hostest du die Erklärseite unter
  einem echten Hostnamen mit gültigem Zertifikat (per DNS-Rewrite auf JustDNS oder hinter
  einem Reverse-Proxy) und musst hier gar kein Zertifikat verwalten.
- Geändert: **Neues Aussehen der Block-Seite.** Die ausgelieferte Seite ist aufgeräumter —
  ein ruhiges dunkles Layout mit pulsierendem Status-Punkt, dem geblockten Host als
  Mono-Chip und einer Zeile „geschützt durch JustDNS".
- Behoben: **Lange Sidebar-Einträge werden nicht mehr abgeschnitten.** Namen wie
  „System & Diagnose" und „Hochverfügbarkeit" brechen jetzt um, statt mit „…" gekürzt zu
  werden.

## [0.19.0] - 2026-11-08

- Neu: **Eine eigene Block-Seite.** Statt dass ein geblockter Name still scheitert, kann
  JustDNS jetzt eine Seite ausliefern, die erklärt, *dass diese Seite gesperrt ist* —
  über HTTP und, mit einem gespeicherten Zertifikat, über HTTPS. Schalte sie unter
  **Einstellungen → Block-Seite** ein, setze die Server-IP, auf die geblockte Namen
  zeigen sollen, wähle ein Zertifikat und schreibe eigene Überschrift und Text. (Über
  HTTPS ist eine Zertifikatswarnung für die geblockte Domain unvermeidbar, ohne die
  Verbindung aufzubrechen — das tut JustDNS nie; über reines HTTP erscheint die Seite
  ohne Warnung. Die Block-Seite nutzt die Ports 80 und 443 des Hosts, die dein Compose
  veröffentlichen muss.)
- Neu: **Ein zentraler Zertifikatsspeicher.** Zertifikate liegen jetzt an einer Stelle
  unter **System → Zertifikate**: gib jedem einen Namen, **erzeuge** ein
  selbstsigniertes oder **lade** dein eigenes **hoch** und wähle es dann für DoT, DoH
  *und* die Block-Seite aus einem Dropdown — ohne erneuten Upload. Ein
  **Erneuern**-Knopf ersetzt ein Zertifikat an Ort und Stelle, nachdem geprüft wurde,
  dass der Ersatz aktuell gültig ist, sodass jede nutzende Funktion weiterarbeitet. Dein
  bestehendes Zertifikat wird automatisch als **Standard** übernommen.
- Geändert: **DoT und DoH wählen jetzt ein Zertifikat aus dem Speicher** statt eines
  eigenen Selbstsigniert/Hochgeladen-Umschalters. Beide starten auf **Standard**, es
  ändert sich also nichts, bis du etwas anderes wählst.
- Geändert: **Die Seitenleiste ist aufgeräumter.** Die Navigation ist in einklappbare
  Gruppen **Regeln** und **System** gegliedert, damit die Liste kurz bleibt; die Gruppe
  mit deiner aktuellen Seite klappt von selbst auf.

## [0.18.0] - 2026-11-08

- Neu: **Die System-Seite zeigt, was der DNS-Dienst wirklich tut.** Betriebliche
  Ereignisse, die früher unsichtbar waren, landen jetzt im Diagnose-Protokoll: ein
  Upstream-Resolver, der ungesund wird und sich erholt (einmal pro Wechsel, nicht pro
  Abfrage), jeder Listen-Abgleich mit Eintragszahl und Download-Dauer, eine **Warnung**,
  wenn ein Feed plötzlich die meisten Einträge verliert (wahrscheinlich kaputte Quelle),
  eine DNSSEC-Verletzung, und wenn Einstellungen oder ein Zeitplan-Fenster greifen.
- Neu: **Ein Ausführlichkeits-Schalter auf der System-Seite.** Hebe die Live-Stufe auf
  *Debug* oder *Trace*, um ein Problem live zu beobachten – ohne Neustart – und stell
  danach wieder auf *Normal*. Er ist bewusst vorübergehend: Ein Neustart kehrt zur
  Normalstufe zurück, damit eine vergessene Debug-Sitzung nicht die Platte vollschreibt.
- Neu: **Das Diagnose-Protokoll nach Kategorie filtern** (DNS & Upstreams, Listen,
  Web & HA, …), um ein Teilsystem zu beobachten, ohne dass der Rest vorbeiscrollt.

## [0.17.0] - 2026-11-08

- Neu: **Einen Cluster-Knoten entfernen klappt sauber und bleibt.** Einen Partner
  entfernen (das × auf seiner Karte) oder den Cluster verlassen fragt jetzt zuerst nach
  und benachrichtigt dann die anderen Knoten — der entfernte Knoten verschwindet
  mesh-weit, statt als unerreichbare „Leiche" liegenzubleiben, die jeder Knoten weiter
  anzusprechen versucht. Technisch ist ein Entfernen jetzt ein Roster-Tombstone, der mit
  dem Abgleich reist, sodass die Kopie eines anderen Knotens ihn nicht wieder aufwecken
  kann; einen versehentlich entfernten Knoten holst du durch erneutes Verbinden zurück.
- Behoben: **Die App aktualisiert auf die neue Version statt eine alte zu zeigen.** Die
  Web-App — vor allem als Handy-App auf dem Startbildschirm — konnte eine monatealte
  Version anzeigen, weil die App-Shell und ihr Service Worker unbegrenzt gecacht wurden.
  Sie werden jetzt immer neu geprüft, während die inhaltsgehashten Dateien gecacht
  bleiben — eine neue Version wird beim nächsten Aufruf übernommen. Nach dem einmaligen
  Update auf diese Version die App am Handy einmal neu laden (oder entfernen und neu
  hinzufügen), um den alten Cache zu leeren; spätere Updates erscheinen dann von selbst.

## [0.16.1] - 2026-11-07

- Behoben: **Keine „could not persist HA state"-Meldung mehr auf Installationen ohne
  gesetztes Datenverzeichnis.** Die Hochverfügbarkeit war der einzige Speicher, dessen
  Datei standardmäßig unter `/data` statt im Arbeitsverzeichnis lag; eine Installation
  ohne `JUSTDNS_DATA_DIR` (ein lokaler oder Nicht-Docker-Lauf) meldete daher bei jedem
  Speichern „No such file or directory". Der Standard ist jetzt das Arbeitsverzeichnis wie
  bei den anderen Speichern; das Docker-Image setzt `JUSTDNS_DATA_DIR=/data` explizit, ein
  normales Deployment ist also nicht betroffen.

## [0.16.0] - 2026-11-07

- Neu: **Geplante Berichte.** Eine neue Seite *Berichte* (nur Admin) verdichtet das
  Protokoll zu lesbaren täglichen oder wöchentlichen Zusammenfassungen — Anfragen gesamt,
  Block-Rate, mittlere Antwortzeit, die Aufteilung nach Quelle, DNSSEC-Urteile sowie die
  Top-Domains, geblockten Domains und Geräte. Beide Takte sind standardmäßig aus und
  konfigurierbar (Uhrzeit, beim Wochen-Report zusätzlich Wochentag), in der Zeitzone aus
  den Einstellungen. Die Zahlen gelten clusterweit, wie der Protokoll-Explorer. Berichte
  liegen auf der Datenplatte, werden in der App angesehen, per *Jetzt erzeugen* sofort
  erstellt und als druckbares HTML oder CSV heruntergeladen — verschickt wird nichts.
- Neu: **Das Protokoll zeigt per Hostname, welcher Server geantwortet hat.** In einem
  Hochverfügbarkeits-Verbund liest sich das Knoten-Abzeichen jeder Zeile nun als Hostname
  des antwortenden Knotens (der Host-Teil seiner Adresse) statt als kryptisches
  Knoten-ID-Präfix; die Zeilen des eigenen Knotens sind gedämpft und als dieser Knoten
  markiert. Auf einer Einzelinstanz bleibt das Abzeichen verborgen.
- Geändert: **Die Benutzer-Seite macht die Zwei-Faktor-Pflicht klarer.** Ein Banner
  *Zwei-Faktor für alle Konten verpflichtend* sitzt nun oben auf der Seite für die globale
  Pflicht, und die Pflicht je Konto ist in eine eigene Spalte *Pflicht* gewandert; solange
  die globale Pflicht an ist, zeigt der Schalter je Konto *Erzwungen*.

## [0.15.1] - 2026-11-07

- Neu: **Der Protokoll-Explorer kann nach Gerät oder Gruppe filtern.** Ein neuer Filter
  *Gerät / Gruppe* grenzt das Protokoll auf ein einzelnes benanntes Gerät oder eine ganze
  Gruppe ein. Ein als Adressbereich (CIDR) definiertes Gerät trifft jede beobachtete
  Adresse in seinem Bereich — nach derselben „spezifischstes Gerät gewinnt“-Zuordnung wie
  in der Gerätespalte des Protokolls.
- Behoben: **Die Verlaufsansicht zeigt jetzt das Datum neben der Uhrzeit.** Sobald ein
  Filter gesetzt ist und der Explorer die gesamte gespeicherte Historie liest, reichen
  die Einträge über mehrere Tage; die Zeit-Spalte trägt daher nun auch das Datum. Der
  Live-Verlauf bleibt reine Uhrzeit.

## [0.15.0] - 2026-11-07

- Neu: **Hochverfügbarkeit synchronisiert jetzt Protokoll und Statistik über den Cluster
  (Stufe C).** Jeder Knoten zieht die Protokolleinträge der anderen Knoten, sodass der
  *Protokoll*-Explorer auf jedem Knoten den gesamten Cluster-Verkehr über das
  Aufbewahrungsfenster zeigt und die Lebenszeit-Statistik nach einem Schwenk auf einen
  anderen Knoten nicht mehr bei null steht. Jede Protokollzeile trägt ein kleines
  Knoten-Badge, das zeigt, von welchem Knoten sie stammt. Nur der Explorer und die
  Gesamtzähler vereinigen sich — das Live-Dashboard (jüngste Anfragen, heutige
  Aktivität, Geräte- und Resolver-Ansichten) bleibt knotenlokal.
- Neu: **Ein geführter Assistent „Weiteren Knoten hinzufügen" für den dritten und
  vierten Knoten.** Die Knotenliste führt dich jetzt Schritt für Schritt durch das
  Hinzufügen — HA einschalten, denselben Schlüssel setzen, eine Seed-Adresse eintragen —
  mit einem Live-Knotenzähler und einem Hinweis, wenn der Cluster bei vier voll ist.
- Neu: **Der gemeinsame Schlüssel lässt sich zum Kopieren an einen neuen Knoten
  anzeigen.** Ein Admin kann den Schlüssel auf der HA-Seite anzeigen und kopieren (ein
  per Umgebungsvariable gepinnter Schlüssel wird nie angezeigt und bleibt außerhalb der
  App verwaltet). Das Anzeigen wird im Audit-Log vermerkt.
- Geändert: **Die Protokoll-Aufbewahrung wird jetzt durchgesetzt.** *Einstellungen →
  Protokoll → Aufbewahrung* (standardmäßig sieben Tage) war bisher wirkungslos — das Log
  wuchs unbegrenzt. Es werden nun tatsächlich ältere Einträge auf jedem Knoten gelöscht,
  und dasselbe Fenster begrenzt, was die Hochverfügbarkeit synchronisiert. Wenn du mehr
  als die eingestellte Zahl an Tagen aufbewahrt hast, erhöhe die Aufbewahrung vor dem
  Update, um sie zu behalten.
- Behoben: **Die Version eines Partners wird nach dessen Update korrekt angezeigt.** Die
  Knotenliste fror die Version jedes Partners im Moment der Paarung ein, sodass ein
  Knoten, der ohne Neu-Paarung aktualisierte, weiter seine alte Version zeigte. Die
  Version wird jetzt in jedem Sync-Zyklus aufgefrischt.

## [0.14.0] - 2026-11-07

- Neu: **Hochverfügbarkeit umfasst jetzt bis zu vier Knoten — als selbst aufbauendes
  Mesh.** Statt zwei Knoten zu paaren, legst du die eigene Adresse jedes Knotens fest und
  lässt einen neuen Knoten irgendeinem bestehenden beitreten; den Rest findet er über
  einen gegossipten Roster und vermascht sich von selbst mit allen. Jeder Knoten gleicht
  direkt mit jedem anderen ab, sodass ein ausgefallener Knoten den Abgleich der übrigen
  nie unterbricht und ein zurückkehrender Knoten von allein aufholt. Die HA-Seite wird zur
  Knotenliste — jeder Knoten mit Zustand, Adresse, Version und letztem Abgleich — mit
  Entfernen je Knoten und einem „Cluster verlassen".
- Behoben: **Ein HA-Abgleich kann die Zwei-Faktor-Anmeldung nicht mehr abschalten.** Als
  die Knoten erstmals Benutzer synchronisierten, wurde ein Konto als Ganzes per
  „neuester gewinnt" zusammengeführt — ein Knoten, dessen Kopie einen späteren Zeitstempel
  trug, aber keine MFA, konnte so allen die MFA nehmen. Bestätigte MFA wird jetzt eigens
  zusammengeführt: Hat einer der Knoten sie für ein Konto, behalten beide sie und
  konvergieren auf „MFA aktiv". Richte die MFA einmal auf dem Knoten neu ein, der sie
  haben soll — sie überträgt sich dann und bleibt. MFA entfernen ist jetzt bewusst pro
  Knoten — auf jedem Knoten löschen (siehe HA-Anleitung).

## [0.13.1] - 2026-11-07

- Behoben: **Der Name eines Knotens wandert nicht mehr über den HA-Verbund.** Der
  Anzeigename der Instanz und der DoT/DoH-Zertifikats-Hostname steckten mit in den
  synchronisierten Einstellungen — eine Änderung auf einem Knoten überschrieb den
  anderen, und beide trugen am Ende denselben Namen. Beide gelten jetzt als knotenlokal
  und werden nie mehr mitsynchronisiert. Benenne nach dem Update jeden Knoten einmal
  neu; danach bleiben die Namen stabil.

## [0.13.0] - 2026-11-07

- Neu: **Hochverfügbarkeit synchronisiert jetzt auch Benutzer und Logins (Stufe B).**
  In einem HA-Verbund wandern alle Konten mit Rolle, Passwort und Zwei-Faktor
  (TOTP-Secret und Wiederherstellungscodes) auf beide Knoten — dieselben Logins und
  dieselbe erzwungene MFA gelten überall. Bei Namensgleichheit entscheidet beim ersten
  Zusammenführen deine Gewinner-Wahl; danach gewinnt die neuere Änderung. Login-Sitzungen
  bleiben bewusst knotenlokal (du meldest dich an jedem Knoten separat an).
- Sicherheit: **Der Sync-Schlüssel ist jetzt so heikel wie die Passwort-Datenbank.**
  Der Snapshot trägt ab sofort Passwort-Hashes und TOTP-Secrets; der Assistent verlangt
  darum einen Schlüssel von mindestens 16 Zeichen, und die Anleitung empfiehlt, nur über
  TLS zu paaren und bei Verdacht auf einen offengelegten Schlüssel alle Passwörter neu
  zu setzen.

## [0.12.1] - 2026-11-07

- Behoben: **Die HA-Paarung ließ sich nicht herstellen.** Der Knoten-Handshake
  (`/api/ha/hello`) verlangte eine bereits bestehende Paarung — beim allerersten
  Verbinden, wenn noch kein Knoten einen Partner hat, scheiterte er deshalb immer
  mit 404. Der Handshake antwortet jetzt, sobald die Hochverfügbarkeit aktiv und
  der gemeinsame Schlüssel gesetzt ist (der Schlüssel ist die eigentliche
  Absicherung), sodass sich zwei frische Knoten verbinden können.
- Geändert: **Der Verbinden-Schritt zeigt jetzt die eigene Adresse zum Kopieren.**
  Unter *Partner verbinden* steht die Adresse dieser Instanz mit Kopier-Knopf — so
  tauschst du nur zwei URLs über Kreuz: jede Knotenadresse ins Partnerfeld des anderen.

## [0.12.0] - 2026-11-07

- Neu: **Hochverfügbarkeit — zwei Server synchron halten (Stufe A).** Betreibe einen
  zweiten JustDNS, der mit dem ersten synchron bleibt, sodass jeder antworten kann und
  du einen zum Aktualisieren oder Neustarten abschalten kannst, ohne dass das Netz es
  merkt. Beide Knoten sind gleichwertig — beide beschreibbar, Änderungen fließen in
  beide Richtungen und werden zusammengeführt (die neuere gewinnt) —, es gibt also
  keinen festen Primary. Eine neue Admin-Seite, *Hochverfügbarkeit*, führt dich
  durch: einschalten, gemeinsamen Schlüssel setzen und den Partner über seine Adresse
  verbinden. Beim ersten Verbinden zweier getrennt eingerichteter Knoten triffst du
  eine Wahl — bei echten Konflikten gewinnt dieser Knoten oder der Partner —, und
  alles, was nur auf einer Seite existiert, bleibt immer erhalten. Diese Stufe
  synchronisiert Einstellungen, Listen, Rewrites, Geräte, Gruppen und Zeitpläne;
  Benutzer und Statistiken folgen in späteren Stufen. Sie ist **standardmäßig aus**
  und tut nichts, bis du sie aktivierst und paarst. JustDNS gleicht ab und zeigt den
  Zustand des Partners, schwenkt den Verkehr aber nicht selbst um — trage bei deinen
  Clients beide Knoten ein (zwei Resolver-IPs oder eine externe virtuelle IP). Siehe
  die neue [Anleitung zur Hochverfügbarkeit](docs/guides/high-availability.de.md).

## [0.11.0] - 2026-11-07

- Neu: **Das gesamte Abfrageprotokoll filtern und durchsuchen.** Die *Protokoll*-Seite
  bekommt eine Filterschiene: das Protokoll nach Quelle (Geblockt, Cache, Upstream,
  Lokal, Rewrite), Record-Typ, Antwortcode, Resolver und einem Von/Bis-Zeitraum
  eingrenzen und mit „Ältere laden" durch die **gesamte gespeicherte Historie**
  zurückblättern — nicht nur die letzten hundert Einträge im Arbeitsspeicher. Der
  Live-Verlauf funktioniert wie bisher, solange kein Filter gesetzt ist; sobald einer
  gesetzt wird, pausiert die Live-Aktualisierung und die Ansicht liest direkt aus dem
  dauerhaften Speicher auf der Platte. Das Suchfeld trifft weiterhin den abgefragten
  Namen und die Client-Adresse (im Live-Verlauf zusätzlich den Gerätenamen).

## [0.10.3] - 2026-11-07

- Geändert: **Alte Images werden nach einem Update automatisch aufgeräumt.** Jedes
  Update behält jetzt genau zwei Images auf dem Host — die eben installierte Version
  und die davor — und entfernt alles Ältere von selbst. Ein Zurückrollen in einem
  Schritt bleibt möglich, während der Speicher nicht mehr vollläuft; die manuelle
  Rückfrage „altes Image löschen?" nach einem Update entfällt damit. Nichts zu
  entscheiden, nichts zu klicken.

## [0.10.2] - 2026-11-07

- Neu: **Öffentliche Container-Images in der GitHub Container Registry (ghcr.io).** Ein
  GitHub-Actions-Workflow baut und veröffentlicht das JustDNS-Image (und das
  Updater-Sidecar) bei jedem Release auf `ghcr.io`, und die offizielle Compose-
  Installation zieht standardmäßig von dort — läuft also überall out of the box, ohne
  Registry-Login, passend zu den öffentlichen GitHub-Tags, die das In-App-Update schon
  prüft. Zum Selbst-Hosten lässt sich `JUSTDNS_IMAGE` weiter auf ein eigenes Registry
  zeigen.

## [0.10.1] - 2026-11-07

- Geändert: **Der Update-Dialog zeigt den echten Datenpfad der Instanz.** Der Hinweis
  zum Sicherungspfad nennt jetzt das tatsächlich genutzte Datenverzeichnis statt eines
  festen `/data`, sodass der „muss dauerhaft sein"-Hinweis auch stimmt, wenn die Daten
  woanders eingehängt sind.

## [0.10.0] - 2026-11-07

- Neu: **Updates in der App.** JustDNS prüft jetzt selbstständig auf GitHub nach
  neuen Versionen und zeigt eine neue Ausgabe auf der Seite **System & Diagnose** —
  mit Änderungsprotokoll und einem **Jetzt updaten**-Knopf — sowie einem dezenten
  Banner für Admins. Ein Klick legt eine Konfigurationssicherung an (an einem
  vorgeschlagenen, dauerhaften Pfad auf dem Datenträger, plus einer herunterladbaren
  Wiederherstellungsdatei), lädt die neue Version und startet den Container neu;
  danach wird gefragt, ob das alte Image gelöscht werden soll. Kein SSH, keine
  `docker`-Befehle. Die Update-Quelle sind öffentliche GitHub-Tags, also funktioniert
  es ohne Token out of the box. Ein-Klick-Updates treibt ein kleines, isoliertes
  **Updater-Sidecar** in der neuen offiziellen `deploy/docker-compose.yml`; der
  DNS-Prozess bekommt den Docker-Socket nie. Eine Installation ohne Sidecar zeigt
  stattdessen einen fertigen Befehl zum Kopieren. Die Update-Prüfung ist
  standardmäßig an und abschaltbar; die gesamte Funktion ist admin-only und
  auditiert. Siehe die [Update-Anleitung](docs/guides/updates.de.md).

## [0.9.1] - 2026-11-06

- Geändert: **Blockierte Dienste zeigen jetzt echte Marken-Logos.** Jede
  Dienst-Karte trägt das echte Markenzeichen (Facebook, TikTok, YouTube,
  WhatsApp, …) statt einer farbigen Initiale, sodass der Katalog auf einen Blick
  lesbar ist. Die Logos sind in die App gebündelt — zur Laufzeit wird kein
  externer Icon-Dienst kontaktiert. Ein Dienstname steht jetzt immer in einer
  Zeile, der Schalter in einer eigenen Zeile. (Disney+ behält eine Buchstaben-
  Kachel, da sein Zeichen im verwendeten Icon-Set nicht verfügbar ist.)

## [0.9.0] - 2026-11-06

- Neu: **Blockierte Dienste.** Eine neue Seite **Blockierte Dienste** (unter Regeln)
  bietet einen kuratierten Katalog bekannter Produkte — soziale Netzwerke, Video- und
  Musikdienste, Messenger, Spiele-Stores — je als einzelnen Schalter, der den ganzen
  Dienst sperrt, indem er die Domains bündelt, auf die er angewiesen ist, statt diese
  von Hand aufzuspüren. Der Schalter auf dieser Seite setzt den **globalen Standard**
  fürs ganze Netzwerk; in den Geräte-, Gruppen- und
  [Zeitplan](docs/guides/schedules.de.md)-Editoren bekommt jeder Dienst dieselbe
  Drei-Zustand-Steuerung **Erben / An / Aus** wie eine Sperrliste — so kannst du
  TikTok überall sperren, aber auf dem eigenen Handy freigeben, oder einen Dienst nur
  für eine Gruppe zu festen Zeiten sperren. Eine **Freigabe gewinnt immer** gegen einen
  gesperrten Dienst, und das [Abfrage-Protokoll](docs/guides/query-log.de.md) nennt den
  Dienst als Grund einer Sperre. Dienste sind **standardmäßig aus**. Siehe die
  [Anleitung zu blockierten Diensten](docs/guides/blocked-services.de.md).

## [0.8.0] - 2026-11-06

- Neu: **Prometheus-Metrik-Endpunkt.** JustDNS kann seine Betriebskennzahlen —
  Abfragezahlen, Cache-Treffer, Block-Entscheidungen, DNSSEC-Urteile, Cache- und
  Listengrößen, Listener-Zustand, aktive Geräte und Abfragelatenz — nun unter
  `GET /metrics` im Prometheus-Textformat bereitstellen, sodass du sie in ein
  bestehendes Monitoring- und Alarmierungssystem einlesen kannst. Der Endpunkt läuft
  über denselben Web-Port wie das Dashboard, ein zusätzlicher Port muss also nicht
  geöffnet werden. Er ist **standardmäßig aus** und schaltet sich erst ein, wenn du
  ein Bearer-Token setzt (`JUSTDNS_METRICS_TOKEN`): ohne Token antwortet er mit 404,
  mit Token muss ein Scraper das Token mitschicken, sonst kommt 401. Ein Scrape liest
  nur die In-Memory-Zahlen, die das Dashboard ohnehin vorhält, und berührt damit nie
  die Abfragedatenbank. Siehe den [Prometheus-Metriken-Leitfaden](docs/guides/metrics.de.md).

## [0.7.1] - 2026-11-06

- Behoben: **Die Versionskachel der System-Seite zeigt jetzt die laufende Version.**
  Sie meldete die interne Build-Version (`0.1.0`) statt der tatsächlichen Version
  (zum Beispiel `v0.7.0`), weil das Container-Image die Version nicht an den
  Server-Prozess durchreichte. Der Fuß des Dashboards war schon korrekt; nun
  stimmt die Diagnose-Kachel damit überein.

## [0.7.0] - 2026-11-06

- Neu: **Seite „System & Diagnose".** Eine neue **System**-Seite (nur Admin, unter
  System) gibt Technikern den Blick „läuft alles, und wenn nicht, wo klemmt's?" –
  ohne in `docker logs` steigen zu müssen. Oben **Laufzeit-Status**-Kacheln: die
  laufende Version und die Laufzeit, welche DNS-Dienste aktiv sind (UDP, TCP, DoT,
  DoH), der letzte Abgleich der Block-/Allow-Listen (rot markiert, wenn er
  fehlschlug), die Größe des Antwort-Caches, die geladenen Listengrößen und die Zahl
  der aktiven Geräte. Darunter ein laufendes **Diagnose-Protokoll** der betrieblichen
  Ereignisse des Servers – Listener- und Zertifikatsprobleme, Feed-Fehler und
  Ähnliches –, das sich nach Schwere filtern (alle / Warnungen+ / nur Fehler),
  durchsuchen und **als .txt herunterladen** lässt. Jeder sichtbare Fehler löst einen
  roten Hinweis aus und wird im Protokoll hervorgehoben.
- Neu: **Diagnose-Protokolle bleiben jetzt erhalten.** Das betriebliche Protokoll
  des Servers ging bisher nur auf die Standardausgabe und war damit nach jedem
  Neustart weg. Es wird nun zusätzlich im Arbeitsspeicher gehalten (für die neue
  Seite) und in eine **rotierende Datei auf der Platte** geschrieben (im Container
  unter `/data/logs`), die standardmäßig die **letzten 7 Tage** aufbewahrt. Die
  Aufbewahrung ist einstellbar (`JUSTDNS_SYSLOG_RETENTION`), und die Datei lässt sich
  ganz abschalten (`JUSTDNS_SYSLOG=off`), während das Protokoll im Dashboard bleibt.
  Das ist das **System-/Diagnose-Protokoll** (wie es dem Server selbst geht); das
  Abfrage-Protokoll, was deine Geräte nachgeschlagen haben, bleibt unverändert.
- Neu: **Health-Endpunkt.** Ein unauthentifiziertes `GET /api/health` meldet, dass
  JustDNS bedient (nur Status und Laufzeit – am offenen Endpunkt keine Version und
  keine Interna), und der Container trägt jetzt einen Docker-**HEALTHCHECK**, der ihn
  nutzt, damit ein Orchestrator oder Uptime-Monitor eine gesunde Instanz von einer
  hängenden unterscheiden kann. Siehe den
  [Leitfaden „System & Diagnose"](docs/guides/system-logs.de.md).

## [0.6.1] - 2026-11-06

- Behoben: **DNSSEC weist keine legitimen Domains mehr ab.** Bei eingeschalteter
  Validierung wurden einwandfreie Seiten – darunter unsignierte Domains und große
  Content-Netzwerke – fälschlich als **Manipuliert** eingestuft und mit SERVFAIL
  abgewiesen, sodass sie nicht mehr luden. JustDNS verlässt sich jetzt auf das Urteil
  des validierenden Upstream-Resolvers (angefragt über das DNSSEC-Signal „DO", ein
  echter Fehler per „CD"-Wiederholung bestätigt), statt die Signaturkette über einen
  Forwarder selbst nachzuprüfen, was nicht zuverlässig möglich war. Eine unsignierte
  Domain wird jetzt korrekt als **Ungesichert** gemeldet und löst normal auf, während
  eine tatsächlich manipulierte oder fehlerhaft signierte Antwort weiterhin abgewiesen
  wird. Siehe die [DNSSEC-Anleitung](docs/guides/dnssec.de.md).

## [0.6.0] - 2026-11-06

- Neu: **DNSSEC-Validierung.** JustDNS kann die kryptografischen Signaturen auf
  DNS-Antworten jetzt selbst prüfen und eine **manipulierte oder fehlerhaft
  signierte Antwort mit SERVFAIL abweisen**, statt eine gefälschte Adresse
  zurückzugeben. Eine neue **DNSSEC**-Seite (unter Überwachung) zeigt das Urteil für
  die letzten Abfragen — **Gesichert**, **Ungesichert**, **Manipuliert** oder
  **Unbestimmt** — mit vier Zähler-Karten, auf die du zum Filtern tippen kannst,
  einem roten Warnbanner, sobald eine manipulierte Antwort abgewiesen wurde, und
  hervorgehobenen Zeilen für manipulierte Antworten. Die Validierung ist
  standardmäßig aus und wird in der Server-Konfiguration (`JUSTDNS_DNSSEC`)
  eingeschaltet. Siehe die [DNSSEC-Anleitung](docs/guides/dnssec.de.md).

## [0.5.2] - 2026-11-06

- Geändert: **übersichtlichere Zwei-Faktor-Spalte auf der Benutzer-Seite.** Die
  „Aktiv"-Anzeige und der „Pflicht"-Schalter klebten bisher aneinander und wirkten
  wie ein Klumpen. Jetzt ist der Status ein ruhiges Badge und „Pflicht" ein echter
  Ein/Aus-Schalter mit Abstand – in der Benutzer-Tabelle wie im Anlegen-Dialog.

## [0.5.1] - 2026-11-06

- Geändert: **mehr Platz auf breiten Bildschirmen.** Die Verwaltungsseiten nutzten
  eine zu schmale Spalte, sodass Übersicht, Benutzer und Audit-Log gequetscht
  wirkten und daneben Platz verschenkten. Der Inhaltsbereich ist jetzt breiter, und
  die Tabellen für Benutzer und Audit-Log nutzen die volle Breite, damit sich ihre
  Spalten nicht mehr zusammendrängen – während die Konto-Formulare in angenehmer
  Lesebreite bleiben.

## [0.5.0] - 2026-11-06

- Geändert: **übersichtlichere Navigation und eine aufgeräumte Verwaltung.** Das
  obere Menü ist jetzt eine gruppierte **Seitenleiste** – Überwachung, Regeln und
  System –, sodass Benutzerverwaltung und Audit-Log einen eigenen, sichtbaren Platz
  haben, statt hinter deinem Namen zu stecken. Am Handy fährt die Seitenleiste über
  einen Menü-Knopf aus, während die häufigsten Seiten weiter mit einem Tipp in der
  unteren Leiste liegen. Neue Konten legst du über einen Dialog mit einem einzigen
  Knopf **+ Benutzer anlegen** an – so bleibt die Benutzer-Seite eine saubere
  Kontenliste –, und das Passwort-Formular auf der Konto-Seite steht in einer
  aufgeräumten Spalte.

## [0.4.0] - 2026-11-06

- Neu: **Benutzer, Rollen & Zwei-Faktor-Anmeldung.** JustDNS ist nicht länger ein
  einzelner Admin. Du kannst jetzt weitere Konten anlegen und jedem eine **Rolle**
  geben: *Administrator* (Vollzugriff), *Bearbeiter* (Filterinhalte ändern –
  Listen, Rewrites, Geräte, Gruppen, Zeitpläne) oder *Betrachter* (nur lesen). Ein
  Konto von vor dieser Änderung bleibt vollwertiger Administrator. Anmeldungen
  lassen sich per **Zwei-Faktor-Authentifizierung** absichern: einen QR-Code in
  eine Authenticator-App scannen, die zehn einmaligen Wiederherstellungscodes
  aufbewahren und fortan nach dem Passwort einen 6-stelligen Code eingeben. Ein
  Administrator kann Zwei-Faktor für alle (in den Einstellungen) oder für einzelne
  Personen (auf der neuen Seite **Benutzer**) **verpflichtend** machen und ein
  Passwort oder einen verlorenen zweiten Faktor zurücksetzen. Ein neues
  **Audit-Log** hält die jüngsten Anmeldungen sowie Konto- und
  Einstellungs­änderungen fest – nie ein Geheimnis, nur wer wann was getan hat.
  Wiederholt falsche Passwörter oder Codes werden ratenbegrenzt, und der letzte
  Administrator kann nie ausgesperrt werden. Siehe die
  [Anleitung](docs/guides/users-roles-mfa.de.md). Konten und Zwei-Faktor bleiben
  bewusst aus Konfigurations-Sicherungen heraus.

## [0.3.0] - 2026-11-06

- Neu: **Zeitpläne.** Eine neue Seite **Zeitpläne** lässt dich nach der Uhr
  filtern. Ein Zeitplan ist ein wiederkehrendes Wochenfenster — wähle die
  Wochentage und eine Start-/Endzeit — das im geöffneten Zustand sein Ziel entweder
  komplett **pausiert** („Internet aus") oder zusätzliche Listen einschaltet und
  einzelne Domains sperrt oder erlaubt. Richte es auf alle, auf eine Gerätegruppe
  oder auf ein einzelnes Gerät. Fenster dürfen über Mitternacht laufen (22:00–07:00
  deckt die Nacht ab) und werden in einer **selbst gewählten Zeitzone** unter den
  Einstellungen gelesen, sodass „Schlafenszeit" auch über die Sommerzeit-Umstellung
  hinweg lokal bleibt. Eine Pause lässt deine Immer-erlauben-Liste weiterhin durch,
  ein Schulportal oder eine Notfallnummer bleibt also erreichbar. Deaktiviere einen
  Zeitplan für die Ferien mit einem Schalter, ohne ihn zu löschen. Siehe die
  [Anleitung](docs/guides/schedules.de.md). Zeitpläne sind in deiner Sicherung
  enthalten.

## [0.2.5] - 2026-11-06

- Verbessert: **aufgeräumte Übersicht.** Die Übersicht ist jetzt in klare Karten
  gegliedert — eine Kopfkarte für die Gesamtzahlen, die Live-Rate mit ihrem
  24-Stunden-Diagramm, eine Reihe mit den Werten der letzten 24 Stunden, die
  aktivsten Geräte und die Resolver-Antwortzeiten nebeneinander sowie darunter
  die zuletzt aufgelösten Anfragen. Alle bisherigen Werte sind weiterhin da;
  Gesamt- und 24-Stunden-Zahlen sind nun beschriftet, damit sie sich nicht mehr
  ähneln.

## [0.2.4] - 2026-11-06

- Wartung: **Sicherheitsaktualisierung der Upstream-DNS-Bibliothek.** JustDNS nutzt
  die Bibliothek hickory-dns, um mit den Resolvern zu sprechen, an die es
  weiterleitet — auch über DoT und DoH. Sie wurde aktualisiert, um drei gemeldete
  Sicherheitsprobleme zu schließen: eines, bei dem ein fehlgeschlagener
  DNSSEC-Prüfvorgang verborgen statt gemeldet werden konnte, eines, bei dem
  unzusammenhängende Einträge einer Antwort verfolgt werden konnten, und eines, bei
  dem eine präparierte Antwort beim Zusammenbauen einer Antwort die CPU auslasten
  konnte. An der Bedienung von JustDNS ändert sich nichts.

## [0.2.3] - 2026-11-06

- Neu: **Resolver-Antwortzeiten.** Die **Übersicht** zeigt jetzt, wie gut jeder
  Upstream-Resolver, an den du weiterleitest, abschneidet — mit Namen aufgeführt, der
  aktivste zuerst, mit der **Median**-Antwortzeit (für das 95. Perzentil über die
  Zeile fahren) und wie viele Antworten er in den letzten 24 Stunden gegeben hat. Es
  zählen nur weitergeleitete Abfragen, du erkennst also auf einen Blick, welcher
  Resolver tatsächlich antwortet und ob einer dauerhaft langsamer ist als die anderen.
  Die Werte überstehen wie die übrigen Statistiken einen Neustart.

## [0.2.2] - 2026-11-06

- Neu: **Top-Domains pro Gerät.** Auf der Seite **Geräte** öffnet die Schaltfläche
  **Top-Domains** bei jeder Zeile die meistgefragten Domains der letzten 24 Stunden
  für genau dieses Gerät, die häufigste zuerst und mit Anzahl. So siehst du auf einen
  Blick, womit ein einzelnes Gerät am meisten spricht. Die Auswertung übersteht wie
  die übrigen Statistiken einen Neustart.

## [0.2.1] - 2026-11-05

- Geändert: **die Verschlüsselungs-Einstellungen sind jetzt an einem Ort.**
  „Verschlüsseltes Client-DNS“ und die Upstream-Verschlüsselung liegen nun in
  einer gemeinsamen Kategorie **Verschlüsselung** (im Experten-Modus), aufgeteilt
  nach den beiden Richtungen: deine Geräte zu JustDNS (DoT/DoH) und JustDNS zu den
  Upstream-Resolvern.

- Verbessert: **die Seite für verschlüsseltes Client-DNS führt dich jetzt.**
  Zuerst den Hostnamen setzen — ohne ihn lassen sich DoT und DoH nicht einschalten
  (das Feld wird stattdessen markiert). Zertifikat-Download/-Upload und die
  kopierfertigen Endpunkt-Adressen erscheinen erst, wenn ein Endpunkt tatsächlich
  läuft, damit die Seite übersichtlich bleibt.

- Behoben: **besser lesbare Einstellungstexte.** Beschreibungen wirken nicht mehr
  gequetscht.

- Wartung: Sicherheitsupdates für das Basis-Image (Debian) und eine
  Web-Abhängigkeit (react-router).

## [0.2.0] - 2026-11-05

- Neu: **verschlüsseltes Client-DNS.** JustDNS ist jetzt von den eigenen Geräten
  über eine verschlüsselte Verbindung erreichbar, nicht nur im Klartext auf Port 53.
  Schalte unter **Einstellungen → Verschlüsseltes Client-DNS** wahlweise
  **DNS-over-TLS (DoT)** auf Port 853 — das, was Android „Privates DNS“ nennt — und/oder
  **DNS-over-HTTPS (DoH)** unter `/dns-query` ein. Deine Geräte- und Gruppenregeln
  gelten für verschlüsselte Clients genauso wie bisher.

- Neu: **Zertifikate nach Wahl.** Standardmäßig erzeugt JustDNS ein eigenes
  Zertifikat; lade die kleine CA auf der Einstellungsseite herunter und installiere
  sie einmalig auf jedem Gerät. Oder lade dein eigenes Zertifikat samt Schlüssel
  hoch (z. B. von einer internen CA oder per ACME). Die Seite zeigt den
  SHA-256-Fingerabdruck des Zertifikats und kopierfertige Endpunkt-Adressen zum
  Einrichten von Handy oder Browser. Das Zertifikat liegt auf dem Daten-Volume und
  übersteht so Updates und Neustarts; eine Schritt-für-Schritt-Anleitung (inklusive
  CA-Installation unter Windows, macOS, iOS, Android und Linux) steht im Handbuch.

## [0.1.32] - 2026-11-05

- Geändert: **die Antwortzeit auf der Übersicht zeigt nur noch den Median.** Die
  große Zahl unter „Letzte 24 Stunden“ ist die typische Auflösungszeit (der Median)
  und damit bei einem Cache-lastigen Setup angenehm niedrig — der Durchschnitt, den
  eine einzelne langsame Upstream-Abfrage hochziehen konnte, und die zusätzlichen
  Perzentile werden nicht mehr angezeigt, damit die Zahl auf einen Blick eindeutig
  ist.

## [0.1.31] - 2026-11-05

- Behoben: **das Bearbeiten einer Gruppe öffnet keine leere Seite mehr.** Beim
  Öffnen des Editors einer frisch angelegten Gruppe (noch ohne Regeln) konnte die
  Seite leer bleiben. Er öffnet jetzt jedes Mal korrekt.

- Geändert: **der Bereich „Gruppen“ steht jetzt oben auf der Geräte-Seite**, und die
  Geräteliste ist **standardmäßig eingeklappt** hinter einer beschrifteten Schaltfläche
  mit Anzahl. So erreichst du die Gruppen, ohne erst an jedem Gerät vorbeizuscrollen,
  und klappst die Liste nur bei Bedarf auf.

- Verbessert: **die Listen-Schalter je Gerät und Gruppe erklären sich selbst.** Der
  dreistufige Schalter neben jeder Sperrliste trägt jetzt eine kurze Erklärung zu
  **Erben / An / Aus** (insbesondere, dass „Aus“ eine Liste für diesen Bereich
  abschaltet, auch wenn sie oben an ist). Stehen nur die zwei eingebauten Objekte
  bereit, weist ein Hinweis, woher weitere Listen kommen — aktiviere unter den
  fertigen Listen eine Sperrliste oder füge eine eigene per URL hinzu, dann erscheint
  sie hier zum Zuweisen je Gerät oder Gruppe.

## [0.1.30] - 2026-11-05

- Neu: **Filterung pro Gerät und pro Gruppe.** Bisher hat JustDNS jeden Client
  gleich gefiltert. Auf der Seite „Geräte“ kannst du jetzt einem einzelnen Gerät
  oder einer ganzen **Gruppe** von Geräten eigene Regeln zusätzlich zur globalen
  Vorgabe geben: einzelne Sperrlisten **an**- oder **ausschalten** und einzelne
  Domains **immer erlauben** oder **immer sperren** — eine Freigabe gewinnt weiter
  über jede Sperre. Die Regeln stapeln sich: ein Gerät erbt die Regeln seiner
  Gruppe, die Gruppe erbt die globale Vorgabe — du stellst also nur ein, was
  abweicht. Jeder Editor zeigt „Hier aktiv“: die Sperrlisten, die nach dem
  Zusammenfalten wirklich gelten. Gruppen legst du im neuen Bereich „Gruppen“ an
  und weist ein Gerät in seinem Editor einer Gruppe zu. Alles steckt im Backup.

- Geändert: **die „Schutz-Stärke“-Stufe ist entfallen.** Der bisherige Schalter
  Aus / Minimal / Standard / Extrem (und der passende Schritt im Einrichtungs-
  Assistenten) ist durch die globalen Sperrlisten und die Schalter für Werbung/
  Schadsoftware ersetzt, die du schon unter Einstellungen siehst — das ist jetzt
  die Standard-Richtlinie, die Geräte und Gruppen verfeinern. Deine vorhandenen
  Listen und Schalter bleiben unberührt; nur die zusätzliche Stufen-Auswahl fällt
  weg.

## [0.1.29] - 2026-11-05

- Verbessert: **die Antwortzeit auf der Übersicht lässt sich nicht mehr so leicht
  verzerren.** Eine einzelne langsame Upstream-Abfrage konnte den reinen
  Durchschnitt unverhältnismäßig nach oben ziehen — gerade an ruhigen Tagen. Unter
  dem Wert stehen jetzt zusätzlich **Median und 95. Perzentil**: der Median ist die
  typische Anfrage — nahe null, wenn das meiste aus dem Cache kommt — während das
  95. Perzentil weiterhin den langsamen Ausläufer zeigt. So liest sich ein
  Cache-lastiger Betrieb so schnell, wie er wirklich ist, und ein einzelner
  Ausreißer dominiert die Zahl nicht mehr.

- Neu: **Geräte erkennen und benennen.** Auf der neuen Seite „Geräte“ gibst du
  den Clients in deinem Netzwerk Namen — einer einzelnen Adresse (z. B.
  `192.168.20.10` = „Wohnzimmer-TV“) oder einem ganzen Bereich in CIDR-Form (z. B.
  `192.168.30.0/24` = „Gäste-WLAN“). Protokoll und Übersicht sprechen dann in
  Namen statt in IP-Adressen, und die Suche im Protokoll findet Geräte auch über
  ihren Namen. Die Liste zeigt alle benannten Geräte und alle zuletzt gesehenen,
  noch unbenannten Clients mit „zuletzt gesehen“, Anzahl der Anfragen und einem
  Punkt für „gerade aktiv“. Pro Gerät sind ein Name, eine optionale Notiz und eine
  Geräte-Art wählbar. Wie lange ein Gerät nach seiner letzten Anfrage als „aktiv“
  gilt, stellst du in den Einstellungen ein (Standard 5 Minuten). Die Namen sind
  in Backups enthalten.

## [0.1.27] - 2026-11-02

- Neu: **wiederverwendbare Adressobjekte für Rewrites.** Gib einer Adresse einen
  Namen — etwa `reverse-proxy` für `192.168.20.9` — im neuen Abschnitt
  „Adressobjekte“ auf der Rewrites-Seite und zeige dann beliebig viele Rewrites
  auf dieses Objekt, statt die Adresse jedes Mal zu tippen. Änderst du die
  Adresse des Objekts einmal, folgen alle Rewrites, die darauf zeigen, auf einen
  Schlag. Ein Objekt kann mehrere Adressen halten (IPv4 und IPv6), und ein Objekt,
  das noch benutzt wird, lässt sich nicht versehentlich löschen — die Meldung
  nennt die Rewrites, die es noch verwenden. Objekte sind in Backups enthalten.

## [0.1.26] - 2026-11-02

- Neu: **Live-Statistik auf der Übersicht.** Die Übersicht zeigt jetzt eine
  Live-Anfragerate — umschaltbar zwischen pro Sekunde und pro Minute — dazu
  Kennzahlen der letzten 24 Stunden (Anfragen gesamt, davon geblockt und die
  durchschnittliche Antwortzeit) sowie die fünf aktivsten Clients mit ihrem
  Anteil an den Anfragen.

## [0.1.25] - 2026-11-02

- Neu: **das Dashboard zeigt die laufende Version.** Eine kleine Zeile am unteren
  Rand jeder Seite nennt jetzt z. B. „JustDNS v0.1.25“ — so siehst du auf einen
  Blick, welcher Stand auf einer Box läuft.
- Behoben: **die obere Leiste ist am Handy nicht mehr verdeckt.** Wenn JustDNS zum
  Startbildschirm hinzugefügt war, konnte der Statusbalken über dem Kopfbereich
  liegen und Sprachumschalter und Abmelden verdecken. Der Header lässt jetzt
  Platz für Statusbalken und Notch.

## [0.1.24] - 2026-11-02

- Neu: **JustDNS als App installieren und ein neu gestaltetes Handy-Erlebnis.**
  JustDNS lässt sich jetzt auf dem Handy oder am PC zum Startbildschirm
  hinzufügen und öffnet sich wie eine normale App — im Vollbild und mit eigenem
  Symbol; selbst bei kurz unterbrochener Verbindung zeigt es noch die zuletzt
  geöffnete Seite. Wenn eine Installation möglich ist, bietet eine kleine Leiste
  „JustDNS als App installieren“ auf Android und Chrome die Installation mit
  einem Tipp an oder zeigt auf dem iPhone den Schritt „Teilen → Zum
  Home-Bildschirm“.
- Verbessert: **die gesamte Oberfläche ist jetzt fürs Handy gemacht.** Auf einem
  kleinen Bildschirm wird das obere Menü zu einer unteren Leiste, die du mit dem
  Daumen erreichst, und jede Seite — Übersicht, Protokoll, Block/Allow, Rewrites
  und Einstellungen — passt sich an, sodass nichts abgeschnitten wird oder
  seitlich übersteht. Am Computer bleibt die Ansicht unverändert.

## [0.1.23] - 2026-11-02

- Neu: **beim Erst-Start aus einer Sicherung wiederherstellen.** Der
  Einrichtungs-Assistent fragt jetzt auf dem allerersten Bildschirm, ob du
  JustDNS neu aufsetzen oder eine Sicherung einspielen willst. Beim
  Wiederherstellen legst du zuerst dein Login an (eine Sicherung enthält nie
  eins) und spielst dann Einstellungen, Listen und Rewrites in einem Schritt aus
  der Datei zurück — ideal für einen neuen Server, eine Neuinstallation oder
  einen Umzug.

## [0.1.22] - 2026-11-02

- Neu: **wähle, womit eine geblockte Domain beantwortet wird.** Unter
  Einstellungen → Schutz gibt es jetzt „Antwort auf geblockte Domains“ mit der
  Wahl zwischen einer Null-IP (`0.0.0.0` / `::`) und NXDOMAIN. **Neuer Standard
  ist die Null-IP** — wie bei AdGuard —, sodass eine geblockte Werbe- oder
  Telemetrie-Adresse sofort ins Leere läuft und Apps weitermachen. Das behebt
  Hänger, die manche Video-Player (zum Beispiel YouTube) mit NXDOMAIN hatten, wo
  der Player die gescheiterte Anfrage wiederholte. Du kannst jederzeit auf
  NXDOMAIN zurückstellen.

## [0.1.21] - 2026-11-02

- Neu: **Einrichtung sichern und wiederherstellen.** Unter Einstellungen →
  Sichern & Wiederherstellen lädst du jetzt deine gesamte Konfiguration als eine
  Datei herunter — deine Einstellungen, deine Allow-/Block-Einträge samt
  Notizen, deine abonnierten Listen und deine Rewrites — und stellst sie später
  oder auf einem neuen Server wieder her. Das Wiederherstellen ersetzt deine
  aktuelle Einrichtung und greift sofort. Das Anfrage-Protokoll und dein Login
  sind nicht Teil der Datei.

## [0.1.20] - 2026-11-02

- Der Punkt neben „Schutz aktiv" zeigt jetzt für alle ein sanftes Lebenszeichen.
  Mit aktivierter „Reduzierter Bewegung" siehst du statt des expandierenden Rings
  ein weiches Auf- und Abblenden — der Punkt wirkt lebendig, ganz ohne große
  Bewegung.

## [0.1.19] - 2026-11-01

- Behoben: **Deine DNS-Rewrites überstehen jetzt Updates und Neustarts.** Sie
  werden zusammen mit deinen übrigen Einstellungen auf dem Daten-Volume
  gespeichert, statt im App-Image, wo ein Update sie verlieren konnte. Ab jetzt
  angelegte Rewrites bleiben sicher erhalten.

## [0.1.18] - 2026-11-01

- Neu: **Notizen zu deinen Allow- und Block-Einträgen.** Füge jeder Domain, die
  du erlaubst oder sperrst, eine optionale Notiz hinzu — eine Erinnerung, warum
  sie dort steht, etwa „mein eigenes NAS“ oder „Smart-TV-Telemetrie“. Die Notiz
  erscheint unter dem Namen und lässt sich jederzeit ändern oder entfernen; sie
  ist nie Pflicht und ändert nichts an der Auflösung.
- Deine eigenen Allow- und Block-Einträge heben sich jetzt deutlicher vom Rest
  der Karte ab, sodass ein von Hand hinzugefügter Name leicht zu erkennen ist.
- Rewrites: Beim Löschen eines Rewrites fragt JustDNS jetzt zuerst nach, damit
  nichts aus Versehen verschwindet.
- Rewrites: Bearbeite einen Rewrite direkt in seiner Zeile, statt dass sich das
  Formular oben wieder füllt.
- Rewrites: Die Ziel-Adressen stehen jetzt sauber untereinander in Spalten, so
  ist auch eine längere Liste gut zu lesen.
- In der Übersicht pulsiert der Punkt neben „Schutz aktiv“ sanft, sodass du auf
  einen Blick siehst, dass er aktiv ist.

## [0.1.17] - 2026-11-01

- Neu: **Einrichtungs-Assistent.** Beim allerersten Start führt dich JustDNS
  jetzt Schritt für Schritt durch die Einrichtung: Administratorkonto und Name
  der Instanz, gewünschte Schutz-Stärke (Aus/Minimal/Standard/Extrem, jeweils mit
  Erklärung und Hinweis zum Fehlalarm-Risiko), Resolver-Profil und Aufbewahrung
  der Protokolle.
- Neu: **Schutz-Stärke als eine Einstellung.** Unter Einstellungen → Schutz
  wählst du die Stufe mit einem Schalter; sie setzt die passenden Sperrlisten
  automatisch. Stellst du einzelne Listen von Hand um, steht die Stufe auf
  „Eigen“.
- Neu: **Name der Instanz.** Gib deiner JustDNS-Installation einen Namen, der
  oben in der Leiste erscheint — praktisch, wenn du mehrere betreibst.

## [0.1.16] - 2026-11-01

- Neu: **Eigene Resolver.** Unter Einstellungen → Upstream-Resolver kannst du
  jetzt die Server der Profile durch deine eigene Liste ersetzen. Pro Eintrag
  gibst du eine IP-Adresse und die Übertragungsart an — Klartext, DNS over TLS
  oder DNS over HTTPS — und bei den verschlüsselten Varianten den Servernamen.
  Die Änderung greift sofort, ganz ohne Neustart; solange noch kein gültiger
  Eintrag vorhanden ist, bleiben die Profil-Server aktiv.

## [0.1.15] - 2026-11-01

- Neu: **Geschwindigkeitsprofile.** Unter Einstellungen → Upstream-Resolver
  wählst du mit einem Schalter zwischen „Privatsphäre“, „Ausgewogen“ und
  „Tempo“. Jedes Profil nutzt passende Resolver — die dir gleich darunter
  angezeigt werden — und legt fest, wie eifrig Antworten zwischengespeichert und
  im Voraus erneuert werden: von datenschutzfreundlich bis besonders schnell.
- Änderungen an Profil, Verschlüsselung und Auswahlmodus greifen jetzt sofort,
  ganz ohne Neustart.

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
