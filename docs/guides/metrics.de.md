# Prometheus-Metriken

*Die englische Fassung dieses Leitfadens steht in [metrics.md](metrics.md).*

JustDNS kann seine Betriebskennzahlen — Abfragezahlen, Cache- und Listengrößen,
DNSSEC-Urteile, Listener-Zustand und Latenz — unter **`GET /metrics`** im
[Prometheus](https://prometheus.io/)-Textformat bereitstellen, sodass du sie in ein
bestehendes Monitoring- und Alarmierungssystem einlesen kannst. Das ist das
maschinenlesbare Gegenstück zur Seite [System & Diagnose](system-logs.de.md).

Der Endpunkt läuft über den **Web-Port** (denselben `8080` wie das Dashboard), ein
zusätzlicher Port muss also nicht geöffnet werden.

## Einschalten

Der Endpunkt ist **standardmäßig aus**. Am einfachsten schaltest du ihn im Dashboard
ein: Öffne **Einstellungen → Netzwerk**, finde **Metrics-Token (Prometheus)**, drücke
**Token generieren**, kopiere es — es wird automatisch gespeichert. Danach ist
`/metrics` erreichbar und der Scraper muss dieses Token mitschicken. Leerst du das
Feld, ist der Endpunkt wieder aus. Das Token liegt bei der übrigen Konfiguration,
verschlüsselt im Ruhezustand.

Ohne gesetztes Token antwortet `/metrics` mit **404** (als gäbe es ihn nicht). Mit
Token muss ein Scraper es als `Authorization: Bearer <token>` mitschicken, sonst
kommt **401**. Ein Prometheus-Scraper kann keine Anmeldesitzung halten — deshalb hat
der Endpunkt ein eigenes Token statt der Dashboard-Konten.

### Stattdessen per Umgebungsvariable setzen

Wenn du das Token lieber nicht im Dashboard hältst, setze es als Umgebungsvariable am
Container:

```
JUSTDNS_METRICS_TOKEN=<eine lange Zufallszeichenkette>
```

Nimm einen langen, zufälligen Wert (zum Beispiel `openssl rand -hex 32`). Ein im
Dashboard gesetztes Token hat Vorrang; die Umgebungsvariable greift nur, solange kein
Dashboard-Token gesetzt ist — bestehende Installationen laufen also unverändert
weiter. Halte das Token geheim — wer es hat, kann deine Betriebskennzahlen lesen.

## Aus Prometheus abholen

```yaml
scrape_configs:
  - job_name: justdns
    metrics_path: /metrics
    authorization:
      type: Bearer
      credentials: "<dasselbe Token>"
    static_configs:
      - targets: ["dein-justdns-host:8080"]
```

## Was bereitgestellt wird

Alle Namen tragen das Präfix `justdns_`.

| Metrik | Typ | Bedeutung |
| --- | --- | --- |
| `justdns_build_info{version}` | gauge | Immer 1; die laufende Version steht im Label. |
| `justdns_uptime_seconds` | gauge | Sekunden seit Betriebsstart des Servers. |
| `justdns_queries_total` | counter | Insgesamt bearbeitete DNS-Abfragen. |
| `justdns_cache_hits_total` | counter | Aus dem Antwort-Cache beantwortete Abfragen. |
| `justdns_queries_upstream_total` | counter | An einen Upstream weitergereichte Abfragen. |
| `justdns_queries_local_total` | counter | Lokal beantwortete Abfragen (Rewrites, unterdrückte Records). |
| `justdns_queries_blocked_total` | counter | Von der Blockier-Engine abgewiesene Abfragen. |
| `justdns_responses_nxdomain_total` | counter | Als NXDOMAIN zurückgegebene Antworten. |
| `justdns_responses_servfail_total` | counter | Als SERVFAIL zurückgegebene Antworten. |
| `justdns_dnssec_verdicts_total{verdict}` | counter | DNSSEC-Urteile nach Ergebnis (`secure`/`insecure`/`bogus`/`indeterminate`). |
| `justdns_cache_entries` | gauge | Aktuell im Antwort-Cache liegende Einträge. |
| `justdns_active_clients` | gauge | Clients, die im aktiven Fenster abgefragt haben. |
| `justdns_list_entries{list}` | gauge | Geladene Listeneinträge nach Herkunft (`ads`/`malware`/`allow`/`custom`). |
| `justdns_listener_up{listener}` | gauge | Ob ein Listener bedient, `1`/`0` (`udp`/`tcp`/`dot`/`doh`). |
| `justdns_feed_last_refresh_timestamp_seconds` | gauge | Unix-Zeit der letzten Listenaktualisierung. |
| `justdns_feed_last_refresh_success` | gauge | Ob die letzte Listenaktualisierung gelang, `1`/`0`. |
| `justdns_query_duration_avg_ms` | gauge | Mittlere Bearbeitungszeit über den letzten Tag. |
| `justdns_query_duration_ms{quantile}` | gauge | Median und 95. Perzentil der Bearbeitungszeit über den letzten Tag. |
| `justdns_resolver_queries_total{resolver}` | gauge | Antworten je Upstream über den letzten Tag. |
| `justdns_resolver_duration_ms{resolver,quantile}` | gauge | Median/p95 der Bearbeitungszeit je Upstream über den letzten Tag. |

---

## Für die technisch Neugierigen

**Billig abzuholen.** Jede Zahl kommt aus denselben In-Memory-Griffen, die auch das
Dashboard liest — die laufenden Summen des Abfragespeichers, die Latenz-Zusammen­
fassung des Tages sowie die Cache-, Blocker- und Laufzeitstatus-Griffe — ein Scrape
führt also nie eine DuckDB-Abfrage aus. Die Counter sind Prozess-Lebenszeit-Summen,
beim Start aus dem Log rehydriert; sie überstehen einen Neustart, statt auf null zu
springen (daran denken bei `rate()` — es verkraftet Counter-Resets, aber bei einem
sauberen Neustart gibt es hier keinen).

**Latenz ist eine Zusammenfassung, kein Histogramm.** JustDNS hält Mittel, Median und
p95 des Tages vor, nicht die rohen Buckets — die Dauer-Metriken sind daher einfache
Gauges mit genau diesen Werten (`quantile="0.5"` / `"0.95"`), kein Prometheus-
`histogram`. Du liest die Quantile also direkt ab und wendest **kein**
`histogram_quantile()` darauf an. Ein echtes Bucket-Histogramm bräuchte Buchführung je
Abfrage im Hot-Path und ist ein möglicher späterer Schritt.

**Das Token.** Es wird in konstanter Zeit verglichen, ein falsches Token lässt sich
also nicht über die Antwortzeit erraten. Der Endpunkt ist die einzige Stelle, an der
die Version einem Aufrufer ohne Anmeldesitzung gezeigt wird — deshalb ist er
abgesichert; das offene [`/api/health`](system-logs.de.md) lässt die Version bewusst
weg.
