# Richtlinien pro Gerät & Gruppe

*Eine englische Fassung dieser Anleitung liegt in [policies.md](policies.md).*

Standardmäßig filtert JustDNS jedes Gerät im Netzwerk gleich. **Richtlinien** ändern
das: Ein einzelnes Gerät oder eine ganze **Gruppe** von Geräten kann eigene Regeln
zusätzlich zur globalen Vorgabe bekommen — strenger für das Kinder-Tablet, lockerer
fürs Arbeits-Notebook.

Verwaltet wird das alles auf der Seite **Geräte**.

## Was du je Gerät oder Gruppe einstellen kannst

- **Welche Sperrlisten an sind.** Schalte jede eingebaute Kategorie (Werbung &
  Tracker, Schadsoftware) oder jedes Listen-Abo **an** oder **aus** — nur für diesen
  Bereich.
- **Immer erlauben** — bestimmte Domains werden hier nie geblockt, auch wenn eine
  Liste sie blocken würde.
- **Immer sperren** — bestimmte Domains werden hier immer geblockt, auch wenn sonst
  nichts sie blocken würde.

Eine **Freigabe gewinnt immer über eine Sperre**, überall. Steht ein Name auf einer
„Immer erlauben“-Liste, wird er aufgelöst — Punkt.

## Wie sich die Regeln stapeln (Vererbung)

Stell dir drei Ebenen vor, jede baut auf der darüber auf:

```
Globale Vorgabe   ← Einstellungen + Listen: gilt für alle
      │  (eine Gruppe ändert nur, was sie will)
   Gruppe          ← z. B. „Kinderzimmer“
      │  (ein Gerät ändert nur, was es will)
   Gerät           ← z. B. „Wohnzimmer-Tablet“
```

- Die **globale Vorgabe** ist das, was du unter [Einstellungen](settings.de.md) (die
  Schalter für Werbung und Schadsoftware) und [Listen](lists.de.md) (deine Feed-Abos
  und manuellen Einträge) festlegst.
- Eine **Gruppe** erbt die globale Vorgabe und ändert nur, was abweicht.
- Ein **Gerät** erbt seine Gruppe (oder, ohne Gruppe, die globale Vorgabe) und
  ändert nur, was abweicht.

Du gibst also nie alles neu ein — du stellst auf jeder Ebene nur den Unterschied
ein. Ein Gerät ohne Gruppe und ohne eigene Regeln verhält sich einfach wie die
globale Vorgabe.

## Schritt für Schritt

### Eine Gruppe anlegen

1. Öffne die Seite **Geräte**.
2. Scrolle unten zum Bereich **Gruppen**.
3. Tippe einen Namen ein (z. B. *Kinderzimmer*) und klick **Gruppe anlegen**.

Die Gruppe startet identisch zur globalen Vorgabe. Klick **Bearbeiten**, um ihr
Regeln zu geben.

### Einem Gerät eigene Regeln geben

1. Finde das Gerät auf der Seite **Geräte** und klick **Bearbeiten**. (Noch nicht
   benannt? Siehe [Geräte](devices.de.md) — benenne es zuerst, oder gleich beim
   Regeln-Setzen.)
2. **Gruppe** — ordne das Gerät optional einer Gruppe zu.
3. **Sperrlisten** — jede Liste zeigt drei Knöpfe:
   - **Erben** — nimm, was der Elternbereich entschieden hat (die Beschriftung zeigt,
     ob das an oder aus bedeutet).
   - **An** — diese Liste hier erzwingen.
   - **Aus** — diese Liste hier abschalten.
4. **Immer erlauben / Immer sperren** — Domains eintippen, eine pro Zeile.
5. Achte auf die Zeile **Hier aktiv**: Sie zeigt die Sperrlisten, die nach dem
   Zusammenfalten wirklich gelten.
6. Klick **Aktualisieren**.

### Beispiel: lockerere Regeln für ein Notebook

Deine globale Vorgabe blockt Werbung für alle, aber ein Arbeitswerkzeug streikt auf
deinem Notebook. Bearbeite das Notebook, setze **Werbung & Tracker → Aus**, und trag
die Domain des Werkzeugs zur Sicherheit unter **Immer erlauben** ein. Alle anderen
Geräte bekommen Werbung weiterhin geblockt.

