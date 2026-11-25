# Warum eine Anfrage geblockt wurde (Entscheidungs-Trace)

*Eine englische Fassung dieser Anleitung liegt in [decision-trace.md](decision-trace.md).*

Jeder Eintrag im [Protokoll](query-log.de.md) kann dir sagen, **warum** JustDNS ihn so
beantwortet hat, wie er beantwortet wurde. Ein farbiges **Grund-Tag** in der Zeile nennt
die Ursache auf einen Blick, und wenn du den Eintrag öffnest, erscheint der
**Entscheidungs-Trace**: die Stufen, die der Resolver der Reihe nach durchlaufen hat, mit
der entscheidenden Stufe hervorgehoben. So beantwortest du „Warum wurde *das* geblockt?“
ohne zu raten, und die Korrektur ist nur einen Klick entfernt.

## Das Grund-Tag in der Zeile

Eine geblockte oder gewarnte Zeile trägt direkt neben dem Namen ein kleines farbiges Tag,
damit du das Protokoll überfliegen und die Ursache sehen kannst, ohne etwas zu öffnen:

- **Blockliste** (rot) — ein Name auf einer deiner [Blocklisten](lists.de.md) oder ein
  per CNAME getarnter Tracker, der enttarnt wurde.
- **Threat-IP** (blau) — die Antwort löste auf eine Adresse auf einer
  Antwort-IP-[Bedrohungsliste](lists.de.md) auf, also wurde die ganze Antwort verweigert.
- **Phishing** (violett) und **Warnung** (gelb) — ein [Phishing](phishing.de.md)-Block
  oder ein reiner Warn-Treffer. Diese tragen bereits den Phishing-Marker und behalten ihn.

## Den Trace öffnen

Klicke eine Zeile an, um sie aufzuklappen. Unter dem Datensatz und dem Block-Grund liegt
der Abschnitt **Entscheidung**: die Stufen, die die Anfrage durchlaufen hat, in der
Reihenfolge, in der sie liefen.

Jeder Schritt zeigt die **Stufe**, ein **Zeichen** für das, was sie tat, und optional ein
Detail (der antwortende Resolver, die nachgeahmte Marke, der Antwort-Code):

- **✕ Treffer** — diese Stufe hat geblockt. Es gibt immer nur eine.
- **! Warnung** — diese Stufe hat die Anfrage markiert, aber durchgelassen.
- **✓ bestanden** — dieses Gate lief und ließ die Anfrage durch.
- **· Info** — ein neutraler Hinweis, etwa die Quelle der Antwort oder der Antwort-Code.

Die Stufen in Pipeline-Reihenfolge sind **Blockliste**, **Phishing**, **Quelle**,
**CNAME-Enttarnung**, **Antwort-IP**, **DNSSEC** und **Antwort**. Eine Anfrage listet nur
die Stufen, die tatsächlich liefen.

Damit eine saubere Antwort ruhig bleibt, werden die nur **bestandenen** Gates
weggeklappt; die entscheidende Stufe und die neutralen Hinweise sind sofort zu sehen. Mit
**Bestandene Stufen zeigen** blendest du den Rest ein.

## Das Pause-Banner

Wenn beim Eintreffen der Anfrage die [Schutz-Pause](query-log.de.md) aktiv war, sagt dir
ein Banner oben im Trace das. Während die Pause läuft, werden die Schutzstufen
übersprungen und darum nicht aufgeführt: der Trace zeigt, wie die Anfrage tatsächlich
behandelt wurde, nicht wie sie mit aktivem Schutz behandelt worden wäre.

## Sprung zur Regel

Die entscheidende Stufe ist ein **Link**. Er führt dich direkt auf die Seite, die diese
Regel verwaltet, mit der richtigen Stelle hervorgehoben:

- Ein **Namensblock** oder eine **CNAME-Enttarnung** öffnet deine eigenen
  [Listen](lists.de.md) beim Freigabe-Editor, sodass du den Namen in einem Schritt als
  Ausnahme hinzufügen kannst.
- Ein **Antwort-IP**-Block öffnet die [Listen](lists.de.md)-Presets, vorgefiltert auf die
  Threat-IP-Listen.
- Ein **Phishing**-Treffer öffnet die [Phishing-Schutz](phishing.de.md)-Seite.

So bist du bei einem Fehlalarm nur einen Klick von der Seite entfernt, die ihn behebt.

## Im Cluster

In einem [Hochverfügbarkeits](high-availability.de.md)-Cluster nennt der Trace zusätzlich
die Node, die entschieden hat, damit du erkennst, welcher Server eine Anfrage bearbeitet
hat, wenn Geräte mehr als eine Node nutzen.

---

## Für technisch Neugierige

Der Trace wird einmal gebaut, während die Anfrage behandelt wird, aus den Signalen, die
der Resolver ohnehin hat: welche Stufe eine geblockte Antwort verweigert hat, ob die
Schutz-Pause aktiv war, das Phishing-Urteil, Quelle und Resolver der Antwort, das
DNSSEC-Urteil und der Antwort-Code. Das Grund-Tag wird aus der entscheidenden Stufe
abgeleitet (ein Namens- oder CNAME-Block → *Blockliste*, ein Antwort-IP-Block →
*Threat-IP*, ein Phishing-Block → *Phishing*, ein reiner Warn-Treffer → *Warnung*).

**Wo er liegt.** Der vollständige Trace liegt nur im dauerhaften DuckDB-Protokoll auf der
Platte, als eine zusätzliche Spalte an der Anfrage-Zeile. Er wird **nie** im
Arbeitsspeicher-Ring gehalten, der die Live-Ansicht speist: die Hot-View behält nur das
winzige Grund-Tag, und der Trace wird daraus entfernt, bevor die Zeile in den Ring geht.
Wenn du einen Eintrag öffnest, holt die Oberfläche den Trace bei Bedarf für genau diese
Zeile (`GET /api/queries/trace`, abgeglichen über Zeit, Client, Name und Record-Typ der
Zeile). So kostet die Erklärbarkeit nichts auf dem Anfrage-Pfad und nichts im
Arbeitsspeicher des Resolvers, und eine ältere Zeile, die vor diesem Feature aufgezeichnet
wurde, zeigt schlicht keinen Trace.

**Warum bestandene Schritte behalten werden.** Der Trace hält die Gates fest, die
bestanden haben, nicht nur das, das geblockt hat, damit der Drawer den ganzen Weg zeigen
kann, den eine Anfrage genommen hat — nützlich, um zu bestätigen, dass etwa Namensblock
und Phishing-Stufe beide liefen und frei waren, bevor eine Antwort ausgeliefert wurde. Sie
sind standardmäßig weggeklappt, damit eine schlichte Antwort lesbar bleibt.
