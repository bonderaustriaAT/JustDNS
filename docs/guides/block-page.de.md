# Eigene Sperrseite

*Eine englische Version findest du in [block-page.md](block-page.md).*

Standardmäßig beantwortet JustDNS einen geblockten Namen mit der **Null-IP**
(`0.0.0.0` / `::`) oder mit **NXDOMAIN** — die Verbindung scheitert einfach, oft als
nacktes *„Verbindung abgelehnt"*, ohne jede Erklärung. Eine **eigene Sperrseite**
ersetzt dieses stille Scheitern durch eine echte Webseite, die dem Nutzer sagt:
*diese Seite ist gesperrt*.

Du findest sie unter **Einstellungen → Block-Seite** (nur Admin). Sie ist
standardmäßig **aus**.

## Zwei Modi

Wenn du die Sperrseite einschaltest, wählst du einen **Modus**:

- **Hinter eigenem Reverse-Proxy (empfohlen).** Hast du ohnehin schon einen
  Reverse-Proxy (NGINX Proxy Manager, Traefik …), lass ihn **nur das Zertifikat**
  liefern: Er terminiert HTTPS und reicht die Anfrage per HTTP an **dieselbe
  JustDNS-Node** weiter, die die Seite rendert. In JustDNS wählst du **kein**
  Zertifikat aus, und dein Proxy erneuert es automatisch (z. B. per Let's Encrypt).
  Der klarste Weg zu einer sauberen HTTPS-Sperrseite. Siehe
  [Hinter einem Reverse-Proxy](#hinter-einem-reverse-proxy).
- **Auf eigener Domain hosten.** Kein eigener Proxy? Dann liefert JustDNS die Seite
  selbst unter *deinem eigenen* Hostnamen — z. B. `https://block.justfuture.at` —
  mit einem Zertifikat, das du in den [Zertifikatsspeicher](certificates.md) lädst
  und **für diesen Namen gültig** ist. Eine geblockte Domain wird dorthin
  weitergeleitet, der Besucher landet auf einer sauberen HTTPS-Seite **ohne
  Warnung**, die sogar anzeigen kann, welche Domain gesperrt wurde.
- **Direkt auf dieser Box anzeigen.** Die eingebaute Seite wird direkt unter der
  Adresse der geblockten Domain gerendert. Am einfachsten einzurichten, aber über
  HTTPS zeigt der Browser zuerst eine Zertifikatswarnung (siehe unten) — das lässt
  sich nicht vermeiden, wenn die Seite *als* fremde Domain antwortet.

So oder so ändert sich am Blocken sonst nichts: welche Namen geblockt werden, deine
Regeln pro Gerät und Gruppe und die Protokolle funktionieren genau wie zuvor. Nur
die *Antwort* auf einen geblockten Namen ändert sich — von einer toten IP zu einer,
die die Seite ausliefert (oder dorthin weiterleitet).

## Die Server-IP (beide Modi)

Beide Modi brauchen **eines**: die **Server-IPv4** — die Adresse, unter der diese
JustDNS-Box im LAN erreichbar ist (z. B. `192.168.20.2`). Geblockte `A`-Anfragen
werden damit beantwortet, sodass der Browser sich mit JustDNS verbindet statt mit
einer toten Adresse. **Lässt du sie leer, wird nichts ausgeliefert** — geblockte
`A`-Anfragen fallen auf die Null-IP zurück und du bekommst wieder „Verbindung
abgelehnt". Setze bei Bedarf auch die **Server-IPv6**; sonst liefern `AAAA`-Blocks
weiter die Null-IP (`::`).

## Hosted-Modus, Schritt für Schritt

Angenommen, deine Domain ist `justfuture.at` und die Seite soll unter
`https://justdns.justfuture.at/blockpage` liegen:

1. **_Block-Seite anzeigen_ einschalten** und Modus **Auf eigener Domain hosten**
   wählen.
2. **Adresse der Sperrseite** → `https://justdns.justfuture.at/blockpage`.
3. **Server-IPv4** → die LAN-IP dieser Box, z. B. `192.168.20.2`.
4. **Einen [DNS-Rewrite](rewrites.md) anlegen**: `justdns.justfuture.at →
   192.168.20.2`, damit dieser Hostname auf diese Box auflöst.
5. **Ein für `justdns.justfuture.at` gültiges Zertifikat** in den
   [Zertifikatsspeicher](certificates.md) hochladen (dein Let's-Encrypt- oder
   Wildcard-Zertifikat) und **als HTTPS-Zertifikat auswählen**.
6. Ports **80** und **443** veröffentlichen (siehe [Ports](#ports)).

Ein Gerät, das eine geblockte Domain über HTTP aufruft, wird jetzt nach
`https://justdns.justfuture.at/blockpage?d=<die-geblockte-domain>` weitergeleitet;
weil der Browser nun *deinen* Hostnamen anfragt, passt das Zertifikat, und die Seite
lädt sauber und zeigt an, welche Domain gesperrt wurde.

## Lokaler Modus, Schritt für Schritt

1. **_Block-Seite anzeigen_ einschalten** und den Modus auf **Direkt auf dieser Box
   anzeigen** lassen.
2. **Server-IPv4** → die LAN-IP dieser Box.
3. Optional **ein HTTPS-Zertifikat wählen** und **Überschrift und Text schreiben**.
4. Ports **80** und **443** veröffentlichen.

## Hinter einem Reverse-Proxy

Das ist der **empfohlene** Weg, wenn du ohnehin schon einen Reverse-Proxy betreibst —
**NGINX Proxy Manager (NPM)**, Traefik, Caddy, ein nacktes nginx. Der Proxy steuert
**nur das Zertifikat** bei; die Sperrseite liefert weiterhin **die JustDNS-Node
selbst**. Der Proxy nimmt die Landing-Adresse per HTTPS entgegen, terminiert TLS mit
*seinem* Zertifikat und reicht die Anfrage per **HTTP** an dieselbe Node weiter. Die
Node erkennt am weitergereichten **`X-Forwarded-Host`**-Header, dass die Landing-Seite
gemeint ist, und rendert sie — ganz ohne Weiterleitungsschleife. Der Proxy ist also
nur der TLS-Vorbau, nicht das Ziel.

Der Durchfluss in einer Zeile:

```
Browser ──HTTPS──▶ Reverse-Proxy (hat das Zert) ──HTTP + X-Forwarded-Host──▶ JustDNS-Node :80 ──▶ Sperrseite
```

### In JustDNS einstellen

1. **_Block-Seite anzeigen_ einschalten** und Modus **Hinter eigenem Reverse-Proxy**
   wählen.
2. **Adresse der Sperrseite** → die öffentliche HTTPS-Adresse der Seite. Nimm eine
   **eigene Subdomain** nur für die Sperrseite, z. B. `https://block.justfuture.at`.
3. **Server-IPv4** → die LAN-IP **dieser JustDNS-Node** (z. B. `192.168.20.5`).
   Darauf zeigen geblockte `A`-Anfragen, und dorthin reicht der Proxy weiter.
4. Optional **Überschrift und Text** anpassen.

Ein Zertifikat für die **Landing-Seite** wählst du hier **nicht** — das macht der
Proxy: Er terminiert HTTPS für `block.justfuture.at` und reicht per HTTP an die Node
weiter. Die Node bindet aber trotzdem **selbst Port 443**, denn geblockte Domains
zeigen direkt auf sie (`Server-IPv4`) und umgehen den Proxy — ruft jemand eine
geblockte Domain über `https://` auf, landet er auf `:443` *dieser Node*. Dort zeigt
sie das Standard-Zertifikat aus dem Speicher; der Browser warnt (wir haben kein Zert
für fremde Domains) und leitet dann auf die saubere Landing-Seite weiter. Diese
Warnung bei geblockten `https://`-Domains ist in **jedem** Modus unvermeidbar (siehe
[Die HTTPS-Zertifikatswarnung](#die-https-zertifikatswarnung)); der Proxy räumt sie
nur auf der Landing-Seite selbst weg.

> **Der Pfad ist egal.** Die Node liefert die Seite auf **jedem** Pfad des
> Landing-Hosts. `https://block.justfuture.at` genügt also — ein `/blockpage` am Ende
> ist nicht nötig (schadet aber auch nicht). Dein Proxy muss darum **nicht** auf einen
> bestimmten Pfad routen; den ganzen Host weiterzureichen reicht.

> **Nimm eine eigene Subdomain** (z. B. `block.justfuture.at`), **nicht** den
> Hostnamen deiner Admin-Oberfläche: Im einfachen Aufbau reicht der Proxy den *ganzen*
> Host an die Sperrseite weiter — teilst du ihn mit der Admin-UI, verdeckt die
> Sperrseite diese. (Willst du doch denselben Host nutzen, siehe
> [Einen bestehenden Host mitbenutzen](#einen-bestehenden-host-mitbenutzen).)

Geblockte Namen zeigen also wie gehabt **direkt auf die JustDNS-Node** (Server-IPv4).
Der Proxy sitzt nur vor dem **Landing-Hostnamen** und leitet an dieselbe Node weiter.

### NGINX Proxy Manager, Schritt für Schritt

Lege in NPM einen **Proxy Host** an, der den Landing-Hostnamen auf die JustDNS-Node
weiterreicht:

- **Details → Domain Names:** `block.justfuture.at` (genau der Host aus der Adresse
  oben).
- **Details → Scheme:** `http`, **Forward Hostname / IP:** die LAN-IP **dieser
  JustDNS-Node** (z. B. `192.168.20.5`), **Forward Port:** der HTTP-Port der
  Sperrseite (Standard **80**; in einem Docker-Setup der veröffentlichte oder
  containerinterne Port der Node).
- **Details:** *Block Common Exploits* an schadet nicht; *Websockets* ist nicht
  nötig.
- **SSL:** ein gültiges Zertifikat für `block.justfuture.at` (z. B. per
  Let's-Encrypt-Request direkt in NPM) wählen, **Force SSL** und **HTTP/2** an.
- **Advanced → Custom Nginx Configuration:** den weitergereichten Host setzen, damit
  die Node die Landing-Seite erkennt:

  ```nginx
  proxy_set_header X-Forwarded-Host $host;
  ```

  (NPM setzt `X-Forwarded-For`/`-Proto` selbst; `X-Forwarded-Host` ist der eine
  Header, auf den JustDNS angewiesen ist.)

Zuletzt muss der **Landing-Hostname** (`block.justfuture.at`) **auf den Proxy**
auflösen (öffentlicher DNS-Eintrag oder ein [JustDNS-Rewrite](rewrites.md) auf die
Proxy-IP), damit der Proxy TLS terminieren kann — die eigentliche Seite liefert
dahinter die Node. Ab dann wird jeder geblockte Aufruf zu
`https://block.justfuture.at/?d=…` geleitet, der Proxy liefert sauberes HTTPS, und die
JustDNS-Node rendert die Seite.

**Kurzer Test:** `http://<node-ip>/` (direkt, mit beliebigem `Host`-Header) muss die
Sperrseite zeigen, und `https://block.justfuture.at/` über den Proxy ebenso — sauberes
Zertifikat, keine Warnung.

#### Einen bestehenden Host mitbenutzen

Willst du **keine** eigene Subdomain anlegen, sondern einen schon vorhandenen Host
(etwa den deiner Admin-UI) nutzen, route in NPM **nur einen Pfad** zur Sperrseite:
Proxy Host → Reiter **Custom locations** → Location `/blockpage` → Forward auf
`http://<node-ip>:80` (Header `X-Forwarded-Host $host;` wie oben). Der restliche Host
bleibt bei deinem bisherigen Ziel. Trage dann als **Adresse der Sperrseite**
`https://<dein-host>/blockpage` ein, damit die Weiterleitung genau diesen Pfad trifft.

> Andere Proxys verhalten sich gleich: Setze `X-Forwarded-Host` auf den angefragten
> Host und leite per HTTP an die JustDNS-Node weiter. In **Traefik** etwa trägt die
> `Host(...)`-Rule den Header ohnehin mit; nur bei exotischen Setups musst du ihn
> explizit setzen.

## Die HTTPS-Zertifikatswarnung

Das ist der ehrliche, unvermeidbare Haken — und der Grund, warum es den
Hosted-Modus gibt. Ruft ein Gerät eine **geblockte HTTPS**-Domain direkt auf — etwa
`https://ads.example.com` —, erwartet der Browser ein Zertifikat, das für
`ads.example.com` ausgestellt ist. JustDNS kann nur das gewählte Zertifikat
vorzeigen, das **nicht** auf die Fremddomain ausgestellt ist, also zeigt der Browser
zuerst eine **Zertifikatswarnung** — sogar noch vor einer möglichen Weiterleitung,
weil der TLS-Handschlag zuerst kommt. Ohne Aufbrechen der verschlüsselten Verbindung
geht das nicht, und das tut JustDNS **nicht**.

In der Praxis:

- Über reines **HTTP** sind beide Modi sauber — die Seite erscheint bzw. die
  Weiterleitung greift, ohne Warnung.
- Über **HTTPS zur geblockten Domain** erscheint die Warnung in *beiden* Modi zuerst.
- **Der Gewinn ist die *Landing*-Seite:** nach der Weiterleitung ist der Besucher auf
  deinem eigenen Hostnamen mit gültigem Zertifikat — diese Seite ist sauber. Im
  Reverse-Proxy-Modus liefert das der Proxy, im Hosted-Modus das von JustDNS gewählte
  Zertifikat. In beiden Fällen bindet die Node ihr eigenes `:443` für die direkten
  Treffer der geblockten Domains; nur dort ist die Warnung unvermeidbar.
- Die meisten geblockten Aufrufe sind heute Werbe- und Tracking-Endpunkte, die
  *innerhalb* einer Seite geladen werden; die scheitern einfach still, und genau das
  ist der Zweck. Die sichtbare Sperrseite zählt vor allem, wenn jemand eine geblockte
  Seite direkt ansteuert.

## Ports

Die Sperrseite belegt die Ports **80** (HTTP) und **443** (HTTPS) des Hosts. Sind
sie nicht erreichbar, endet jede geblockte Domain in „Verbindung abgelehnt".

**Normalerweise musst du nichts tun.** Eine frische Installation aus dem offiziellen
`deploy/docker-compose.yml` veröffentlicht sie bereits, und bei einem **Update in der
App** veröffentlicht der Updater-Sidecar sie für dich — er legt eine kleine, klar
gekennzeichnete `docker-compose.override.yml` neben deine Compose-Datei (Compose
hängt `ports` aneinander, deine anderen Ports bleiben also erhalten). Lösche diese
Override und erzeuge den Dienst neu, wenn du die Ports wieder los willst oder etwas
anderes am Host 80/443 braucht.

**Einmalige Ausnahme:** Eine Installation, deren Updater älter als diese Version ist,
läuft noch mit dem alten Sidecar, der das noch nicht kann. Frisch ihn einmalig auf —

```bash
cd /opt/justdns && docker compose pull updater && docker compose up -d updater
```

— und ab dann erledigt jedes Update die Ports von selbst. Alternativ den Resolver mit
**`network_mode: host`** betreiben, wo 80/443 immer erreichbar sind und kein Mapping
nötig ist; der Updater lässt deine Compose dann unangetastet.

## Der Browser zeigt „Website ist nicht erreichbar" statt der Sperrseite

Fast immer ist es eine dieser beiden Ursachen:

- **Falsche Server-IPv4.** Sie muss die LAN-Adresse **dieser JustDNS-Box** sein — die,
  unter der die Ports 80/443 veröffentlicht sind. Steht dort die IP eines anderen Geräts,
  landet die geblockte Domain bei einem Host ohne Sperrseite. Prüfen: Die geblockte Domain
  muss auf genau diese IP auflösen, und `http://<diese-IP>/` muss die Sperrseite zeigen.
- **Chrome meldet `ERR_ADDRESS_INVALID`** (vor v0.21.3). Ohne gesetzte Server-IPv6
  beantwortete JustDNS die IPv6-Anfrage mit der Nulladresse `::`, die Browser sofort
  ablehnen. Ab **v0.21.3** bleibt diese Antwort leer und der Browser nutzt die IPv4-Seite —
  einfach updaten.
- **Die Block-Seite schaltete sich im Cluster immer wieder selbst ab** (vor v0.22.1). Ihr
  An/Aus-Schalter steckte im einen Einstellungs-Datensatz, den die Knoten synchronisieren,
  sodass eine fremde Einstellungsänderung auf einem Knoten einen älteren Stand „aus" auf
  den anderen tragen konnte — die Listener kamen beim Start hoch und gingen Sekunden
  später wieder aus. Ab **v0.22.1** wird die Block-Seite als eigener Datensatz mit eigenem
  Zeitstempel synchronisiert und ändert sich clusterweit nur noch, wenn du die Block-Seite
  änderst. Einfach updaten.

---

## Für die technisch Neugierigen

**Die DNS-Seite.** Ist die Sperrseite an, beantwortet der Block-Zweig der Pipeline
ein geblocktes `A` mit der konfigurierten IPv4 und ein geblocktes `AAAA` mit der
konfigurierten IPv6 statt mit der Null-IP. Eine Adressfamilie ohne konfigurierte IP
fällt auf das bisherige Verhalten zurück (Null-IP / NXDOMAIN), sodass ein
IPv4-only-Setup bei `AAAA` weiter korrekt arbeitet. Das Ziel liegt in einem geteilten
Slot, den die Einstellungen live aktualisieren — das Umschalten braucht keinen
Neustart.

**Die Web-Seite — ein Server, zwei Aufgaben.** Zwei Listener laufen nur, solange die
Seite aktiv ist: reines HTTP auf `JUSTDNS_BLOCKPAGE_HTTP_LISTEN` (`0.0.0.0:80`) und
TLS auf `JUSTDNS_BLOCKPAGE_HTTPS_LISTEN` (`0.0.0.0:443`). Für jede Anfrage schaut der
Handler auf den `Host`-Header:

- **Lokaler Modus** (keine Landing-URL): jede Anfrage rendert eine in sich
  geschlossene, dunkle HTML-Seite mit Status **451 (Unavailable For Legal Reasons)**
  und `Cache-Control: no-store`; der angefragte `Host`, deine Überschrift und dein
  Text werden HTML-escaped hineingeschrieben.
- **Hosted-Modus** (eine Landing-URL ist gesetzt): eine Anfrage, deren Host
  **nicht** der Landing-Host ist (eine hierher gelenkte geblockte Domain), bekommt
  `302 Found` mit `Location: <landing-url>?d=<geblockter-host>`; eine Anfrage, deren
  Host der Landing-Host **ist**, bekommt die Seite selbst (`200 OK`), die `?d=`
  liest, um die geblockte Domain zu benennen. Der Abgleich ignoriert Port und
  Groß-/Kleinschreibung, und *jeder* Pfad unter dem Landing-Host liefert die Seite —
  so entsteht keine Weiterleitungsschleife.

Den angefragten Host liest der Handler zuerst aus dem `Host`-Header; fehlt er (jede
**HTTP/2**-Anfrage trägt den Host stattdessen in der `:authority`-Pseudo-Header), aus
der Request-URI. Im **Reverse-Proxy-Modus** (`behindReverseProxy`) kommt er aus
`X-Forwarded-Host` — aber nur dann, denn ohne diese ausdrückliche Vertrauensstellung
könnte ein Client den Header fälschen. Der `behindReverseProxy`-Schalter steuert
**ausschließlich** dieses Vertrauen in `X-Forwarded-Host` — **nicht**, welche Ports
binden: der Server hält `:80` **und** `:443`, solange die Sperrseite an ist, in jedem
Modus. Das ist nötig, weil geblockte Domains per `server_ipv4` direkt auf die Node
zeigen und den Proxy umgehen; ein `https://`-Aufruf einer geblockten Domain trifft
also `:443` dieser Node. Der Proxy terminiert TLS nur für den Landing-Host.

Ein Supervisor startet und stoppt die Listener bei Settings-Änderung, und die
HTTPS-Konfiguration wird aus dem [Zertifikatsspeicher](certificates.md) neu gebaut,
sobald du ein anderes Zertifikat wählst oder das aktuelle erneuerst — ein atomarer
`ArcSwap`-Tausch, kein Rebind.

**Keine On-the-fly-CA.** Der HTTPS-Listener zeigt genau das gewählte Zertifikat.
JustDNS stellt bewusst **keine** Zertifikate pro Domain aus und spielt **nicht**
MITM — deshalb ist die Fremddomain-Warnung unvermeidbar, und deshalb ist das Hosten
der Seite unter deinem eigenen Namen (wo das Zertifikat *passt*) der saubere Weg.

**Hochverfügbarkeit.** Der An/Aus-Schalter, Modus (inkl. Reverse-Proxy-Schalter),
Überschrift, Text, Landing-URL **und die gewählte Zertifikats-id** werden im
[Cluster](high-availability.md) **geteilt** — der
Zertifikatsspeicher selbst wird synchronisiert (siehe
[Zertifikate](certificates.md#hochverfügbarkeit)), sodass die id auf jedem Knoten
dasselbe Zertifikat benennt. Nur die **Server-IPs** bleiben **knotenlokal**: jeder
Knoten lenkt geblockte Namen auf sich selbst, eine Synchronisierung trägt sie also
nie zwischen Knoten (pro Knoten einstellbar bleibt es). Mit geteilter Landing-URL
leitet jeder Knoten auf dieselbe saubere Seite.