### Beispiel: strengere Regeln für die Kinder

Lege eine Gruppe *Kinderzimmer* an, schalte eine zusätzliche strenge Liste für sie
**An**, und trag ein paar Spieleseiten unter **Immer sperren** ein. Ordne das Gerät
jedes Kindes der Gruppe zu. Änderst du die Gruppe einmal, folgt jedes Gerät darin.

## Gut zu wissen

- Ein Gerät wird über seine **IP-Adresse** oder über einen **Bereich** erkannt
  (siehe technischen Hinweis unten). Der genaueste Treffer gewinnt.
- Eine Gruppe zu löschen löscht ihre Geräte nicht — sie fallen einfach auf die
  globale Vorgabe zurück.
- Alles hier steckt in deiner [Sicherung](backup.de.md).
- Änderungen wirken sofort für neue Abfragen — nichts muss neu gestartet werden.

---

## Für Technik-Interessierte

**Wie ein Client erkannt wird.** Jeder Geräte-Eintrag hat ein *Ziel*: entweder eine
einzelne IP (`192.168.20.10`) oder einen CIDR-Bereich (`192.168.30.0/24`). Kommt eine
Abfrage herein, schaut JustDNS auf die Quell-IP des Clients und wählt das
**genaueste** passende Gerät per Longest-Prefix: eine exakte IP (ein /32 oder /128)
schlägt einen Bereich, ein engerer Bereich schlägt einen weiteren. Passt nichts,
bekommt der Client die globale Vorgabe. Ein Gerät, das nur einen Namen trägt (keine
Gruppe, keine Regeln), gilt als „keine Richtlinie“ und löst ebenfalls zur globalen
Vorgabe auf.

**Wie die Vererbung aufgelöst wird.** Jeder Bereich speichert nur ein *Delta*
gegenüber dem, was er erbt:

- `listsAdded` / `listsDisabled` — Listen-IDs, die gegenüber dem Elternbereich an-
  oder ausgeschaltet werden. Eine Listen-ID ist die ID eines Feed-Abos oder die
  eingebauten `ads` / `malware`.
- `allow` / `block` — einzelne Ad-hoc-Domains.

Zur Rebuild-Zeit (beim Start und nach jeder Änderung an Listen, Gruppen, Geräten
oder den Werbung/Schadsoftware-Schaltern) faltet JustDNS `global → Gruppe → Gerät`
zu **einer fertigen Richtlinie je Gerät** plus einer globalen Richtlinie. Der
DNS-Hot-Path läuft nie durch den Vererbungsbaum — er macht einen
Longest-Prefix-Lookup des Clients und dann eine normale Erlauben/Sperren-Einordnung
gegen eine fertige Richtlinie. Deshalb kostet die Filterung pro Client keine
messbare Verzögerung.

**Reihenfolge innerhalb einer Richtlinie.** In der aufgelösten Richtlinie gilt:
zuerst die Erlauben-Menge (eine Freigabe bricht ab, der Name wird aufgelöst), dann
die Sperrquellen in fester Reihenfolge — Schadsoftware, deine eigene manuelle
Sperrliste, Ad-hoc-Sperren der Gruppe, Ad-hoc-Sperren des Geräts, deine aktiven
Feeds, dann Werbung — die erste treffende Quelle gewinnt und ist das, was das
Protokoll als Grund meldet.

**Warum der Cache trotzdem geteilt wird.** Sperren, Rewriting und Safe Search laufen
*vor* dem Antwort-Cache, und der Cache speichert nur echte Upstream-Antworten, die
client-unabhängig sind. Zwei Clients mit unterschiedlichen Richtlinien teilen sich
also gefahrlos einen Cache: Die Richtlinie entscheidet, *ob* ein Client für einen
Namen in den Cache greift, nicht *was* die gecachte Antwort ist.

**Dauerhaftigkeit.** Gruppen und Geräte-Deltas liegen in `clients.json` auf dem
`/data`-Volume, jedes neue Feld mit Default, sodass eine ältere Datei unverändert
lädt und eine heute geschriebene Datei auch auf einem älteren Build lädt. Sie
überleben Neustarts und Image-Updates und sind Teil von Sicherung/Wiederherstellung.
