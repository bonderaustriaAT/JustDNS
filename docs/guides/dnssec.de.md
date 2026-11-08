# DNSSEC-Validierung

*Eine englische Fassung dieser Anleitung liegt in [dnssec.md](dnssec.md).*

**DNSSEC** ist das Signatursystem, mit dem ein Resolver prüfen kann, dass eine
DNS-Antwort wirklich aus der Zone stammt, die sie vorgibt, und unterwegs nicht
verändert wurde. Ohne DNSSEC kann jeder, der deinen Verkehr manipulieren kann, eine
gefälschte Adresse für einen Namen zurückgeben. Mit eingeschalteter Validierung
prüft JustDNS diese Signaturen selbst und **weist eine Antwort ab, die nicht
besteht** — eine manipulierte Antwort wird so zu einem harten Fehler statt zu einer
falschen Adresse.

Die **DNSSEC**-Seite (unter *Überwachung*) zeigt, was diese Prüfung ergeben hat.

## Die vier Urteile

Jede weitergeleitete Abfrage erhält eines von vier Urteilen:

- **Gesichert** — eine gültige, lückenlose Signaturkette von der Wurzel bis zur
  Antwort. Die Antwort ist echt.
- **Ungesichert** — die Zone ist nicht signiert, es gibt also nichts zu prüfen. Das
  ist normal: Ein Großteil des Internets ist weiter unsigniert.
- **Manipuliert** (Bogus) — es gab Signaturen, die aber **nicht** bestanden. JustDNS
  weist die Antwort ab und gibt **SERVFAIL** zurück, sodass der Client die gefälschten
  Daten nie sieht. Das ist der Fall, den man beachten sollte.
- **Unbestimmt** — es konnte kein Urteil gefällt werden, etwa weil die Vertrauenskette
  nicht auswertbar war.

## Die Seite lesen

- **Vier Zähler-Karten** oben zeigen, wie viele Abfragen in jedes Urteil fielen.
- **Tippe auf eine Karte**, um die Tabelle darunter auf nur dieses Urteil zu filtern;
  tippe erneut (oder auf *Filter zurücksetzen*), um wieder alles zu zeigen.
- Wurde eine Antwort als **Manipuliert** abgewiesen, erscheint oben ein rotes
  **Warnbanner** — eine manipulierte oder fehlerhaft signierte Zone ist einen Blick
  wert.
- Die **Tabelle** listet die zuletzt validierten Abfragen: Zeit, Gerät, Name und ein
  farbiges **Urteils-Badge**. Manipulierte Zeilen sind hervorgehoben, damit sie
  auffallen.

## Einschalten

Die DNSSEC-Validierung ist **standardmäßig aus**. Sie wird in der Server-Konfiguration
über die Umgebungsvariable **`JUSTDNS_DNSSEC`** eingeschaltet (auf `1`, `true` oder
`on` setzen) und greift beim Start von JustDNS. Solange sie aus ist, werden keine
Urteile aufgezeichnet und die Seite bleibt leer.

---

## Für die technisch Neugierigen

JustDNS ist ein **Forwarder**, kein rekursiver Resolver, und stützt sich daher auf den
validierenden Upstream-Resolver (die voreingestellten DNS4EU- und Quad9-Endpunkte
validieren beide), statt die Signaturkette selbst nachzulaufen. Der vollständige
Kettenlauf über einen Forwarder ist unzuverlässig — er kann die `NSEC`/`NSEC3`-Nachweise,
die eine Delegation als unsigniert *beweisen*, nicht immer abrufen —, und eine frühere
Version, die das versuchte, bezahlte dafür damit, einwandfreie unsignierte Domains als
**Manipuliert** einzustufen. Das Urteil leitet sich jetzt aus den Signalen des Upstreams
ab:

- Die Abfrage trägt das EDNS-**DO**-Bit, das den Upstream auffordert zu validieren und das
  Ergebnis im **AD**-Flag (Authentic Data) zu melden. Eine Antwort **mit** AD ist
  **Gesichert**; eine schlichte Antwort **ohne** AD stammt aus einer unsignierten Zone und
  ist **Ungesichert**.
- Einen Signaturfehler hält ein validierender Upstream als **SERVFAIL** zurück, statt die
  Daten zu liefern. Um das von einem gewöhnlichen Upstream-Fehler zu unterscheiden,
  wiederholt JustDNS die Abfrage einmal mit gesetztem **CD**-Bit (Checking Disabled): Kommen
  die Daten dann mit abgeschalteter Prüfung zurück, war das ursprüngliche SERVFAIL ein
  Validierungsfehler — **Manipuliert**. Schlägt es weiter fehl, gilt es als echter
  Upstream-Fehler.

Ein **Manipuliert**-Urteil wird in **SERVFAIL** ohne Antworten umgewandelt und **nicht
gecacht** — auch nicht durch den Hintergrund-Prefetch in den Cache aufgefrischt —, sodass
eine gefälschte oder manipulierte Antwort nie einen Client erreicht. (Negative Antworten —
NXDOMAIN/NODATA — tragen kein Urteil, weil uns das AD-Flag auf diesem Weg nicht vorliegt.)

Das Urteil entsteht **nur bei eingeschalteter Validierung** — ist DNSSEC aus, wird das
DO-Bit nicht gesetzt und kein Urteil gemeldet, sodass sich das Protokoll nicht füllt und die
*Manipuliert → SERVFAIL*-Durchsetzung wirkungslos bleibt. Das Urteil wird zusammen mit dem
übrigen Abfrage-Ereignis im DuckDB-Store gespeichert, und die Zähler-Karten sind die
Gesamtwerte über die Laufzeit aus der In-Memory-Statistik.

Die Validierung kostet höchstens einen zusätzlichen Roundtrip bei einem Cache-Fehlschlag
(nur wenn der Upstream SERVFAIL liefert, für die CD-Wiederholung); normale Antworten kosten
nichts über das DO-Bit hinaus, und Cache-Treffer sind nicht betroffen.
