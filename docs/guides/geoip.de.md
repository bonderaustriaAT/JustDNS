# Land und Netz der Antworten (GeoIP)

*Eine englische Fassung dieses Leitfadens ist in [geoip.md](geoip.md).*

Jedes Mal, wenn JustDNS eine Anfrage beantwortet, kennt es bereits die Adressen, zu denen
der Name aufgelöst wurde. Mit einer geladenen GeoIP-Datenbank zeigt es zusätzlich, in
**welches Land und welches Netz (ASN)** jede dieser Adressen gehört – so siehst du auf einen
Blick, dass dein Smart-TV gerade einen Namen in ein Netz in einem anderen Land aufgelöst
hat. Aus „was wurde gefragt" wird „und wohin zeigt das".

Es läuft **ausschließlich offline**: die Zuordnung kommt aus einer lokalen Datenbankdatei,
nie aus einer Abfrage bei einem Online-Dienst. Genau das ist der Sinn für ein
Privatsphäre-Werkzeug – JustDNS zeigt, wohin der Verkehr deiner Geräte geht, ohne selbst zu
einer weiteren Stelle zu werden, die mitschaut.

## Was du siehst

- **Im Protokoll** trägt jede Zeile, die zu einer öffentlichen Adresse aufgelöst hat, eine
  kleine Plakette mit Länderflagge und Netzbetreiber (zum Beispiel
  `🇺🇸 CLOUDFLARENET · AS13335`). Private Ziele (dein eigenes LAN) tragen keine. Den
  Live-Feed kannst du zudem auf Antworten einschränken, die in einem Land oder einem
  bestimmten ASN landen.
- **Im Privatsphäre-Dashboard** gruppiert ein neuer Abschnitt, **Wohin telefonieren deine
  Geräte**, die Ziele deines gesamten Netzwerks im gewählten Zeitraum nach Land und Netz –
  die aktivsten zuerst.

Am DNS-Dienst selbst ändert sich nichts, und das Protokoll wird nicht größer – Land und Netz
werden beim Lesen des Dashboards ermittelt. Das Ein- und Ausschalten oder ein Wechsel der
Datenbank wirkt daher sofort und beschriftet sogar ältere Protokolleinträge nach.

## Die Datenbank

JustDNS liefert die **DB-IP-Lite**-Datenbanken für Land und ASN mit (lizenziert unter
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) von
[DB-IP](https://db-ip.com)), sodass die Funktion im Container-Image sofort ohne Einrichtung
funktioniert. Die Lite-Datenbanken werden monatlich aktualisiert; ein neues Image zieht die
Fassung des aktuellen Monats.

### Eigene Datenbank verwenden

Wenn du MaxMinds GeoLite2-Daten (oder eine andere `.mmdb` im MaxMind-Format) willst, zeig
JustDNS über zwei Umgebungsvariablen auf deine eigenen Dateien:

```
JUSTDNS_GEOIP_COUNTRY_DB=/pfad/zu/GeoLite2-Country.mmdb
JUSTDNS_GEOIP_ASN_DB=/pfad/zu/GeoLite2-ASN.mmdb
```

Jede Datei darf auch allein angegeben werden – eine Länderdatenbank ohne ASN-Datenbank zeigt
weiterhin Flaggen, umgekehrt weiterhin Netze. Lässt sich keine Datei lesen, bleibt die
Funktion einfach aus: das Protokoll trägt keine Plaketten und der Privatsphäre-Abschnitt
blendet sich aus. (GeoLite2 braucht ein kostenloses MaxMind-Konto und unterliegt MaxMinds
eigener Lizenz – deshalb ist es nicht mitgeliefert.)

## Für die technisch Neugierigen

- Die Datenbanken werden im Standard-`.mmdb`-Format von MaxMind gelesen, DB-IP Lite und
  MaxMind GeoLite2 sind also austauschbar.
- Nachschläge werden pro Adresse im Speicher gecacht; dieselben aktiven Ziele (große CDNs,
  deine Resolver) werden einmal zugeordnet und sind danach gratis.
- Private, Loopback-, Link-Local- und CGNAT-Adressen werden lokal erkannt und erreichen die
  Datenbank nie – sie erscheinen als lokal.
- Die Datenbankpfade sind node-lokal (sie kommen aus der Umgebung, nicht aus den
  synchronisierten Einstellungen), ein eigener Pfad auf einer Node wird also nie an den Rest
  eines Hochverfügbarkeits-Clusters verteilt.
