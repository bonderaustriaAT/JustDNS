# Phishing-Schutz

*Eine englische Fassung dieser Anleitung liegt in [phishing.md](phishing.md).*

Blocklisten arbeiten über Mitgliedschaft: Steht ein Name auf der Liste, wird er
geblockt. Gegen gezieltes **Phishing** hilft das kaum. Eine frisch registrierte
Nachahmer-Domain — ein kyrillisches „а" in `pаypal.com`, ein `micros0ft-login.com`,
ein als `xn--…` kodierter Punycode-Name — steht auf keiner Liste, weil es sie
gestern noch nicht gab. Der **Phishing-Schutz** ergänzt deshalb eine **berechnete**
Erkennungsschicht neben dem Blocker:

- **Nachahmer-Erkennung** — verwechselbare Zeichen (Homoglyphen) und Punycode,
  geprüft gegen eine Liste geschützter Marken. Vollständig lokal, keine externe
  Abfrage.
- **Neu registrierte Domains (NRD)** — ein Abo frischer Domains über die
  Listen-Infrastruktur, das **nicht** in die Sperre einfließt, sondern eigens in
  den Phishing-Schutz.

Die **Phishing-Schutz**-Seite liegt im Menü direkt unter *DNSSEC*.

## Warnen oder blockieren

Für jede der beiden Kategorien entscheidest du selbst zwischen zwei Modi:

- **Warnen** — die Antwort bleibt **unverändert**, der Treffer wird nur am
  Protokoll markiert und, wenn du magst, per Webhook gemeldet. Das ist der
  Standard: Eine Nachahmer-Prüfung hat Fehlalarme, Warnen ist also die sichere
  Wahl.
- **Blockieren** — der Name wird wie ein normaler Block abgewiesen, noch vor der
  Auflösung. Nimm das bewusst, vor allem bei NRD: Viele frische Domains sind
  harmlos.

„Warnen" ist dabei kein eigener Antwortweg, sondern nur ein Vermerk — genau wie
das DNSSEC-Urteil. Der Client bekommt dieselbe Antwort wie ohne Phishing-Schutz.

## Die Seite bedienen

- Ganz oben schaltet ein **Hauptschalter** den Phishing-Schutz als Ganzes ein
  oder aus. Ist er aus, prüft nichts, und die Karten darunter sind ohne Wirkung.
- Die Karte **Nachahmer-Erkennung** hat ein **Warnen/Blockieren**-Segment und
  darunter die geschützten Marken, nach Gruppe sortiert (*Tech & Mail*,
  *Banken AT*, *Banken DE*). Jede Marke ist ein Chip: Antippen schaltet eine
  mitgelieferte Marke ab oder wieder an. Über das Eingabefeld fügst du **eigene
  Marken** als Domain hinzu (etwa `meine-firma.at`) und entfernst sie wieder.
- Die Karte **Neu registrierte Domains** hat einen eigenen Schalter, ein
  **Warnen/Blockieren**-Segment und die **NRD-Feeds** zum Anklicken (siehe
  unten). Je mehr Fenster aktiv sind, desto weiter reicht die Abdeckung — und
  desto mehr Arbeitsspeicher kostet die Liste.
- Ein **Webhook-Schalter** meldet Treffer zusätzlich über die globalen
  [Benachrichtigungen](notifications.de.md).
- Darunter zeigt eine **Trefferliste** die letzten Phishing-Treffer aus dem
  Protokoll, jeweils mit Art (*Lookalike*, *Punycode*, *Neu registriert*) und
  Modus (*Gewarnt* oder *Blockiert*).

Ein Treffer taucht außerdem direkt im [Protokoll](query-log.de.md) auf: als
farbiges Badge an der Domain (bernstein beim Warnen, rot beim Blockieren) und als
eigenes Feld in der Detailansicht einer Zeile.

## Die NRD-Feeds anklicken

Die neu registrierten Domains kommen als Listen-Abo, aufgeteilt in Zeitfenster,
jedes mit einem Klick aktivierbar:

