# System & Diagnose

*Eine englische Fassung dieses Leitfadens liegt daneben: [system-logs.md](system-logs.md).*

Die **System**-Seite (unter *System*, nur für Admins) beantwortet auf einen Blick
eine Frage: **Ist JustDNS gesund, und wenn etwas klemmt, wo?** Sie stellt eine
Momentaufnahme des **Laufzeit-Status** neben einen laufenden Strom des
**Diagnose-Protokolls** – der betrieblichen Ereignisse des Servers –, sodass du nur
noch selten auf `docker logs` ausweichen musst.

Das ist das **System**-Protokoll (wie es dem Server selbst geht). Es ist etwas
anderes als das **Abfrage-Protokoll** (was jedes Gerät nachgeschlagen hat), das
unter *Überwachung* liegt und im [Leitfaden zum Abfrage-Protokoll](query-log.de.md)
beschrieben ist.

## Laufzeit-Status

Die Kacheln oben sind eine Momentaufnahme, die sich alle paar Sekunden aktualisiert:

- **Version** – der laufende Build, darunter die **Laufzeit** des Prozesses.
- **Dienste** – die vier DNS-Zugänge (**UDP**, **TCP**, **DoT**, **DoH**), je mit
  einem Punkt: grün, wenn der Dienst bedient, grau, wenn er aus ist. DoT und DoH sind
  aus, bis du sie unter [Verschlüsseltes Client-DNS](encrypted-dns.de.md)
  einschaltest – grau ist dort also normal.
- **Letzter Listen-Abgleich** – wann die Block-/Allow-Abos zuletzt geholt wurden und
  ob der Lauf erfolgreich war. Die Kachel wird rot, wenn der letzte Abgleich
  fehlschlug.
- **Cache-Einträge** – wie viele Antworten gerade im Antwort-Cache liegen.
- **Aktive Geräte** – wie viele Clients kürzlich angefragt haben (innerhalb des
  Aktiv-Fensters aus den [Einstellungen](settings.de.md)).
- **Geladene Listeneinträge** – die Gesamtzahl der geladenen Regeln, aufgeschlüsselt
  nach Werbung/Tracker, Malware, eigenen Blocks und Allow-Einträgen.

## Das Diagnose-Protokoll

Unter dem Status läuft das Protokoll. Jede Zeile ist ein Ereignis, das der Server
ausgegeben hat, neueste zuerst, mit **Zeit**, einem farbigen **Schwere-Abzeichen**,
der **Quelle** (welcher Teil von JustDNS es protokolliert hat) und der **Meldung**.

- **Nach Schwere filtern** über die Knöpfe: *Alle*, *Warnungen+* (Warnungen und
  Fehler) oder *Nur Fehler*.
- **Suche** engt die Liste auf Zeilen ein, deren Meldung, Quelle oder Felder deinen
  Text enthalten – sofort beim Tippen.
- **Nach Kategorie filtern** über das Auswahlmenü (erscheint, sobald mehr als ein
  Teilsystem protokolliert hat): nur *DNS & Upstreams*, *Listen*, *Web & HA* usw., um
  einen Teil des Servers zu beobachten, ohne dass der Rest vorbeiscrollt.
- **Suche** engt die Liste auf Zeilen ein, deren Meldung, Quelle oder Felder deinen
  Text enthalten – sofort beim Tippen.
- **Als .txt laden** speichert genau das, was du gerade siehst (mit Filter und Suche)
  als Textdatei – praktisch für einen Fehlerbericht.
- Ist ein **Fehler** sichtbar, erscheint über dem Protokoll ein **roter Hinweis**,
  und die Fehlerzeilen sind rot hinterlegt, damit ein Problem kaum zu übersehen ist.

### Ausführlichkeit (vorübergehend)

Über dem Protokoll liegt ein **Ausführlichkeits**-Schalter – *Normal*, *Debug* oder
*Trace*. Er ändert, wie viel JustDNS schreibt, nicht nur, was der Strom zeigt:
*Debug* und *Trace* schalten den Mehrwert frei, den der DNS-Dienst sonst stillhält
(Upstream-Wiederholungen pro Abfrage, Matcher-Neuaufbau u. a.). Er wirkt sofort, ohne
Neustart.

Die Änderung ist **bewusst vorübergehend**: Ein Neustart kehrt zu *Normal* zurück,
damit eine vergessene Debug-Sitzung nicht still die Platte vollschreibt. Nutze ihn,
um ein Problem live zu beobachten, und stell danach wieder auf *Normal*. (Wurde der
Server mit explizitem `RUST_LOG` gestartet, zeigt der Schalter *Custom*, und eine
Auswahl hier überschreibt das bis zum nächsten Neustart.)

### Was der Dienst protokolliert

Über Start und Fehler hinaus meldet der DNS-Dienst jetzt die betrieblichen
Ereignisse, die früher unsichtbar waren – damit ein Problem hier auftaucht und nicht
nur an den Symptomen:

- **Upstream-Zustand** – antwortet ein Resolver nicht mehr, wird er als *ungesund*
  markiert (eine Warnung, nicht eine pro Abfrage); antwortet er wieder, eine Zeile
  *recovered*.
- **Listen-Abgleich** – jedes Abo protokolliert seine neue Eintragszahl und die
  Dauer des Downloads, plus eine **Warnung**, wenn ein Feed plötzlich die meisten
  Einträge verliert (wahrscheinlich kaputte Quelle, die sonst still den Schutz
  schwächt).
