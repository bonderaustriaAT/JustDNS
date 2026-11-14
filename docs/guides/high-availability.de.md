# Hochverfügbarkeit (Knoten-Mesh)

*Eine englische Fassung dieser Anleitung liegt in [high-availability.md](high-availability.md).*

Ein einzelner JustDNS ist ein einzelner Ausfallpunkt: Startet er neu, aktualisiert
er sich oder stürzt er ab, steht die Namensauflösung im Netz. Mit der
Hochverfügbarkeit betreibst du **bis zu vier JustDNS-Knoten**, die synchron bleiben —
so kann jeder für sich antworten, und du kannst einen abschalten, ohne dass das Netz
es merkt.

Alle Knoten sind **gleichwertig**: Jeder ist beschreibbar, und eine Änderung auf
einem fließt zu allen anderen. Es gibt keinen festen Primary. JustDNS hält die
Konfigurationen synchron und zeigt dir den Zustand jedes Knotens; es schwenkt den
Verkehr aber **nicht** selbst um (siehe
[Warum es kein automatisches Failover gibt](#warum-es-kein-automatisches-failover-gibt)).

Die Knoten bilden ein **vollständiges Mesh**: Jeder gleicht direkt mit jedem anderen
ab, sodass ein ausgefallener Knoten den Abgleich der übrigen nie unterbricht und ein
zurückkehrender Knoten automatisch aufholt. Einen neuen Knoten verbindest du immer nur
mit **einem** bestehenden von Hand — die anderen findet und vermascht er selbst.

Die Funktion ist **standardmäßig aus**. Es läuft kein Abgleich und die
Knoten-Schnittstelle bleibt geschlossen, bis du sie aktivierst und die Knoten selbst
verbindest.

## Was synchronisiert wird

Alles, was du im Dashboard einstellst:

- **Einstellungen** — alle Servereinstellungen (Resolver, DNSSEC, Tempo-Profil …).
- **Listen** — deine Allow- und Block-Einträge und deine Feed-Abos.
- **Rewrites** — DNS-Rewrites und die Adressobjekte, auf die sie verweisen.
- **Geräte und Gruppen** — benannte Clients, ihre Gruppen und die Richtlinien je Ebene.
- **Zeitpläne** — die wiederkehrenden Zeitfenster.
- **Zertifikate** — der ganze [Zertifikatsspeicher](certificates.de.md),
  **inklusive der privaten Schlüssel**, sodass ein Zertifikat, das du auf einem Knoten
  anlegst, hochlädst oder erneuerst, auf allen vorhanden ist (sein Schlüssel reist nur
  über diesen authentifizierten Knoten-zu-Knoten-Abgleich, nie zu einem Client). Die
  Zertifikatsauswahl der Block-Seite wird mitgeteilt; die DoT/DoH-Auswahl bleibt
  knotenlokal, weil jeder Knoten seinen eigenen Hostnamen bedient.
- **Benutzer und Logins** — alle Konten mit Rolle, Passwort und Zwei-Faktor
  (TOTP-Secret und Wiederherstellungscodes). So gelten dieselben Logins — inklusive
  erzwungener MFA — auf jedem Knoten. Bestätigte MFA gilt als „klebrig": Hat ein
  Knoten sie für ein Konto, behält der Abgleich sie und trägt sie auf die anderen
  Knoten — eine versehentlich ältere Kopie kann die Zwei-Faktor-Anmeldung also nie
  still abschalten. Der Kompromiss ist bewusst: **Um MFA von einem Konto zu
  entfernen, lösche sie auf jedem Knoten**, sonst spielt ein Knoten, der sie noch
  hat, sie beim nächsten Abgleich wieder ein.

- **Protokoll / Statistik-Historie** — jeder Knoten zieht auch die
  **Protokolleinträge der anderen Knoten**, sodass der *Protokoll*-Explorer auf jedem
  Knoten den gesamten Cluster-Verkehr über das Aufbewahrungsfenster zeigt und die
  Lebenszeit-Statistik nach einem Schwenk auf einen anderen Knoten nicht bei null
  steht. Nur der Explorer und die Gesamtzähler vereinigen sich; das Live-Dashboard
  (der jüngste Ring, die heutige Aktivität, die Geräte- und Resolver-Ansichten)
  bleibt knotenlokal. Jede Protokollzeile trägt ein kleines **Knoten-Badge**, das
  zeigt, von welchem Knoten sie stammt. Wie weit das zurückreicht, bestimmt die
  **Protokoll-Aufbewahrung** unter *Einstellungen → Protokoll* (standardmäßig sieben
  Tage): dieses Fenster beschneidet jetzt das lokale Log **und** begrenzt, was
  synchronisiert wird.

Bewusst **nicht** synchronisiert wird alles Knotenlokale — die Knoten-ID, die eigene
Adresse und der Anzeigename dieses Knotens, sein DoT/DoH-**Hostname** samt
Zertifikatsauswahl, die **Block-Seiten-Server-IP** (jeder Knoten zeigt geblockte
Namen auf sich selbst), die Mesh-Mitgliedschaft, **Login-Sitzungen** (du meldest dich
an jedem Knoten separat an) und der Betriebszustand. Die Hochverfügbarkeit auf einem
Knoten abzuschalten schaltet sie darum nie auch auf den anderen ab.

## Einrichtung

Du richtest zuerst **jede JustDNS-Instanz ganz normal** ein, jede auf eigenem Host.
Dann auf **jedem** Knoten:

1. **Öffne *Hochverfügbarkeit*** in der Seitenleiste (nur für Admins).
2. **Schalte die Hochverfügbarkeit ein** (Schritt 1 des Assistenten).
3. **Setze den gemeinsamen Schlüssel** (Schritt 2) — ein langes, zufälliges Geheimnis.
   Er muss auf **jedem Knoten exakt gleich** sein; damit weisen sie sich gegenseitig
   aus. Alternativ pinnst du ihn über eine Umgebungsvariable (siehe unten).
4. **Setze die eigene Adresse dieses Knotens** (Schritt 3) — die Web-Adresse, unter der
   die anderen Knoten ihn erreichen (z. B. `http://192.168.20.5:8080`). Sie ist aus der
   Adresse vorausgefüllt, über die du die Seite aufgerufen hast. Sie wird im Cluster
   bekannt gemacht, damit sich neu hinzukommende Knoten mit diesem vermaschen können;
   ohne sie kann dieser Knoten zwar von anderen ziehen, bleibt neuen aber unsichtbar.
5. **Tritt dem Cluster bei** (Schritt 4) — trage die Adresse **irgendeines** Knotens
   ein, der bereits im Cluster ist (z. B. `http://192.168.20.6:8080`). JustDNS prüft die
   Verbindung mit dem gemeinsamen Schlüssel und vermascht sich bei Erfolg mit diesem
   Knoten — den Rest findet und verbindet es dann von selbst.

Der **erste** Knoten, den du aufsetzt, hat noch niemanden zum Beitreten; er wartet
einfach bei Schritt 4, bis ein anderer Knoten ihm beitritt, und wechselt dann von
selbst in die Cluster-Ansicht.

Sobald ein Knoten vermascht ist, wird die Seite zur **Knotenliste**: dieser Knoten und
jeder Partner, je mit Ampel, Adresse, Version und Zeitpunkt des letzten Abgleichs, dazu
Knöpfe für **sofortigen Abgleich** und **Cluster verlassen** sowie ein Entfernen je
Partner (das × auf seiner Karte).

### Einen Knoten entfernen

Einen Partner mit dem **×** entfernen oder selbst den **Cluster verlassen** fragt zuerst
nach und **benachrichtigt dann die verbleibenden Knoten** — der Knoten verschwindet
mesh-weit auf einmal und bleibt nicht als unerreichbarer Eintrag liegen, den jeder
Knoten weiter anzusprechen versucht. Das Entfernen bleibt: die Kopie eines anderen
Knotens kann ihn nicht wieder hereinholen. Einen versehentlich entfernten Knoten holst
du einfach durch **erneutes Verbinden** zurück (seine Daten auf den anderen Knoten
bleiben erhalten, der Abgleich konvergiert also günstig). Einen nur **vorübergehend
offline** stehenden Knoten lässt du stehen — er holt von selbst auf, wenn er
zurückkommt; entferne ihn erst, wenn er dauerhaft weg ist.

Für einen **dritten oder vierten** Knoten öffne in der Knotenliste **Weiteren Knoten
hinzufügen**. Der Assistent führt dich durch und zeigt einen Live-Knotenzähler (z. B.
*2/4 Knoten*): richte den neuen Knoten ein, schalte HA ein und setze dann — am neuen
Knoten — denselben **gemeinsamen Schlüssel** (den du hier **anzeigen und kopieren**
kannst) und trage **eine** Seed-Adresse dieses Clusters ein (der Assistent bietet die
Adresse dieses Knotens zum Kopieren an). Der neue Knoten vermascht sich dann von selbst
mit allen, bis zu vier Knoten insgesamt; ab vier meldet der Assistent, dass der Cluster
voll ist.

### Wer beim Beitritt gewinnt

Jeder Knoten wurde getrennt eingerichtet, hat also schon eigene Einstellungen, einen
eigenen Admin, vielleicht überlappende Listeneinträge. Wenn ein Knoten dem Cluster
**beitritt**, triffst du für ihn **eine Wahl**: *der Cluster gewinnt* (Standard —
dieser Knoten übernimmt dessen Konfiguration) oder *dieser Knoten gewinnt* (seine
Konfiguration setzt sich durch). Diese Wahl entscheidet **jeden echten Konflikt** auf
einmal — dieselbe Einstellung, denselben Listeneintrag, dieselbe Gruppe, denselben
Gerätenamen, Benutzer und so weiter.

Was nur auf **einer** Seite existiert, bleibt immer erhalten — ein Beitritt ist eine
Vereinigung, nie eine Löschung. Nach dem Beitritt konvergieren die Knoten, und ab dann
gilt die normale Regel: **Die jeweils neuere Änderung gewinnt.** (Genau darum
überträgt sich eine auf einem Knoten neu eingerichtete MFA auf alle — eine frische
Einrichtung ist schlicht die neueste Änderung.)

### Den Schlüssel per Umgebungsvariable pinnen

Statt den Schlüssel im Assistenten einzutippen, kannst du ihn auf beiden Knoten setzen:

```
JUSTDNS_SYNC_KEY=<eine lange, zufällige Zeichenkette>
```

Ist er so gesetzt, zeigt der Assistent den Schlüssel als außerhalb der App verwaltet
und lässt ihn dort nicht ändern. Zwei weitere optionale Variablen justieren den Sync:

```
JUSTDNS_NODE_ID=<stabile ID>       # wird automatisch erzeugt und gespeichert, wenn leer
JUSTDNS_SYNC_INTERVAL=30           # Sekunden zwischen den Abgleichen (mindestens 5)
```

### Sicherheit: Behandle den Schlüssel wie die Passwort-Datenbank

Seit Benutzer mitsynchronisiert werden, trägt der Snapshot, den ein Knoten vom Partner
holt, **alle Konten samt Passwort-Hashes und TOTP-Secrets** sowie die privaten
Zertifikatsschlüssel. Diese Nutzlast reist **verschlüsselt** — jeder Knoten leitet aus
dem gemeinsamen Schlüssel einen symmetrischen Schlüssel ab (HKDF-SHA256) und versiegelt
jede Peer-Antwort mit ChaCha20-Poly1305. Ein Mitleser auf der Leitung sieht also nur
Geheimtext, und der gemeinsame Schlüssel selbst reist **nie** — ein Knoten beweist, dass
er ihn hält, indem er eine Nutzlast schickt, die sich unter ihm öffnen lässt. Das gilt
auch über eine offene `http`-Leitung im selben Netz. Der gemeinsame Schlüssel bleibt
damit das Kronjuwel: Wer ihn hat, kann denselben Schlüssel ableiten, den Snapshot lesen
und verändern (und damit auch MFA aushebeln). Darum:

- **Nutze einen langen, zufälligen Schlüssel** (mindestens 16 Zeichen; der Assistent
  verlangt das). Ein kurzer oder erratbarer Schlüssel gibt die Credential-Datenbank preis.
- **TLS ist jetzt optionale Härtung, keine Pflicht.** Die Nutzlast ist ohnehin
  verschlüsselt; eine HTTPS-Adresse des Partners legt nur eine zweite Schicht darüber.
- **Bei Verdacht auf einen kompromittierten Schlüssel:** den Schlüssel auf beiden
  Knoten wechseln **und** alle Passwörter neu setzen — der alte Schlüssel konnte die
  Hashes bereits kopiert haben.

> **Beim Update beide Knoten aktualisieren.** Die Peer-Verschlüsselung ist ein
> abgestimmter Schnitt: Ein Knoten dieser Version kann die versiegelten Nutzlasten eines
> alten Knotens nicht öffnen und umgekehrt. Läuft ein Knoten noch auf einer älteren
> Version, pausiert der Sync, bis beide aktualisiert sind — die Auflösung läuft die ganze
> Zeit weiter, nur der Abgleich wartet.

## Den ganzen Cluster von einer Node aus verwalten

Standardmäßig sind alle Knoten gleichwertig und beschreibbar, und eine Änderung auf einem
beliebigen Knoten synchronisiert zu den anderen. Willst du lieber an einem Ort verwalten,
wähle auf der HA-Seite eine **Hauptnode** (den Primary). Ab dann gilt:

- **Die Hauptnode ist der Ort zum Verwalten.** Alles, was du dort änderst, synchronisiert
  wie bisher nach außen.
- **Jede andere Node ist eine nur-lesende Follower-Node.** Ihr Dashboard öffnet sich
  weiterhin – zum Einloggen und Nachsehen beim Debuggen – zeigt aber einen Hinweis mit der
  Adresse der Hauptnode und weist Änderungen ab. (Die HA-Seite selbst bleibt auf einer
  Follower-Node bedienbar, damit du den Cluster reparieren oder die Rolle übernehmen
  kannst.)
- **Die Einstellungen der Hauptnode gewinnen immer.** Weil Follower nie selbst Änderungen
  erzeugen, gibt es nichts zu streiten – der Cluster konvergiert auf den Stand der
  Hauptnode.

Jeder Knoten hat zudem ein paar Einstellungen, die *nie* synchronisieren, weil sie pro
Knoten verschieden sein müssen: Anzeigename, DoT/DoH-Hostname, welches gespeicherte
Zertifikat er zeigt und seine Block-Page-Adresse (jeder Knoten zeigt blockierte Namen auf
sich selbst). Von der HA-Seite der Hauptnode aus setzt du diese jetzt auf **jeder** Node –
öffne die Karte der Node, bearbeite **Node-Einstellungen** und speichere. Ebenso löst du
dort ein **Ein-Klick-Update** auf jeder Node aus (sie sichert sich zuerst, verschlüsselt mit
dem eingegebenen Passwort), sodass du dich nie auf den anderen Knoten einloggen musst.

Du kannst von der Hauptnode aus auch den **ganzen Cluster auf einmal** aktualisieren. Auf
ihrer HA-Seite aktualisiert **Ganzen Cluster aktualisieren** jede Node in sicherer
Reihenfolge: jeden Follower einzeln, die Hauptnode **zuletzt**. Zur nächsten Node geht es
erst, wenn die eben aktualisierte wieder läuft und die **neue Version** ausliefert (so ist
nie mehr als eine Node gleichzeitig unten), und die Hauptnode selbst kommt erst dran, wenn
alle Follower gesund sind. Jede Node nimmt **dieselbe** Version, sichert sich zuerst
(verschlüsselt mit dem einen Passwort, das du eingibst), und du musst dich auf keiner Node
einloggen. Kommt eine Node nicht rechtzeitig zurück, **stoppt der Vorgang und die Hauptnode
bleibt unangetastet** – du stehst also nie mit dem ganzen Cluster mitten im Update da:
Node in Ordnung bringen und erneut starten.

Ist die Hauptnode einmal weg, öffne die HA-Seite einer überlebenden Node und wähle **Als
Hauptnode übernehmen**. Diese Node wird zum Primary und ist wieder beschreibbar. Zurück zum
alten „alle gleich"-Verhalten kommst du, indem du die Hauptnode leerst – ohne festgelegte
Hauptnode ist jede Node beschreibbar wie zuvor.

## Beide Knoten bei den Clients eintragen

Weil JustDNS den Verkehr nicht selbst umschwenkt, sollten deine Clients **beide**
Knoten nutzen können. Zwei gängige Wege:

- **Zwei Resolver.** Verteile beide JustDNS-IPs per DHCP (oder in den
  Netzwerkeinstellungen jedes Geräts). Das Betriebssystem weicht auf den zweiten
  Resolver aus, wenn der erste nicht antwortet. Einfach und überall verfügbar.
- **Eine gemeinsame virtuelle IP.** Setze etwas wie `keepalived` vor das Paar, sodass
  eine schwebende IP immer auf einen lebenden Knoten zeigt. Mehr bewegliche Teile,
  aber die Clients sehen nur eine Adresse.

## Warum es kein automatisches Failover gibt

Automatisches Failover — ein Knoten erklärt den anderen für tot und reißt dessen
Adresse an sich — ist die Stelle, an der Hochverfügbarkeits-Setups meist scheitern:
Ein kurzer Netz-Aussetzer lässt beide Knoten glauben, sie seien allein, beide
beanspruchen die Adresse, und die Auflösung ist kaputter als ohne jedes Failover. Es
sicher zu machen braucht Quorum und Fencing — viel Maschinerie für einen Resolver im
Heim- oder Kleinbüro-Einsatz.

JustDNS hört bewusst bei **Sync + Status** auf. Beide Knoten antworten weiter, deine
Clients erreichen ohnehin beide — das gibt dir die Ausfallsicherheit ohne das
Split-Brain-Risiko. Der Resolver-Rückfall des Betriebssystems *ist* dein Failover,
und er ist erprobt.

---

## Für die technisch Neugierigen

**Das Modell ist Last-Writer-Wins mit Tombstones.** Jeder Knoten führt ein kleines
*Ledger* in `/data/ha.json`: Für jede synchronisierte Entität (Schlüssel
`"<Sammlung>/<ID>"`) vermerkt er einen Inhalts-Hash, den Zeitpunkt, zu dem ihr
aktueller Zustand gesetzt wurde, und welcher Knoten ihn gesetzt hat. Je Zyklus holt
ein Knoten das Ledger und die Sammlungen des Partners; pro Schlüssel gewinnt das
spätere `(Zeitstempel, Knoten-ID)`. Eine Löschung hinterlässt einen **Tombstone** —
eine datierte Markierung, dass die Entität weg ist —, damit „auf A gelöscht" gegen
„auf B noch vorhanden" gewinnt, statt dass B sie in der nächsten Runde neu anlegt.
Tombstones werden nach 30 Tagen entfernt.

**Es konvergiert, weil der Merge symmetrisch und kommutativ ist.** Jeder Knoten zieht
und mergt jedes Intervall dasselbe gegen jeden seiner Partner, und der Merge ist eine
reine Funktion über eine *totale* Ordnung aus `(Zeitstempel, Knoten-ID)` — die
Knoten-ID bricht den Gleichstand, wenn zwei Uhren dieselbe Millisekunde zeigen.
`merge(A, B)` und `merge(B, A)` liefern dasselbe, und Mergen ist idempotent; alle
Knoten landen also bei identischer Konfiguration, egal wer zuerst und in welcher
Reihenfolge gemergt hat. Weil der Snapshot auch die Ledger-Einträge des Partners
trägt, propagiert eine Änderung **transitiv**: A's Änderung erreicht C über B, auch
wenn A und C nie direkt sprachen — darum trägt dieselbe Engine zwei Knoten wie vier.

**Das Mesh baut sich aus einem gegossipten Roster selbst auf.** Neben der Konfiguration
trägt jeder Snapshot einen *Roster* — Knoten-ID → Adresse —, zusammengeführt mit
demselben Last-Writer-Wins (für die eigene Adresse ist ein Knoten maßgeblich). Zieht
ein Knoten von einem Partner, meldet er in den Request-Headern zugleich seine eigene ID
und Adresse an — so wird selbst ein Knoten bekannt, der nur zieht. Jeden Zyklus
verbindet sich ein Knoten mit jedem Roster-Mitglied, mit dem er noch nicht vermascht
ist, bis zu drei Partner; **eine** Seed-Adresse genügt also, damit ein neuer Knoten den
ganzen Cluster findet und sich mit ihm vermascht. Der Roster trägt auch **Tombstones**:
einen Knoten entfernen — oder ein Knoten, der geht — markiert seinen Roster-Eintrag als
entfernt, auf diesen Moment gestempelt, und diese Markierung gewinnt und gossippt
genauso, sodass eine lebende Kopie auf einem anderen Knoten ihn nicht wiederbeleben kann
und das Auto-Mesh die Verbindung einstellt. Der entfernende (oder gehende) Knoten meldet
den Weggang zusätzlich direkt bei den verbleibenden Partnern (`/api/ha/depart`), damit
er sofort wirkt statt erst beim nächsten Zug; ein in dem Moment nicht erreichbarer
Partner erfährt es später über den Tombstone. Für seine **eigene** Lebendigkeit ist ein
Knoten maßgeblich, sodass ein wirklich wiederkehrender Knoten sich neu ankündigt und
einen alten Tombstone überschreibt.

**Ein Beitritt ist nur ein einmaliger Zeitstempel-Schub.** Die Wahl „der Cluster /
dieser Knoten" ist kein eigener Modus: Das gesamte Ledger des beitretenden Knotens wird
einmalig gestempelt — weit zurück, um den Cluster zu übernehmen, oder auf jetzt, um zu
gewinnen —, sodass das normale Last-Writer-Wins danach jeden echten Konflikt
entscheidet, während Einträge, die es nur auf einem Knoten gibt, keinen Gegner haben
und einfach erhalten bleiben. Automatisch entdeckte Partner stempeln nie um; der Seed
hat das bereits entschieden.

**MFA wird so zusammengeführt, dass ein Abgleich sie nicht abschalten kann.** Ein
Benutzerdatensatz wird als Ganzes per Last-Writer-Wins aufgelöst — das konnte, als die
Knoten erstmals Benutzer synchronisierten, einen Knoten, dessen Kopie einen späteren
Zeitstempel *ohne* zweiten Faktor trug, allen die MFA nehmen lassen. Darum wird MFA
eigens zusammengeführt: Hat eine der Seiten einen *bestätigten* Faktor für ein Konto,
behält ihn der zusammengeführte Datensatz. Die Auswahl ist deterministisch und
symmetrisch, sodass die Knoten auf „MFA aktiv" konvergieren statt zu flattern. Der
bewusste, sichere Preis: MFA entfernen propagiert nicht mehr — lösche sie auf jedem
Knoten.

**Das Protokoll synchronisiert sich als append-only Vereinigung.** Ein Query-Event
wird nie geändert oder gelöscht, es gibt also keinen Konflikt zu lösen: jeder Knoten
zieht die Events der *anderen* Knoten und hängt sie an sein eigenes durable Log an.
Eine Zeile trägt eine **Herkunft** — leer für eine lokal beantwortete Anfrage, die
Knoten-ID des Peers für eine gezogene — und ein Knoten liefert **nur seine eigenen**
Zeilen aus, sodass im Vollmesh nichts weitergeleitet wird und keine Schleife entsteht.
Der Abgleich ist inkrementell: jeder Knoten führt je Peer ein **Wasserzeichen** (das
jüngste `at_ms`, das er eingespielt hat) und fragt nur Zeilen ab dort an, wobei die
Grenz-Millisekunde erneut gezogen und per natürlichem Schlüssel entdoppelt wird. Ein
Hintergrund-Prune löscht auf jedem Knoten Zeilen jenseits des Aufbewahrungsfensters,
sodass das Log begrenzt bleibt; der Pull ist auf dasselbe Fenster begrenzt, damit kein
Knoten Zeilen holt, die er sofort wieder verwerfen würde. Die gezogenen Zeilen
speisen das durable Log und die Lebenszeit-Summen, nie die Live-Hot-View — darum bleibt
das Dashboard knotenlokal, während der Explorer vereinigt.

**Der Hash ist über Builds hinweg stabil.** Es ist ein 64-bit-FNV-1a über die
kanonische JSON-Form jeder Entität, nicht Rusts `DefaultHasher` (dessen Ausgabe sich
zwischen Builds ändern kann und nie gespeichert werden darf). Nur so kann ein Knoten
über Neustarts und Versionen hinweg „unverändert" von „bearbeitet" unterscheiden.

**Die Knoten-Endpunkte authentifizieren und verschlüsseln mit dem gemeinsamen
Schlüssel, nicht mit einem Login.** Ein Knoten hält keine Dashboard-Sitzung. Aus dem
gemeinsamen Schlüssel leiten beide Seiten per HKDF-SHA256 denselben AEAD-Schlüssel ab.
Ein Aufruf trägt im Header `X-Ha-Auth` ein versiegeltes, zeitgestempeltes Token (Knoten-ID
und eigene Adresse, verschlüsselt mit ChaCha20-Poly1305); dass es sich öffnen lässt, ist
zugleich der Authentizitätsbeweis — der Schlüssel reist nie als Token, und ein Stempel
außerhalb eines 60-Sekunden-Fensters wird abgewiesen, was Replays begrenzt. Die
Antworten von `/api/ha/hello`, `/api/ha/snapshot` und `/api/ha/queries` sind ebenso
versiegelt, sodass die Nutzlast — Zertifikatsschlüssel, Passwort-Hashes, MFA-Secrets —
nie im Klartext über die Leitung geht. Die Endpunkte antworten mit **404**, bis die
Hochverfügbarkeit aktiv *und* ein Schlüssel gesetzt ist — auf einer Instanz, die die
Funktion nicht nutzt, existiert die Sync-Fläche also gar nicht. Eine bestehende Paarung
wird bewusst **nicht** verlangt (der Schlüssel ist die Absicherung), sonst könnten sich
zwei frische Knoten nie zum ersten Mal verbinden. Die Verwaltungs-Endpunkte (aktivieren,
Schlüssel setzen, verbinden, jetzt abgleichen) liegen hinter dem normalen Admin-Login.

**Die Hauptnode wird an zwei Stellen durchgesetzt.** Die Knoten-ID des gewählten Primary
reist in den synchronisierten Einstellungen mit (`cluster.primaryNodeId`), sodass alle
Knoten übereinstimmen, wer es ist. Auf einer Follower-Node weist der Auth-Guard jeden
Schreibzugriff (jedes Nicht-GET) mit einem 403 ab, das die Adresse der Hauptnode nennt –
außer der Fläche für das eigene Konto und `/api/ha`, damit du deinen eigenen Login
verwalten und den Cluster von einer Follower-Node aus reparieren oder übernehmen kannst.
Das Dashboard spiegelt dieselbe Regel, indem es seine Bearbeiten-Elemente abschaltet – du
siehst also eine Nur-Lese-Ansicht statt Knöpfen, die fehlschlagen würden. Weil eine
Follower-Node nie schreibt, konvergiert Last-Writer-Wins ohnehin auf die Werte der
Hauptnode; das Gate macht „an einem Ort verwaltet" nur explizit und sichtbar. Eine
**unbekannte** Knoten-ID als Hauptnode wird abgelehnt, damit der Cluster nie auf einen
nicht existierenden Primary festgenagelt werden kann (was *jede* Node nur-lesend machen
würde).

**Die Fern-Verwaltung der Knoten nutzt den versiegelten Peer-Kanal.** Die vier
Node-eigenen Felder und das Ein-Klick-Update werden von der Hauptnode über denselben
schlüssel-authentifizierten ChaCha20-Poly1305-Kanal wie der Snapshot gesetzt – die Nutzlast
(samt dem Backup-Passwort eines Fern-Updates) ist versiegelt, nie im Klartext. Eine Node
schreibt immer nur ihre *eigenen* Identitätsfelder und führt ihr *eigenes* Update aus (das
Backup landet in ihrem eigenen Datenverzeichnis), die Hauptnode fragt also an, statt
hineinzugreifen. Nur der Knoten, der *gerade* die Hauptnode ist, verwaltet einen anderen
Knoten aus der Ferne; ein Follower, der dazu aufgefordert wird, lehnt ab – „Verwaltung von
einer Stelle" gilt also auch für die Verwaltungsaktionen selbst, nicht nur für normale
Änderungen. Der gemeinsame Schlüssel ist ohnehin das Wurzel-Credential des Clusters
(er öffnet jeden Passwort-Hash im Snapshot), diese Endpunkte fügen also kein neues
Vertrauen hinzu.

**Das Cluster-Update ist orchestriert, nicht einfach ausgelöst.** „Ganzen Cluster
aktualisieren" fährt eine kleine Zustandsmaschine auf der Hauptnode. Sie fixiert eine
Zielversion für den ganzen Lauf und je Follower der Reihe nach: löst das Update der Node
über den versiegelten Kanal aus (mit der fixierten Version) und pollt dann deren
`/api/ha/hello`, bis es genau diese Version meldet – das passiert erst, wenn der Container
auf dem neuen Image neu erstellt wurde und wieder ausliefert, sodass eine Node, die nur auf
dem *alten* Image neu gestartet ist, nicht fälschlich als fertig gilt. Erst wenn jeder
Follower die Zielversion meldet, aktualisiert sich die Hauptnode selbst. Scheitert ein
Follower oder kommt er nicht im Zeitfenster zurück, bricht der Lauf ab und die Hauptnode
bleibt unangetastet – der Cluster steht nie mit allen Nodes mitten im Update. Der
Lauf-Zustand liegt im Speicher der Hauptnode (er wird beim eigenen Update am Ende
zurückgesetzt); deshalb ist das Dashboard beim letzten Schritt kurz weg – das ist die
Hauptnode, die sich selbst neu erstellt.

**Der Read-only-Follower ist eine Bequemlichkeit, keine Sicherheitsgrenze zwischen
Admins.** Die Hauptnode beantwortet „wo wird konfiguriert", nicht „wer darf
konfigurieren". Da Konten und Rollen mitsynchronisiert werden, ist *jedes* Admin-Konto auf
jedem Knoten ein vollwertiger Admin: Das read-only Dashboard eines Followers ist der
alltägliche „hier nicht verwalten"-Hinweis und schützt vor versehentlichen Änderungen,
aber ein auf einem Follower eingeloggter Admin kann jederzeit **Zur Hauptnode machen**
drücken – eine bewusste, bestätigte Aktion – und dieser Knoten wird beschreibbar. Behandle
einen Admin-Login daher als clusterweit und voll privilegiert, egal wo er stattfindet;
verlass dich nicht auf „ist ja nur ein Follower", um eine Sitzung einzugrenzen. Die echten
Grenzen sind der Admin-Login selbst und der gemeinsame HA-Schlüssel, nicht das
Hauptnode-Kennzeichen.

**Uhren-Drift ist wichtig.** Last-Writer-Wins vertraut den Uhren, halte also beide
Knoten per NTP synchron. Der Knoten-ID-Tiebreak verhindert Flattern, wenn zwei Stempel
auf genau dieselbe Millisekunde fallen, aber eine stark falsche Uhr kann eine
veraltete Änderung gewinnen lassen — der übliche Vorbehalt jedes LWW-Systems.

**Einstellungen mergen als eine Einheit.** Das gesamte Einstellungsobjekt ist ein
einzelner Ledger-Eintrag; werden also *zwei verschiedene* Einstellungen fast
gleichzeitig auf den beiden Knoten geändert, verliert der ältere Gesamtstand beide.
Für ein Paar im Heim- oder Kleinbüro-Einsatz ist das ein vertretbarer Kompromiss; ein
feinkörnigeres Mergen der Einstellungen ist ein möglicher späterer Schritt.
