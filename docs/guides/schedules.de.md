# Zeitpläne

*Eine englische Fassung dieser Anleitung liegt in [schedules.md](schedules.md).*

Richtlinien gelten in JustDNS normalerweise **rund um die Uhr**. **Zeitpläne**
ergänzen die fehlende Dimension — die **Zeit**. Ein Zeitplan ist ein
wiederkehrendes Wochenfenster (ein paar Wochentage plus Start- und Endzeit), das
im geöffneten Zustand ein Ziel entweder komplett **pausiert** („Internet aus") oder
ein **zusätzliches Filter-Delta** anwendet (mehr Listen oder einzelne gesperrte bzw.
erlaubte Domains). Außerhalb des Fensters tut es nichts.

Du verwaltest das alles auf der Seite **Zeitpläne**.

## Woraus ein Zeitplan besteht

- **Name** — damit du ihn in der Liste wiedererkennst (z. B. *Schlafenszeit*).
- **Aktiv** — der Hauptschalter. Ein deaktivierter Zeitplan greift nie, egal was
  seine Zeiten sagen.
- **Wochentage** — die Tage, an denen das Fenster öffnet. Bei einem Fenster über
  Mitternacht ist das der **Starttag** (siehe unten).
- **Von / Bis** — das Fenster als lokale `HH:MM`-Zeiten.
- **Ziel** — für wen es gilt:
  - **Alle (global)** — das ganze Netzwerk.
  - **Gruppe** — eine Gerätegruppe.
  - **Gerät** — ein einzelnes benanntes Gerät.
- **Wirkung** — was es im offenen Fenster tut:
  - **Pause** — sperrt jede Anfrage für das Ziel. Deine **Immer-erlauben**-Liste
    gewinnt weiterhin, erlaubte Namen bleiben also erreichbar (praktisch für ein
    Schulportal oder eine Notfallnummer).
  - **Listen/Domains** — legt ein zusätzliches Delta über die normale Filterung:
    weitere Sperrlisten einschalten oder einzelne Domains sperren/erlauben, genau
    wie bei einer Geräte- oder Gruppenrichtlinie.

## Wie Zeitpläne mit Richtlinien zusammenspielen

Ein Zeitplan ersetzt deine normale Filterung nicht — er wird im offenen Fenster
**obendrauf** angewendet:

```
Globale Vorgabe  →  Gruppe  →  Gerät        (deine normale Richtlinie, immer aktiv)
        +  aktive Zeitpläne für dieses Ziel  (nur im offenen Fenster)
```

- Eine **Pause** gewinnt immer: solange für ein Ziel ein Pausenfenster offen ist,
  ist alles gesperrt (abzüglich der Allow-Liste), egal was sonst eingestellt ist.
- Ein **Listen/Domains**-Zeitplan ergänzt seine Listen und Domains zu dem, was das
  Ziel ohnehin hat. Ein von ihm erlaubter Name gewinnt weiterhin über eine Sperre,
  wie überall in JustDNS.
- Mehrere Zeitpläne können gleichzeitig dasselbe Ziel betreffen; ihre Wirkungen
  addieren sich, und eine aktive Pause dominiert.

## Zeitzone

Fenster werden in der **Zeitzone** ausgewertet, die du unter
[Einstellungen](settings.md) → *Zeitzone für Zeitpläne* festlegst. Wähle deine
lokale IANA-Zone (z. B. `Europe/Vienna`), damit „22:00–07:00" die lokale
Schlafenszeit meint und das Fenster der Sommerzeit-Umstellung automatisch folgt.
Unbekannte Werte fallen auf UTC zurück. Standard ist UTC.

## Fenster über Mitternacht

Ist **Bis** früher als (oder gleich) **Von**, läuft das Fenster **über
Mitternacht**. Der Wochentag-Satz gilt für den **Starttag**. Ein Zeitplan
*Schlafenszeit* mit **22:00–07:00** an **Mo–Fr** deckt also Montag 22:00 bis
Dienstag 07:00 ab, und so weiter bis von Freitagnacht in den Samstagmorgen —
genau das, was „Schulnächte" bedeutet. Die Seite zeigt in diesem Fall einen
*über Nacht*-Hinweis.

## Schritt für Schritt

### Zeitplan anlegen

1. Öffne die Seite **Zeitpläne** und klicke auf **Zeitplan anlegen**.
2. Gib einen **Namen** ein.
3. Wähle die **Wochentage**, an denen er laufen soll (mindestens einen).
4. Setze **Von** und **Bis**. Achte auf den Hinweis: er sagt dir, ob das Fenster
   am selben Tag bleibt oder über Mitternacht geht, und in welcher Zeitzone es
   gelesen wird.
5. Wähle ein **Ziel**: alle, eine Gruppe oder ein Gerät.
6. Wähle die **Wirkung**:
   - **Pause** — mehr ist nicht einzustellen.
   - **Listen/Domains** — der Richtlinien-Editor erscheint; schalte Listen ein/aus
     und ergänze Immer-erlauben-/Immer-sperren-Domains, genau wie auf der
     Geräte-Seite.
7. Klicke auf **Anlegen**.

### Bearbeiten, deaktivieren oder löschen

- **Bearbeiten** öffnet das Formular erneut. **Speichern** übernimmt die Änderungen
  sofort.
- Der **Aktiv**-Schalter in jeder Zeile deaktiviert einen Zeitplan, ohne ihn zu
  löschen — praktisch während der Ferien.
- **Löschen** entfernt ihn nach einer Rückfrage.

Änderungen wirken sofort für neue Anfragen — es gibt nichts neu zu starten.

### Beispiel: Internet aus zur Schlafenszeit

*Schlafenszeit*, **Mo–Fr**, **22:00–07:00**, Ziel **Gruppe: Kinderzimmer**, Wirkung
**Pause**. Jedes Gerät in dieser Gruppe ist in Schulnächten über Nacht gesperrt.
Trage eine Lese-App-Domain in die Immer-erlauben-Liste der Gruppe ein, falls sie
erreichbar bleiben soll.

### Beispiel: keine sozialen Medien in der Fokuszeit

*Fokus*, **Mo–Fr**, **09:00–12:00**, Ziel **Gerät: Büro-PC**, Wirkung
**Listen/Domains**, mit den Social-Media-Domains unter **Immer sperren**. Der
restliche Tag bleibt unberührt, und andere Geräte sehen die Regel nie.

## Gut zu wissen

- Ein Zeitplan, der auf eine gelöschte Gruppe oder ein gelöschtes Gerät zeigt, wird
  einfach ignoriert, bis du ihn korrigierst oder entfernst — die Auflösung bricht
  nie ab.
- Alles hier ist in deiner [Sicherung](backup.md) enthalten.
- Zeitpläne verursachen keine messbare Latenz: sie werden in fertige Richtlinien
  eingefaltet, und der DNS-Hot-Path matcht weiterhin nur eine fertige Richtlinie
  (siehe unten).

---

## Für die technisch Neugierigen

**Wann „jetzt" entschieden wird.** JustDNS hält die aktuelle Lokalzeit in deiner
konfigurierten IANA-Zone (über `chrono` + `chrono-tz`). Ein Zeitplan ist *jetzt
aktiv*, wenn er aktiviert ist, der heutige Wochentag (bzw. bei einem Fenster über
Nacht der des Starttags) in seinem Satz liegt und die aktuelle Lokalzeit in
`[Start, Ende)` fällt. Bei einem Fenster über Nacht (`Ende <= Start`) liegt eine
Zeit drin, wenn sie an einem gelisteten Tag `>= Start` ist **oder** am Tag nach
einem gelisteten Tag `< Ende`. Ein Fenster mit `Start == Ende` deckt den ganzen
gelisteten Tag ab.

**Wie es den Hot-Path erreicht.** Aktive Zeitpläne werden in denselben
Richtlinien-Rebuild eingefaltet, der global/Gruppe/Gerät-Vererbung verarbeitet:
ihre Deltas werden zum passenden Ziel addiert, und eine Pause macht die aufgelöste
Richtlinie zu einer *Alles-sperren*-Richtlinie (die Allow-Liste durchsticht sie
weiterhin). Heraus kommt eine fertige Richtlinie pro Gerät plus eine globale — der
DNS-Hot-Path wertet nie direkt einen Zeitplan aus, die Kosten pro Anfrage bleiben
also unverändert.

**Was die Faltung erneut auslöst.** Zwei Auslöser: jede Änderung an einem Zeitplan
(anlegen/bearbeiten/löschen/umschalten) baut sofort neu, und ein Minuten-Ticker
berechnet den Satz der aktuell offenen Zeitpläne neu und baut **nur dann neu**, wenn
sich dieser Satz ändert (ein Fenster hat gerade geöffnet oder geschlossen).
Minutengenauigkeit passt zu den `HH:MM`-Fenstern und ist robust über
Sommerzeit-Umstellungen hinweg — keine fragile „nächste-Grenze"-Rechnung. Die
Zeitzone wird bei jedem Tick frisch gelesen, ein Wechsel in den Einstellungen wirkt
also ohne Neustart.

**Pause-Semantik.** Eine Pause wird als Richtlinie umgesetzt, deren Block-Quellen
geleert sind und deren „Alles sperren"-Flag gesetzt ist, beschriftet mit dem Namen
des Zeitplans. Die Klassifizierung prüft trotzdem zuerst die Allow-Liste, sodass
immer erlaubte Namen auch während einer Pause auflösen; alles andere wird mit deiner
konfigurierten Blockantwort beantwortet (standardmäßig die Null-IP, oder NXDOMAIN).

**Persistenz.** Zeitpläne liegen in `schedules.json` auf dem `/data`-Volume. Jedes
neue Feld hat einen Standardwert, eine ältere Datei lädt also unverändert und eine
heute geschriebene Datei lädt auch auf einem älteren Build. Sie überstehen Neustarts
und Image-Updates und sind Teil von Sicherung/Wiederherstellung (ältere Sicherungen
ohne `schedules`-Abschnitt werden einfach ohne Zeitpläne eingespielt).
