# JustDNS

**Das DNS deines ganzen Netzwerks — gefiltert, verschlüsselt und selbst gehostet.**

*Auf Englisch lesen: [README.md](README.md).*

JustDNS ist ein filternder DNS-Forwarder, den du selbst betreibst. Richte deinen
Router oder deine Geräte darauf aus, und jede Namensauflösung läuft durch eine
Stelle: Werbung, Tracker und Schad-Domains werden blockiert, den Rest
beantworten deine gewählten Resolver über eine verschlüsselte Verbindung, und ein
aufgeräumtes Web-Dashboard zeigt dir genau, was in deinem Netz passiert — ohne
fremde Konten, ohne Cloud, und ohne dass Daten deine Maschine verlassen.

> Status: aktiv in Entwicklung, vor 1.0. Es läuft und ist schon nützlich;
> Einstellungen und Details können sich zwischen Versionen noch ändern.

## Highlights

- **Blockt Werbung, Tracker und Schadsoftware** von Haus aus, mit kuratierten
  [Hagezi](https://github.com/hagezi/dns-blocklists)-Listen. Ein Schalter für die
  **Schutz-Stärke** (Aus / Minimal / Standard / Extrem) aktiviert die passenden
  Listen für dich; eigene Allow-/Block-Einträge oder Listen-Abos jederzeit
  ergänzbar.
- **Privat von Anfang an.** Standardmäßig datenschutzfreundliche EU-Resolver
  (DNS4EU, Quad9) über **DNS-over-TLS**, um die Wette abgefragt. Nutze eigene
  Resolver — Klartext, DoT oder DoH — oder wähle ein Profil **Privatsphäre /
  Ausgewogen / Tempo**.
- **Schnell.** Ein Antwort-Cache mit Vorab-Erneuerung und Serve-Stale sowie warme
  verschlüsselte Verbindungen halten wiederholte Abfragen nahezu sofort.
- **Lokale Rewrites.** Eigene Namen aus festen Adressen beantworten (A / AAAA) —
  praktisch für ein NAS, eine Homelab-Kiste oder Split-Horizon.
- **Safe Search.** Google, YouTube, Bing und DuckDuckGo netzweit auf ihre
  gefilterten Endpunkte zwingen.
- **Alles im Blick.** Ein Live-Dashboard und ein durchsuchbares Anfrage-Protokoll
  (was gefragt wurde, von welchem Gerät, wie es beantwortet wurde und worauf es
  auflöste).
- **Geführte Einrichtung.** Ein Assistent beim ersten Start führt durch
  Administratorkonto, Schutz-Stärke, Resolver und Aufbewahrung. Oberfläche auf
  **Deutsch und Englisch**.

## Schnellstart (Docker)

Image bauen und starten:

```bash
docker build -t justdns .
```

```bash
docker run -d --name justdns \
  -p 53:53/udp -p 53:53/tcp \
  -p 8080:8080/tcp \
  -v justdns-data:/data \
  justdns
```

Dann **http://<dein-host>:8080** öffnen — der Einrichtungs-Assistent übernimmt ab
hier. Richte das DNS eines Geräts (oder deines Routers) auf die IP des Hosts, um
loszulegen.

Der Zustand — Protokoll, Konfiguration, Konto, Listen und Caches — liegt unter
`/data`; das Volume bewahrt ihn über Neustarts und Updates hinweg auf.

## Konfiguration

Nach der Einrichtung ist alles über die Weboberfläche konfigurierbar. Einiges
wird beim Start über Umgebungsvariablen gesetzt:

| Variable | Standard | Wirkung |
|---|---|---|
| `JUSTDNS_LISTEN` | `0.0.0.0:53` | Adresse/Port, auf dem der DNS-Server lauscht |
| `JUSTDNS_WEB_LISTEN` | `0.0.0.0:8080` | Adresse/Port für Weboberfläche und API |
| `JUSTDNS_IPV6` | `false` | AAAA-Anfragen (IPv6) beantworten |
| `JUSTDNS_CACHE_CAPACITY` | `10000` | Maximale Anzahl zwischengespeicherter Antworten |
| `JUSTDNS_UPSTREAMS` | *(EU-Standard)* | Kommagetrennte Klartext-Resolver (`ip` oder `ip:port`) statt der mitgelieferten — praktisch, um auf einen lokalen Resolver zu zeigen |

Der dauerhafte Zustand liegt im Image standardmäßig unter `/data` (Datenbank,
`config.json`, Konto und Listen). Einmal gesetzt, ist die Konfigurationsdatei die
maßgebliche Quelle — die Umgebung befüllt nur den ersten Start.

## Aus dem Quellcode bauen

Benötigt eine Rust-Toolchain (Edition 2024, rustc ≥ 1.85) und Node.js für die
Weboberfläche. Die UI wird zuerst gebaut und in die Binärdatei eingebettet:

```bash
npm --prefix crates/justdns-web/ui ci
npm --prefix crates/justdns-web/ui run build
cargo build --release --bin justdns
```

Die Release-Binärdatei ist eigenständig (TLS-Wurzelzertifikate sind einkompiliert).
Oder nutze einfach das `Dockerfile`, das beide Schritte für dich erledigt.

## Wie es funktioniert

Jede Anfrage läuft durch eine einzige Pipeline: Zuerst gewinnt ein lokaler
**Rewrite**, dann die **Block**-Prüfung (ein gesperrter Name wird vor jeder
Auflösung abgelehnt), dann der **Cache** und schließlich die **Upstream**-Resolver
über die gewählte Übertragungsart. Jede behandelte Anfrage wird in einem lokalen
DuckDB-Protokoll festgehalten, das das Dashboard speist — und die Maschine nie
verlässt.

## Fahrplan & Änderungsverlauf

- Wohin sich JustDNS entwickelt: [ROADMAP.de.md](ROADMAP.de.md)
  ([English](ROADMAP.md)).
- Was in jeder Version neu ist: [CHANGELOG.de.md](CHANGELOG.de.md)
  ([English](CHANGELOG.md)).

## Lizenz

[AGPL-3.0-or-later](LICENSE). Beiträge werden unter derselben Lizenz angenommen;
siehe [CONTRIBUTING.md](CONTRIBUTING.md).