- **Letzte 7 Tage** — das kleinste Fenster, rund 40 MB im Speicher.
- **Tage 8 bis 14** — zusammen mit dem 7-Tage-Fenster die vollen letzten 14 Tage.
- **Tage 15 bis 21** und **Tage 22 bis 28** — für breitere Abdeckung, wenn der
  Speicher reicht.

Quelle ist das offen gepflegte **HaGeZi**-NRD-Projekt (GPLv3). Die Fenster sind
bewusst getrennt: Jede Datei bleibt klein genug, und du lädst nur, was du
wirklich brauchst. Auf kleinen Geräten reicht oft schon das 7-Tage-Fenster. Keines
der Fenster ist als empfohlen markiert, damit nicht versehentlich Dutzende MB
gezogen werden.

## Einschalten

Der Phishing-Schutz ist **standardmäßig aus**. Alles wird über das Dashboard
gesteuert — der Hauptschalter, die Modi, die Marken und die NRD-Feeds. Eine
Änderung wirkt **sofort**, ohne Neustart. Solange der Hauptschalter aus ist, wird
nichts geprüft und nichts protokolliert.

---

## Für die technisch Neugierigen

Der Phishing-Schutz ist kein zweiter Blocker im Hot-Path, sondern eine einzelne
Auswertung des angefragten Namens vor der Auflösung, genau an der Stelle des
bestehenden Block-Checks:

- **Punycode.** Ein Label mit `xn--` wird nach Unicode dekodiert und dann gegen
  die Marken abgeglichen. So fällt ein Name auf, der ausgeschrieben harmlos
  aussieht, dekodiert aber eine Marke nachahmt.
- **Homoglyph.** Für jedes registrierbare Label wird ein „Skeleton" gebildet —
  eine kuratierte, kompilierte Confusable-Tabelle bildet verwechselbare Zeichen
  auf eine Normalform ab (`0→o`, `1→l`, kyrillisch `а→a`, griechisch …). Stimmt
  das Skeleton mit dem einer geschützten Marke überein, die Domain ist aber
  **nicht** die echte Marken-Domain, ist das ein Treffer. Die echte Domain und
  legitime internationalisierte Namen lösen bewusst **keinen** Treffer aus.
- **NRD.** Die registrierbare Domain wird gegen die aktive NRD-Menge geprüft —
  reine Mengen-Mitgliedschaft, wie beim Blocker.

Die effektive Markenliste ist *mitgeliefert − abgeschaltete + eigene*. Der
Detektor hält seine Konfiguration in einem Atomic-Swap; ändert sich etwas am
Dashboard oder wird ein NRD-Feed aktualisiert, wird die Konfiguration neu gebaut
und ausgetauscht, ohne die Auflösung zu unterbrechen. Ein **Warn**-Treffer wird
wie das DNSSEC-Urteil zusammen mit dem Abfrage-Ereignis im DuckDB-Store abgelegt;
ein **Block**-Treffer schreibt die Block-Antwort und kehrt zurück, bevor der
Upstream überhaupt gefragt wird. Rewrites und eigene Namen sind ausgenommen.

Zum Speicher: Eine NRD-Menge ist groß. Das 14-Tage-Fenster liegt bei grob 86 MB,
die vollen 28 Tage bei mehreren Hundert MB im Arbeitsspeicher. Auf einer kleinen
Box lohnt es, mit dem 7-Tage-Fenster zu beginnen und nur bei Bedarf zu erweitern.

**Noch nicht verdrahtet:** Eine optionale Online-Prüfung des Registrierungsdatums
per **RDAP** ist in der Konfiguration vorgesehen (standardmäßig aus, der einzige
Weg, auf dem ein Domainname die Box verließe), aber im Dashboard noch nicht
freigeschaltet. Bis dahin bestimmt allein, welche NRD-Fenster du aktiviert hast,
wie weit „neu registriert" offline reicht.
