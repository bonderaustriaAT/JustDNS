# Blockierte Dienste

*Eine englische Fassung dieser Anleitung liegt unter [blocked-services.md](blocked-services.md).*

Manches, was du sperren willst, ist keine einzelne Domain, sondern ein ganzes Produkt —
TikTok, YouTube, Netflix, ein Spiele-Store. Jedes davon erreicht das Internet über eine
Handvoll Domains, und es von Hand zu sperren heißt, sie alle aufzuspüren. **Blockierte
Dienste** nimmt dir das ab: ein kuratierter Katalog mit einem Schalter je Dienst, der
die Domains bündelt, auf die der Dienst angewiesen ist.

## Der globale Katalog

Öffne die Seite **Blockierte Dienste** (unter *Regeln*, nur Admin). Du bekommst je Dienst
eine Karte, gruppiert in Kategorien — soziale Netzwerke, Video, Messenger, Gaming, Musik.
Filtere nach Kategorie oder suche nach dem Namen.

Legst du einen Schalter um, ist dieser Dienst standardmäßig für das **ganze Netzwerk**
gesperrt, sofort. Zurückgelegt ist er wieder freigegeben. Der Zähler oben sagt dir, wie
viele Dienste global gesperrt sind.

Diese Seite setzt den **globalen Standard**, die Grundlinie, die jeder Client erbt —
dieselbe Rolle, die [Block/Allow](lists.de.md) für Listen spielt.

## Je Gerät, Gruppe und Zeitplan

Der globale Schalter ist selten die ganze Geschichte. Oft willst du einen Dienst überall
sperren *außer* auf dem eigenen Handy, oder für die Kinder-Gruppe, oder nur während der
Hausaufgabenzeit. Dafür sind die Schalter je Ebene da.

In den [Geräte- und Gruppen-Editoren](policies.de.md) und im [Zeitplan-Editor](schedules.de.md)
zeigt jeder Dienst dieselbe Drei-Zustand-Steuerung wie eine Sperrliste:

- **Erben** — der übergeordneten Ebene folgen (globaler Standard bzw. Gruppe).
- **An** — den Dienst hier sperren, auch wenn er oben aus ist.
- **Aus** — den Dienst hier freigeben, auch wenn er oben an ist.

So kannst du TikTok global sperren und für dein eigenes Gerät auf **Aus** stellen, oder
ihn global in Ruhe lassen und mit einem [Zeitplan](schedules.de.md) für eine einzelne
Gruppe zwischen 18:00 und 20:00 Uhr auf **An** setzen.

## Gut zu wissen

- **Eine Freigabe gewinnt immer.** Steht eine Domain auf deiner [Allow-Liste](lists.de.md),
  wird sie durchgelassen, selbst wenn ein gesperrter Dienst sie fangen würde.
- Sperrt ein Dienst eine Abfrage, nennt das [Abfrage-Protokoll](query-log.de.md) den
  Dienst (zum Beispiel `TikTok`) als Grund — eine Sperre bleibt also nie ein Rätsel.
- Welche Dienste global gesperrt sind, gehört zu deinem [Backup](backup.de.md).
- Alles wirkt live auf neue Abfragen — kein Neustart.
- Dienste sind **standardmäßig aus**: Eine frische Installation sperrt hier nichts, bis
  du es sagst.

## Gut zu wissen zur Abdeckung

Der Katalog zielt auf die Domains, die ein Dienst wirklich braucht, nicht auf jede, die er
je berührt hat. Das hält den Kollateralschaden klein, aber eine hartnäckige App erreicht
Inhalte womöglich noch über ein geteiltes CDN, das der Katalog in Ruhe lässt. Für eine
lückenlose Sperre kombiniere den Dienst-Schalter mit einer aggressiven
Sperr[liste](lists.de.md).

---

## Für die technisch Neugierigen

**Ein Mechanismus, kein neuer.** Ein Dienst ist nur eine weitere Block-Quelle in der
[Policy-Engine](policies.de.md). Der Katalog ist in die Binärdatei einkompiliert (wie die
mitgelieferten Ads-/Malware-Starter-Sets) und braucht daher keinen Netzzugang. Jeder Dienst
trägt ein kleines [`DomainSet`](lists.de.md) seiner registrierbaren Wurzeln, exakt oder
per Subdomain gematcht — ein Eintrag für `tiktok.com` passt auf `tiktok.com` und jedes
`*.tiktok.com`.

**Wie ein Schalter reist.** Jeder Dienst hat eine Listen-Objekt-ID der Form `svc:<id>`
(zum Beispiel `svc:tiktok`). Der globale Schalter speichert die nackte ID in
`settings.blocked_services`; ein Schalter je Ebene schreibt die `svc:<id>`-ID in den
Policy-Delta dieser Ebene — in denselben `listsAdded`/`listsDisabled`-Kanal, den die
Sperrlisten nutzen. Nichts im Policy-Editor, in der Vererbungs-Rechnung oder im
Zeitplan-Falten unterscheidet einen Dienst von einer Liste, also kommen Per-Client-Auflösung,
Live-Rebuild, „Freigabe gewinnt" und Zeitpläne geschenkt dazu.

**Auflösungsreihenfolge.** Beim Rebuild wird jeder aktive Dienst zu einer Block-Quelle, die
mit dem Dienstnamen beschriftet und zwischen die Feeds einsortiert wird. Die Engine prüft
weiterhin zuerst das Allow-Set (eine Freigabe bricht sofort ab), dann die Block-Quellen; der
erste Treffer ist der Grund, den das [Abfrage-Protokoll](query-log.de.md) festhält.

**Katalog erweitern.** Die Liste liegt in `crates/justdns-filter/src/services.rs` als
statische Tabelle von `Service { id, name, category, domains }`. Einen Dienst hinzuzufügen
ist ein Eintrag dort plus seine Domains; ein Test prüft, dass jede gelistete Domain den
Regel-Parser übersteht — ein Tippfehler bricht so den Build, statt stillschweigend eine
Domain zu verschlucken.
