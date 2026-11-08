# Geplante Berichte

*Die englische Fassung dieses Leitfadens steht in [reports.md](reports.md).*

Die Seite **Berichte** verdichtet das rohe [Protokoll](query-log.de.md) zu regelmäßigen,
lesbaren Zusammenfassungen des DNS-Geschehens über einen Tag oder eine Woche: wie viele
Anfragen es gab, wie viele geblockt wurden, welche Domains und Geräte am aktivsten waren
und wie schnell geantwortet wurde. Es ist die Seite, die „Wie sah die letzte Woche aus?“
beantwortet, ohne Tausende Protokollzeilen durchzublättern — und die, die man jemandem
gibt, der nur die Eckdaten möchte.

Berichte werden **nirgendwohin verschickt**. Sie werden auf dem Server erzeugt, auf der
Datenplatte abgelegt, hier angesehen und bei Bedarf als Datei heruntergeladen. Keine
E-Mail, kein Webhook, kein zu verbindendes Konto.

## Was ein Bericht enthält

Jeder Bericht deckt ein Fenster ab — den jeweils vorigen vollen **Tag** oder die vorige
volle **Woche** — und zeigt:

- **Eckzahlen** — Anfragen gesamt, davon geblockt samt **Block-Rate** und die mittlere
  Antwortzeit.
- **Nach Quelle** — die Aufteilung auf Cache, Upstream, Lokal, Rewrite und Geblockt.
- **Top-Domains**, **Top geblockte Domains** und **Top-Geräte**, jeweils als Rangliste.
  Die Geräteliste zeigt den freundlichen [Gerätenamen](devices.de.md), sofern zum
  Zeitpunkt der Erzeugung einer passte.
- **DNSSEC-Urteile** — die Zähler für sicher, unsigniert, ungültig und unbestimmt, nur
  angezeigt, wenn die [Validierung](dnssec.de.md) welche erfasst hat.

Die Zahlen gelten **clusterweit**: In einem [Hochverfügbarkeits](high-availability.de.md)-
Verbund decken sie den Verkehr aller Knoten über das Fenster ab, genau wie der
Protokoll-Explorer — nicht nur den gerade betrachteten Knoten.

## Zeitplan einschalten

Beide Takte sind **standardmäßig aus**. Unter **Zeitplan** lässt sich einschalten:

- **Täglicher Bericht** — Uhrzeit wählen. Deckt den jeweils vorigen vollen Tag ab.
- **Wöchentlicher Bericht** — Wochentag und Uhrzeit wählen. Deckt die jeweils vorige
  volle Woche ab.

Maßgeblich ist die Zeitzone aus den [Einstellungen](settings.de.md); „06:00“ bedeutet also
sechs Uhr morgens deiner Ortszeit, auch über die Sommerzeit hinweg. Ein Bericht für ein
Fenster wird einmal erzeugt; eine geschlossene App verpasst ihn nicht, solange der Server
läuft, wenn die Uhrzeit kommt.

## Jetzt einen erzeugen

Du musst nicht auf den Zeitplan warten. **Jetzt erzeugen** erstellt sofort einen Bericht
für den letzten vollen Tag bzw. die letzte volle Woche — praktisch beim ersten Öffnen der
Seite oder wenn du die Zahlen dieser Woche vor dem Wochenlauf sehen willst. Der neue
Bericht öffnet sich sofort und erscheint in der Liste darunter.

## Ansehen und herunterladen

Die Liste **Erzeugte Berichte** zeigt jeden gespeicherten Bericht, neueste zuerst, mit
Fenster und Eckdaten. Ein Klick auf **Ansehen** öffnet den formatierten Bericht mit den
Zahlen und den drei Top-Listen. Von dort aus:

- **HTML (druckbar)** lädt eine eigenständige Seite, die sich in jedem Browser öffnen und
  drucken lässt — samt „als PDF drucken“, falls du ein PDF ablegen oder weiterreichen
  willst.
- **CSV** lädt die Top-Listen als tabellenfreundliche Datei.

Ältere Berichte werden automatisch ausgedünnt, damit die Liste eine nützliche jüngere
Historie bleibt und nicht unbegrenzt wächst.

---

## Für die technisch Neugierigen

Ein Bericht ist ein **eingefrorenes Aggregat**, keine Live-Ansicht. Bei der Erzeugung
führt JustDNS eine Handvoll `GROUP BY`-Abfragen gegen den DuckDB-Protokollspeicher über
das halboffene Fenster `[start, end)` aus — denselben Speicher und dieselbe clusterweite
Vereinigung, die der [Explorer](query-log.de.md) liest — und legt die entstandenen Summen
und Top-Listen als JSON auf der `/data`-Platte ab. Weil er eingefroren ist, behält ein
Bericht die Gerätenamen, wie sie beim Lauf waren, und späteres Lesen kostet nichts: keine
erneute Abfrage.

**Der Scheduler** ist ein Hintergrund-Thread aus derselben Familie wie das Retention-Prune
und die HA-Sync-Schleife. Er wacht periodisch auf, berechnet das jeweils fällige Fenster
pro Takt in der konfigurierten Zeitzone (mit derselben `chrono`/`chrono-tz`-Mechanik wie
die [Zeitpläne](schedules.de.md)) und erzeugt ein Fenster nur, wenn es fällig und noch
nicht gespeichert ist. Die Dedupe erfolgt über den Fensterbeginn, sodass ein Tag oder eine
Woche höchstens einmal geschrieben wird, egal wie oft der Thread aufwacht — und bei beiden
Takten aus tut der Thread gar nichts.

**Die Ablage ist pro Takt gedeckelt** (die jüngsten täglichen und wöchentlichen Berichte
bleiben, die ältesten fallen heraus) und wird atomar über dasselbe
tmp-Datei-dann-umbenennen-Muster geschrieben wie die übrigen JSON-Speicher auf der
Datenplatte. Die Takt-Einstellungen selbst liegen in den [Einstellungen](settings.de.md)
und synchronisieren im Cluster daher wie jede andere Einstellung — jeder Knoten ist sich
über den Zeitplan einig, und jeder Knoten kann die von ihm erzeugten Berichte ausliefern.

**Die Download-Formate sind bewusst schlicht.** HTML ist ein eigenständiges Dokument mit
Inline-Stilen, das sauber druckt und zum Rendern keinen Server-Umweg braucht — „als PDF
drucken“ deckt den PDF-Fall ab, ohne eine schwere PDF-Bibliothek einzubinden. CSV trägt
die drei Top-Listen für alle, die die Zahlen in einer Tabelle weiterdrehen wollen.