- **DNSSEC** – eine Validierungs-Verletzung (*Bogus*, die gefälschte Antwort wird
  verworfen) wird als Warnung mit dem Namen protokolliert.
- **Einstellungen & Zeitpläne** – das Anwenden neuer Einstellungen (baut Upstream-Pool,
  Cache-Tuning und Policies neu) und das Öffnen/Schließen eines Zeitplan-Fensters
  protokollieren je eine Zeile, sodass eine Live-Änderung nachvollziehbar ist.

Das meiste davon ist *Info* oder *Warnung* und immer an. Das Detail pro Abfrage
(Wiederholung über TCP, der DNSSEC-Gegenprobe mit abgeschalteter Prüfung) ist *Debug*
und erscheint nur, solange der Ausführlichkeits-Schalter auf *Debug* oder *Trace*
steht.

## Wo das Protokoll liegt

Das Diagnose-Protokoll wird an drei Stellen zugleich geschrieben:

- **Standardausgabe** – wie bisher, für `docker logs` und deine Container-Plattform.
- **Im Arbeitsspeicher** – ein rollierender Puffer der jüngsten Ereignisse; das ist,
  was die System-Seite zeigt. Er wird bei einem Neustart geleert.
- **Eine Datei auf der Platte** – eine täglich rotierende Datei, damit das Protokoll
  einen **Neustart übersteht**. Im Container liegt sie unter `/data/logs` (auf dem
  eingehängten Volume), benannt `justdns.<datum>.log`.

Standardmäßig bewahrt das Datei-Protokoll die **letzten 7 Tage** auf. Wie du das
änderst oder abschaltest, steht unten.

---

## Für die technisch Neugierigen

### Konfiguration

Die Datei-Senke steuern drei Umgebungsvariablen, beim Start gelesen:

| Variable | Standard | Wirkung |
| --- | --- | --- |
| `JUSTDNS_SYSLOG_DIR` | `logs` (das Image setzt `/data/logs`) | Verzeichnis für die rotierenden Protokolldateien. |
| `JUSTDNS_SYSLOG_RETENTION` | `7` | Wie viele Tagesdateien aufbewahrt werden; ältere werden gelöscht. |
| `JUSTDNS_SYSLOG` | *(an)* | Auf `off`, `0`, `false` oder `no` setzen, um die **Datei**-Senke abzuschalten. |

Schaltet man die Datei-Senke ab, arbeiten der Speicher-Puffer (und damit die
System-Seite) und die Standardausgabe weiter – nur die Kopie auf der Platte entfällt.
Der Speicher-Puffer hält unabhängig davon die jüngsten 5000 Ereignisse.

Die Protokollstufe folgt der üblichen `RUST_LOG`-/`tracing`-`EnvFilter`-Konvention
(Standard `info`); hebt man sie auf `debug`, fließt der Mehrwert an alle drei Senken
zugleich. Der **Ausführlichkeits**-Schalter auf der System-Seite tut dasselbe zur
Laufzeit über eine `tracing`-Reload-Schicht – er tauscht einen neuen Filter-Ausdruck
ein (z. B. `info,justdns_dns=debug,…`) und baut den Callsite-Interest-Cache neu, sodass
zuvor abgeschaltete Ereignisse ohne Neustart zu fließen beginnen. Er wird **nicht**
persistiert; ein Neustart kehrt zur `RUST_LOG`-/Standardstufe zurück.

### Endpunkte

Drei Endpunkte versorgen diese Seite:

- `GET /api/health` – **unauthentifiziert**. Liefert
  `{"status":"ok","uptimeSecs":N}` und sonst nichts: keine Version, keine Interna,
  also gefahrlos für einen Health-Checker. Der Docker-`HEALTHCHECK` des Containers
  ruft ihn per curl ab.
- `GET /api/logs/system?level=&limit=` – **nur Admin**. Liefert die jüngsten
  Ereignisse aus dem Speicher-Ring, neueste zuerst, optional auf eine Mindestschwere
  gefiltert.
- `GET /api/logs/level` / `PUT /api/logs/level` – **nur Admin**. Liest oder setzt die
  Live-Ausführlichkeit (`normal`/`debug`/`trace`). Die Änderung läuft über die
  Reload-Schicht und wird nicht persistiert. Ein Setzen wird im
  [Audit-Log](users-roles-mfa.de.md) festgehalten (`logs.level`).
- `GET /api/diag` – **nur Admin**. Liefert die Status-Momentaufnahme hinter den
  Kacheln (Version, Laufzeit, Dienste, Abgleich, Cache- und Listengrößen, aktive
  Clients und die aktive Protokollstufe).

Die Version wird bewusst **nur** am authentifizierten `/api/diag` ausgegeben, nie am
offenen Health-Endpunkt, damit ein unauthentifizierter Aufrufer den Build nicht
fingerprinten kann.

### Umsetzung

Die Protokollerfassung ist eine kleine `tracing`-Schicht, die jedes Ereignis an einen
begrenzten, geteilten Ring-Puffer anhängt; derselbe Puffer wird der Web-Schicht
gereicht, sodass die API liest, was der Rest des Prozesses schreibt. Die rotierende
Datei ist eine eigene `tracing-appender`-Schicht (Tagesrotation, ANSI aus). Der
Laufzeit-Status ist ein günstiger geteilter Griff, den das Binary beim Hochfahren der
Listener und beim Listen-Abgleich aktualisiert und den der Diagnose-Endpunkt bei
Bedarf liest – pro Anfrage wird nichts gepollt oder neu berechnet.
