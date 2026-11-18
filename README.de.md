# JustDNS

**Das DNS deines ganzen Netzwerks — gefiltert, verschlüsselt und selbst gehostet.**

<p align="center">
  <a href="https://demo.justdns.justfuture.at"><img src="https://img.shields.io/badge/Live_Demo-demo.justdns.justfuture.at-2ea043?style=for-the-badge" alt="Live-Demo von JustDNS"></a>
</p>
<p align="center"><em>Schau dir fast alles direkt im Browser an. Ohne Installation, ohne Einrichtung.</em></p>

[![Release](https://img.shields.io/github/v/tag/bonderaustriaAT/JustDNS?label=release&sort=semver&color=2ea043)](https://github.com/bonderaustriaAT/JustDNS/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/bonderaustriaAT/JustDNS/image.yml?branch=main&label=build)](https://github.com/bonderaustriaAT/JustDNS/actions/workflows/image.yml)
[![Container](https://img.shields.io/badge/container-ghcr.io-2496ED?logo=docker&logoColor=white)](https://github.com/bonderaustriaAT/JustDNS/pkgs/container/justdns)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org)

*Auf Englisch lesen: [README.md](README.md).*

JustDNS ist ein filternder DNS-Forwarder, den du selbst betreibst. Richte deinen
Router oder deine Geräte darauf aus, und jede Namensauflösung läuft durch eine
Stelle: Werbung, Tracker und Schad-Domains werden blockiert, den Rest
beantworten deine gewählten Resolver über eine verschlüsselte Verbindung, und ein
aufgeräumtes Web-Dashboard zeigt dir genau, was in deinem Netz passiert — ohne
fremde Konten, ohne Cloud, und ohne dass Daten deine Maschine verlassen.

> Status: aktiv in Entwicklung, vor 1.0. Es läuft und ist schon nützlich;
> Einstellungen und Details können sich zwischen Versionen noch ändern.

<p align="center">
  <img src="docs/assets/dashboard.gif" alt="Das JustDNS-Dashboard: hochtickende Live-Zähler, ein Anfrageraten-Diagramm und 24-Stunden-Statistik" width="820">
</p>

<p align="center">
  <img src="docs/assets/privacy-dashboard.png" alt="Das Privacy-Dashboard: welche Firmen jedes Gerät kontaktiert, der geblockte Anteil und eine Aufschlüsselung pro Gerät" width="820">
</p>

<p align="center">
  <img src="docs/assets/world-map.png" alt="Eine Offline-Weltkarte, die jedes Land danach einfärbt, wie viele Antworten dorthin auflösten, samt der Netze (ASN) dahinter" width="820">
</p>

## Highlights

- **Sieh, welche Firmen deine Geräte kontaktieren.** Ein Privacy-Dashboard ordnet
  jede Anfrage der Firma dahinter zu (Werbung, Analyse, Social Media,
  Fingerprinting), mit Aufschlüsselung pro Gerät und einer Weltkarte, wohin
  Antworten auflösen. Ein Klick blockt eine Firma überall oder nur für ein
  einzelnes Gerät. Dazu eine **DACH-Trackerliste** (INFOnline/IVW, ÖWA), die
  internationale Listen kaum abdecken.
- **Blockt Werbung, Tracker und Schadsoftware** von Haus aus, mit kuratierten
  [Hagezi](https://github.com/hagezi/dns-blocklists)-Listen. Schalte die
  eingebauten Kategorien an oder aus, ergänze jederzeit eigene Allow-/Block-
  Einträge oder Listen-Abos und gib jedem **Gerät oder jeder Gruppe** eigene
  Regeln obendrauf.
- **Läuft als Cluster statt als Single Point of Failure.** Vernetze **bis zu vier
  gleichberechtigte Knoten** im Mesh, die synchron bleiben — was du auf einem
  Knoten änderst, fließt zu allen anderen. Nimm einen zum Neustarten oder Updaten
  vom Netz, ohne dass es jemand merkt, und ein **rollendes Cluster-Update** hebt
  jeden Knoten einzeln auf eine neue Version — ohne DNS-Ausfall. Standardmäßig aus;
  schalte es ein, wenn du es brauchst.
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
- **Richtlinien pro Gerät & Gruppe.** Gib einem einzelnen Gerät oder einer ganzen
  Gruppe eigene Sperrlisten und Erlauben/Sperren-Regeln. Die Regeln stapeln sich —
  globale Vorgabe → Gruppe → Gerät — du stellst also nur ein, was abweicht.
- **Gehärtet für den echten Betrieb.** DNSSEC-Validierung, Benutzerkonten mit
  **Rollen und TOTP-Zwei-Faktor** und jedes Geheimnis (Passwort-Hashes,
  TOTP-Schlüssel, Wiederherstellungscodes) **verschlüsselt gespeichert**. Ein
  benannter TLS-Zertifikatsspeicher bedient Dashboard, Sperrseite und
  verschlüsseltes DNS.
- **Geführte Einrichtung.** Ein Assistent beim ersten Start führt durch
  Administratorkonto, Resolver und Aufbewahrung. Oberfläche auf **Deutsch und
  Englisch**.

## Ein Blick hinein

<table>
  <tr>
    <td width="33%" valign="top">
      <img src="docs/assets/query-log.jpg" alt="Das Protokoll mit Quell-Filtern"><br>
      <sub><b>Protokoll</b> — jede Anfrage, welches Gerät fragte und wie beantwortet wurde, mit Live-Filtern.</sub>
    </td>
    <td width="33%" valign="top">
      <img src="docs/assets/blocked-services.jpg" alt="Der Katalog blockierbarer Dienste"><br>
      <sub><b>Blockierte Dienste</b> — ganze Dienste (TikTok, YouTube, …) global oder pro Gerät abschalten.</sub>
    </td>
    <td width="33%" valign="top">
      <img src="docs/assets/settings-notifications.jpg" alt="Der Einstellungsabschnitt Benachrichtigungen"><br>
      <sub><b>Benachrichtigungen</b> — Webhook-Alerts bei Node-Ausfall, Update, ablaufendem Zertifikat oder Geräte-Spike.</sub>
    </td>
  </tr>
</table>

## Im Vergleich

JustDNS spielt in derselben Liga wie Pi-hole und AdGuard Home. Hier sind die
Unterschiede — ehrlich, Gleichstand inklusive:

| | **JustDNS** | Pi-hole | AdGuard Home |
|---|---|---|---|
| Kern | Rust, ein Binary | C + PHP/Web | Go |
| Firmen-Privacy-Dashboard | ✅ mit Weltkarte | — | — |
| Natives Mehrknoten-HA | ✅ Mesh, bis zu 4 gleichberechtigt | ⚠️ über Dritte (gravity-sync, keepalived) | — |
| Rollendes Update ohne DNS-Ausfall | ✅ | — | — |
| Verschlüsselter Upstream per Vorgabe | ✅ DoT von Haus aus | ⚠️ Zusatz (cloudflared/unbound) | ✅ unterstützt |
| DNSSEC-Validierung | ✅ | ✅ | ✅ |
| Richtlinien pro Gerät & Gruppe | ✅ | ⚠️ Client-Gruppen | ✅ Clients |
| Benutzer, Rollen & TOTP-2FA | ✅ | — ein Passwort | — ein Login |
| Geheimnisse verschlüsselt gespeichert | ✅ | — | — |
| One-Click-Update in der App | ✅ | ⚠️ CLI | ✅ |
| Deutsche UI + komplettes Handbuch | ✅ | ⚠️ teilweise | ⚠️ teilweise |

Pi-hole und AdGuard Home sind hervorragende, weit erprobtere Projekte — wenn du
die größte Community und die längste Historie willst, nimm sie. Greif zu JustDNS,
wenn du **mehr als einen Knoten ohne DNS-Ausfall** willst, echte Konten und 2FA,
Verschlüsselung und EU-Resolver als Vorgabe und eine durchgängig deutsche
Oberfläche.

## Schnellstart (Docker)

Empfohlen ist das Zwei-Container-Bundle in [`deploy/`](deploy): der **Resolver +
Web-UI** plus ein kleines **Updater-Sidecar**, das One-Click-Updates aus dem
Dashboard ermöglicht. Der Hauptprozess fasst den Docker-Socket nie an — nur das
Sidecar —, damit das Updaten sicher bleibt. Beide Images kommen aus der öffentlichen
GitHub Container Registry, es ist also nichts zu bauen.

Aus einem Checkout dieses Repositorys:

```bash
cd deploy
cp .env.example .env     # optional: Version festnageln, Metrics-Token setzen
docker compose up -d
```

Kein Checkout? Hol dir die zwei Dateien direkt von GitHub und starte sie irgendwo:

```bash
mkdir justdns && cd justdns
curl -fsSL https://raw.githubusercontent.com/bonderaustriaAT/JustDNS/main/deploy/docker-compose.yml -o docker-compose.yml
curl -fsSL https://raw.githubusercontent.com/bonderaustriaAT/JustDNS/main/deploy/.env.example -o .env
docker compose up -d
```

Dann **http://<dein-host>:8080** öffnen — der Einrichtungs-Assistent übernimmt ab
hier. Richte das DNS eines Geräts (oder deines Routers) auf die IP des Hosts, um
loszulegen.

Der Zustand — Protokoll, Konfiguration, Konto, Listen und Caches — liegt im Volume
`justdns-data` und übersteht Neustarts und Updates. Ab hier ist Updaten ein Klick auf
der *System*-Seite des Dashboards — siehe [JustDNS aktualisieren](docs/guides/updates.de.md).

> **Einzelner Container (ohne In-App-Updates).** Wenn du nur den Resolver willst und
> selbst aktualisierst, kannst du nur das eine Image starten — dann ist das
> One-Click-Update aber nicht verfügbar, weil es das Sidecar braucht:
>
> ```bash
> docker run -d --name justdns -p 53:53/udp -p 53:53/tcp -p 8080:8080/tcp -v justdns-data:/data ghcr.io/bonderaustriaat/justdns:latest
> ```

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

Der DNS-Server bindet eine einzelne Adresse. Der Standard `0.0.0.0:53` ist die
IPv4-Wildcard, lauscht also bereits auf allen IPv4-Interfaces gleichzeitig und
braucht keine Interface-Einstellung. Um ihn auf ein Interface zu beschränken,
setze `JUSTDNS_LISTEN` auf dessen IPv4-Adresse, etwa `192.168.1.5:53`. Es wird
eine Bind-Adresse unterstützt, also entweder die Wildcard (alle Interfaces) oder
eine bestimmte Adresse.

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

## Anleitungen

Schritt-für-Schritt-Anleitungen zu jedem Feature, in Klartext (Deutsch und
Englisch), liegen im **[Handbuch](docs/README.de.md)** ([English](docs/README.md)).
Neu bei JustDNS? Starte mit der [Ersteinrichtung](docs/guides/setup.de.md), dann
[die Übersicht](docs/guides/overview.de.md).

## Fahrplan & Änderungsverlauf

- Wohin sich JustDNS entwickelt: [ROADMAP.de.md](ROADMAP.de.md)
  ([English](ROADMAP.md)).
- Was in jeder Version neu ist: [CHANGELOG.de.md](CHANGELOG.de.md)
  ([English](CHANGELOG.md)).

## Mitstreiter gesucht

JustDNS ist ein junges Projekt und wir freuen uns ehrlich über jede Hilfe. Ob
Bugfix, neues Feature, bessere Doku oder eine Übersetzung — Beiträge sind herzlich
willkommen. Wirf einen Blick in die
[offenen Issues](https://github.com/bonderaustriaAT/JustDNS/issues), öffne ein
neues für eine Idee, oder lies [CONTRIBUTING.md](CONTRIBUTING.md) für den Einstieg.
Kein Coder? Ein Stern, ein Fehlerbericht oder schlichtes Feedback hilft genauso. 😉

## Lizenz

[AGPL-3.0-or-later](LICENSE). Beiträge werden unter derselben Lizenz angenommen;
siehe [CONTRIBUTING.md](CONTRIBUTING.md).
