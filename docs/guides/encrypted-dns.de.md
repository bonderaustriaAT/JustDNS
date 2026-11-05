# Verschlüsseltes Client-DNS (DoT & DoH)

*Eine englische Fassung dieser Anleitung liegt in [encrypted-dns.md](encrypted-dns.md).*

Standardmäßig reden deine Geräte mit JustDNS **im Klartext** auf Port 53: Wer im
selben Netzwerk sitzt (oder dein Internetanbieter, sobald der Verkehr das Haus
verlässt), kann die abgefragten Namen mitlesen und manipulieren. Mit
**verschlüsseltem Client-DNS** erreichen deine eigenen Geräte JustDNS über einen
verschlüsselten Kanal:

- **DNS-over-TLS (DoT)** auf Port **853** — das, was Android **„Privates DNS“**
  nennt.
- **DNS-over-HTTPS (DoH)** unter **`/dns-query`** (Standard-Port 8443) — das, was
  Browser und viele Betriebssysteme unterstützen.

Verschlüsselt wird dabei nur die Strecke zwischen deinen Geräten und JustDNS. Was
JustDNS mit einer Anfrage macht — Filterung, deine Geräte- und Gruppenregeln,
Cache, die Upstream-Abfrage — ist genau dasselbe wie bei einer Klartext-Anfrage
auf Port 53. Die Strecke zu den *Upstream*-Resolvern verschlüsselt JustDNS ohnehin
schon; dies verschlüsselt zusätzlich die *Client*-Seite.

Zu finden unter **Einstellungen → Verschlüsseltes Client-DNS**. Beide Endpunkte
sind **aus**, bis du sie einschaltest.

## Einrichten

1. **Hostnamen wählen.** Trage den Namen ein, unter dem deine Geräte JustDNS
   erreichen, etwa `dns.fritz.box` oder `dns.home`. Dieser Name steht im
   Zertifikat und muss exakt der Name sein, den die Geräte ansprechen.
2. **Hostnamen auf JustDNS zeigen lassen** — im Netzwerk. Am einfachsten über
   einen [DNS-Rewrite](rewrites.de.md), der diesen Namen mit der IP-Adresse von
   JustDNS beantwortet; manche Router erlauben auch einen lokalen Hostnamen-Eintrag.
3. **Zertifikat wählen** (siehe unten) und beim selbstsignierten Standard die
   **CA herunterladen** und auf jedem Gerät installieren.
4. **DoT und/oder DoH einschalten.** Die Endpunkte starten sofort — ohne Neustart.
5. **Geräte auf den Endpunkt zeigen lassen**, wie unter *Endpunkte zum Einrichten*
   angezeigt.

## Das Zertifikat

Eine verschlüsselte Verbindung braucht ein Zertifikat, dem das Gerät vertraut. Du
hast zwei Möglichkeiten:

- **Selbstsigniert (Standard).** JustDNS erzeugt eine eigene kleine
  Zertifizierungsstelle (CA) und ein Zertifikat für deinen Hostnamen. Weil keine
  öffentliche Stelle es signiert hat, muss jedes Gerät der CA einmalig vertrauen —
  **lade das CA-Zertifikat** auf dieser Seite herunter und installiere es (Schritte
  je Plattform unten). Mit **Zertifikat neu erzeugen** bekommst du ein frisches,
  etwa nach einer Hostnamen-Änderung (danach die CA erneut installieren).
- **Eigenes hochladen.** Hast du schon ein Zertifikat — von einer internen
  Firmen-CA oder einem öffentlichen per ACME —, füge die Zertifikatskette und den
  privaten Schlüssel ein (beides PEM). JustDNS prüft das Paar vor der Übernahme.
  Geräte, die dieser Stelle schon vertrauen, brauchen keine weitere Einrichtung.

Der **Fingerabdruck (SHA-256)** auf der Seite kennzeichnet das genau verwendete
Zertifikat. Manche Clients erlauben, ihn zu pinnen oder zu prüfen; so bestätigst du
am Gerät, dass du mit *deinem* JustDNS redest und nicht mit einem Betrüger.

## Ein Gerät auf JustDNS zeigen lassen

Die Seite zeigt die beiden fertigen Endpunkt-Adressen — kopiere sie mit
**Kopieren**:

- **DoT** — `dein-hostname:853`
- **DoH** — `https://dein-hostname:8443/dns-query`

**Android (Privates DNS):** Einstellungen → Netzwerk & Internet → Privates DNS →
*Hostname des privaten DNS-Anbieters* → deinen Hostnamen eintragen. Android nutzt
DoT auf 853. Bei selbstsigniertem Zertifikat zuerst die CA installieren (unten).

