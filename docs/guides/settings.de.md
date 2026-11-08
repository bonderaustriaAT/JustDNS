# Einstellungen

*Eine englische Fassung dieser Anleitung liegt in [settings.md](settings.md).*

Auf der Seite **Einstellungen** legst du JustDNS' globales Verhalten fest — die Basis,
die für dein ganzes Netzwerk gilt. (Für Ausnahmen pro Gerät oder Gruppe siehe
[Richtlinien](policies.de.md).) Änderungen werden automatisch gespeichert, während du
sie machst.

## Schutz

- **Name dieser Instanz** — eine Beschriftung oben in der Leiste, praktisch, wenn du
  mehr als ein JustDNS betreibst. Nur Anzeige; wirkt sich nicht auf die Filterung aus.
- **Werbung & Tracker** — die eingebaute Werbe-/Tracker-Sperrmenge für die globale
  Vorgabe an- oder ausschalten.
- **Schadsoftware & Phishing** — die eingebaute Schadsoftware-Sperrmenge an- oder
  ausschalten.
- **Safe Search** — Google, YouTube, Bing und DuckDuckGo netzweit auf ihre
  gefilterten Endpunkte zwingen.
- **Block-Antwort** — womit ein geblockter Name beantwortet wird:
  - **Null-IP** (`0.0.0.0` / `::`) — der Standard; das Gerät bekommt eine tote Adresse.
  - **NXDOMAIN** — dem Gerät wird gesagt, der Name existiere nicht.

  Für die meisten Setups ist Null-IP passend; wann NXDOMAIN hilft, steht im
  technischen Hinweis.

> Für deine eigenen Sperrlisten und Feed-Abos siehe [Listen](lists.de.md). Die
> Schalter Werbung und Schadsoftware hier sind die An/Aus-Schalter der beiden
> eingebauten Kategorien.

## Resolver & Tempo

- **Tempo-Profil** — ein Schalter, der Datenschutz gegen Tempo abwägt:
  **Privatsphäre**, **Ausgewogen** (empfohlen) oder **Tempo**. Es wählt die Resolver
  und wie eifrig Antworten gecacht werden.
- **Upstream verschlüsseln** — mit deinen Resolvern über DNS-over-TLS statt
  Klartext-DNS sprechen, sodass deine Abfragen unterwegs privat sind.
- **Eigene Resolver** — schalte dies an, um eigene Resolver statt der eingebauten zu
  nutzen. Jeder kann Klartext-DNS, DNS-over-TLS (DoT) oder DNS-over-HTTPS (DoH) sein.
- **Upstream-Modus** — wie JustDNS mehrere Resolver gleichzeitig nutzt (um die Wette
  abfragen, Failover, ausbalancieren oder nach Regeln). Lass ihn auf dem Standard,
  außer du weißt, dass du etwas anderes willst.

## Protokollierung & Pflege

- **Anfrage-Protokoll** — ob JustDNS jede Abfrage aufzeichnet (nötig für die Seite
  [Protokoll](query-log.de.md) und die Statistik).
- **Protokolle aufbewahren** — wie viele Tage Verlauf behalten werden, bevor alte
  Einträge entfernt werden.
- **Aktiv-Fenster** — wie lange ein Gerät nach seiner letzten Abfrage auf der Seite
  [Geräte](devices.de.md) noch als „gerade aktiv“ gilt (Standard 5 Minuten).
- **IPv6** — ob JustDNS AAAA-(IPv6-)Records beantwortet.
- **DNS-Cache leeren** — jetzt jede gecachte Antwort verwerfen, sodass die nächste
  Abfrage pro Name frisch ist. Praktisch nach Änderungen an Listen oder Resolvern.

Einige fortgeschrittene Schalter sind verborgen, bis du den **Experten-Modus**
einschaltest (eine Einstellung pro Browser).

---

## Für die technisch Neugierigen

**Block-Antwort.** Null-IP (`0.0.0.0` / `::`) gibt eine gültige-aber-tote A/AAAA-Antwort
zurück, sodass Clients schnell scheitern, ohne Auflösungsfehler. NXDOMAIN signalisiert
„kein solcher Name“, was manche Clients und Browser eleganter behandeln (gar kein
Verbindungsversuch), aber einige Apps zu Wiederholungen oder seltsamem Verhalten
bringen kann. Beides ist gültig; Null-IP ist der sichere Standard.

**Tempo-Profil vs. Upstream-Modus.** Das Tempo-Profil ist der eine Regler, den die
meisten nutzen: Es wählt einen Resolver-Pool und die Cache-Aggressivität
(Vorab-Erneuerung, Serve-Stale). Der Upstream-Modus ist die feinere Steuerung, *wie*
mehrere Resolver genutzt werden — um die Wette abfragen und die erste Antwort nehmen
(geringste Latenz), Failover-Reihenfolge, Ausbalancieren oder regelbasierte Auswahl.
`encryptUpstream` hebt den eingebauten Pool auf DoT; `customUpstreams` ersetzt den
Pool ganz durch deine eigenen Einträge, jeder mit eigenem Transport (Klartext / DoT /
DoH), Port und optionalem TLS-Servernamen.

**Der Cache.** Nur echte Upstream-Antworten werden gecacht, und sie sind
client-unabhängig — das erlaubt den Richtlinien pro Client, sich einen Cache zu
teilen. Filterung, Rewriting und Safe Search laufen alle vor dem Cache, sodass ein
geblockter oder umgeschriebener Name ihn nie verunreinigt. „DNS-Cache leeren“ leert
ihn sofort; eine Profil- oder Resolver-Änderung schreibt gecachte Einträge nicht
rückwirkend um — leere also den Cache, wenn du sofort einen sauberen Stand willst.

**Dauerhaftigkeit.** Alle Einstellungen liegen in `config.json` auf dem `/data`-Volume
und werden beim Start geladen. Unbekannte Felder werden ignoriert, sodass eine von
einem neueren oder älteren Build geschriebene Konfiguration weiter lädt — so wird das
abgeschaffte „Schutz-Stärke“-Feld früherer Versionen still verworfen, ohne deine Datei
zu beschädigen.
