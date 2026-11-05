# Änderungsverlauf

Was sich in JustDNS getan hat, in einfacher Sprache. Eine englische Fassung
dieser Datei liegt daneben: [CHANGELOG.md](CHANGELOG.md). Datumsangaben im
Format Jahr-Monat-Tag.

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