**Ein Browser (Firefox, Chrome):** in der Einstellung für sicheres DNS / DoH einen
benutzerdefinierten Anbieter wählen und die DoH-URL einfügen.

**Systemweites DoH (Windows 11, macOS, iOS):** akzeptieren eine DoH-URL in den
Netzwerkeinstellungen oder über ein Konfigurationsprofil. Bei selbstsigniertem
Zertifikat muss das System zuerst der CA vertrauen.

---

## Für Technik-Interessierte

**Was die Endpunkte sind.** DoT ist schlicht DNS-over-TCP in TLS: JustDNS nutzt
seinen längenframten TCP-DNS-Handler über den entschlüsselten Stream wieder. DoH
ist RFC 8484 — ein einzelner `GET`/`POST /dns-query`-Endpunkt mit
`application/dns-message`, über HTTP/1.1 oder HTTP/2. Beide terminieren TLS in
JustDNS selbst; es gibt keinen Reverse-Proxy, also ist die **vom Policy-Modul
gesehene Client-IP die echte Geräteadresse** — Geräte- und Gruppenregeln greifen
unverändert. DoH-Antworten tragen ein `Cache-Control: max-age` gleich der
kleinsten Record-TTL der Antwort.

**Ports.** DoT lauscht standardmäßig auf `0.0.0.0:853`, DoH auf `0.0.0.0:8443`
(überschreibbar mit `JUSTDNS_DOT_LISTEN` / `JUSTDNS_DOH_LISTEN`). Das Container-Image
exponiert beide; dein Deployment mappt sie (`-p 853:853/tcp -p 8443:8443/tcp`).
Port 443 ist oft von anderen Diensten belegt, deshalb liegt DoH auf 8443; lege es
nur dann hinter dein eigenes 443, wenn dort nichts anderes gebraucht wird.

**Wo das Zertifikat liegt.** Zertifikat und Schlüssel liegen in
`JUSTDNS_TLS_DIR` (`/data/tls` im Image): `cert.pem` (Leaf + CA-Kette), `key.pem`
(der private Schlüssel, Modus `0600`) und beim selbstsignierten Fall `ca.pem` /
`ca-key.pem`. Weil `/data` das persistente Volume ist, **übersteht das Zertifikat
Image-Updates und Neustarts** — der Fingerabdruck bleibt über Upgrades gleich, du
installierst die CA also nur einmal. Der Schlüssel liegt **nicht** in einer
[Konfigurations-Sicherung](backup.de.md) (eine Sicherung enthält die An/Aus-Schalter
und den Hostnamen, nicht die private Identität des Servers); nach dem Einspielen auf
einem frischen Gerät das Zertifikat neu erzeugen oder erneut hochladen.

**Die selbstsignierte CA installieren.**

- **Windows:** `justdns-ca.pem` doppelklicken → *Zertifikat installieren* →
  *Lokaler Computer* → *Alle Zertifikate in folgendem Speicher ablegen* →
  **Vertrauenswürdige Stammzertifizierungsstellen**.
- **macOS:** in der **Schlüsselbundverwaltung** (System-Schlüsselbund) öffnen, dann
  auf *Immer vertrauen* stellen.
- **iOS/iPadOS:** die Datei als Profil installieren (Einstellungen → Allgemein →
  VPN & Geräteverwaltung), dann unter **Allgemein → Info → Zertifikatsvertrauen**
  aktivieren.
- **Android:** Einstellungen → Sicherheit → *Verschlüsselung & Anmeldedaten* →
  *Zertifikat installieren* → **CA-Zertifikat**. Modernes Android vertraut
  nutzerinstallierten CAs für den System-Resolver (Privates DNS) — für Privates
  DNS reicht das, auch wenn Apps ihnen nicht zwangsläufig vertrauen.
- **Linux:** nach `/usr/local/share/ca-certificates/justdns-ca.crt` kopieren und
  `update-ca-certificates` ausführen.

**Von der Kommandozeile prüfen.**

```bash
# DoT
kdig +tls @dein-hostname -p 853 example.com
# DoH (eine Wire-Format-Anfrage per POST)
curl --http2 -H 'content-type: application/dns-message' \
  --data-binary @query.bin https://dein-hostname:8443/dns-query | xxd
```

**Sicherheitshinweise.** Der private Schlüssel wird nur für den Eigentümer lesbar
geschrieben (`0600`). Ein hochgeladenes Zertifikat/Schlüssel-Paar wird vor der
Übernahme geprüft. Weil JustDNS TLS terminiert, vertraut es für die Client-IP nie
einem `X-Forwarded-For`-Header, sondern nimmt die echte TLS-Peer-Adresse.
