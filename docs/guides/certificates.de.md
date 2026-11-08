# Zertifikate

*Eine englische Fassung dieser Anleitung findest du in [certificates.md](certificates.md).*

Mehrere JustDNS-Funktionen liefern TLS aus: die verschlüsselten Client-Endpunkte
[DoT und DoH](encrypted-dns.de.md) und die HTTPS-Seite der
[Block-Seite](block-page.de.md). Statt dass jede ihr eigenes Zertifikat mitträgt,
hält JustDNS einen zentralen **Zertifikatsspeicher** — einen benannten Katalog, den
du an einer Stelle verwaltest und dann überall dort **auswählst**, wo ein
Zertifikat gebraucht wird, ohne erneuten Upload.

Du findest ihn unter **System → Zertifikate** (nur Admins).

## Was ein gespeichertes Zertifikat ist

Jeder Eintrag hat einen **Namen**, den du vergibst, einen **Typ** und die aus dem
Zertifikat selbst gelesenen Fakten — Betreff, die Namen, für die es gilt, das
Ablaufdatum und einen SHA-256-Fingerabdruck. Die Spalte **Genutzt von** zeigt,
welche Funktionen es gerade ausliefern (DoT, DoH oder die Block-Seite), damit du
auf einen Blick siehst, was eine Änderung beträfe.

Zwei Typen:

- **Selbstsigniert** — JustDNS hat es selbst erzeugt, zusammen mit einer lokalen
  Zertifizierungsstelle (CA). Lade diese CA herunter und installiere sie auf jedem
  Gerät, das der Verbindung vertrauen soll (die Schritte je Plattform stehen in der
  [Anleitung zum verschlüsselten DNS](encrypted-dns.de.md)).
- **Hochgeladen** — eine Zertifikatskette und ein privater Schlüssel, die du
  bereitstellst, etwa von einer internen Firmen-CA oder per ACME ausgestellt.
  Geräte, die dieser Ausstellerin bereits vertrauen, brauchen keine weitere
  Einrichtung.

Eine frische Installation startet mit einem einzigen selbstsignierten Zertifikat
namens **Standard**, aus der Zeit vor dem Speicher migriert; DoT und DoH zeigen
darauf, bis du etwas anderes wählst.

## Zertifikat hinzufügen

Gib unter **Zertifikat hinzufügen** einen Namen ein, dann entweder:

- **Erzeugen** — ein neues selbstsigniertes Zertifikat prägen. Lass den Hostnamen
  leer, um deinen Encrypted-DNS-Hostnamen zu verwenden, oder setze einen eigenen.
  JustDNS erstellt die CA und das Zertifikat für dich.
- **Hochladen** — eine Zertifikatskette und ihren privaten Schlüssel einfügen
  (beide PEM). Das Paar wird vor dem Speichern geprüft; passt es nicht zusammen,
  wird es mit klarer Meldung abgelehnt.

Das neue Zertifikat erscheint sofort im Katalog und ist in jedem Zertifikats-
Dropdown der Funktionen auswählbar.

## Zertifikat erneuern

Ein Zertifikat wird mit **Erneuern** **an Ort und Stelle** ersetzt: Lade eine
frische Kette samt Schlüssel hoch, und sind sie aktuell gültig, übernehmen sie
denselben Eintrag — gleicher Name, gleiche Id, sodass **jede Funktion, die es
bereits ausgewählt hatte, weiterarbeitet**, ohne dass du ihre Einstellungen
anfasst. Der Ersatz wird zuerst geprüft: Ein abgelaufener oder noch nicht gültiger
Upload wird verweigert, und das alte Zertifikat bleibt. So rotierst du ein
hochgeladenes Zertifikat vor dem Ablauf oder tauschst später ein selbstsigniertes
gegen ein echtes.

## Zertifikat löschen

**Löschen** entfernt ein Zertifikat endgültig aus dem Speicher. Ein Zertifikat, das
eine Funktion noch auswählt, **lässt sich nicht löschen** — weise DoT, DoH und die
Block-Seite zuerst einem anderen Zertifikat zu, dann löschen. Der Speicher behält
immer mindestens ein Zertifikat.

## CA herunterladen

Bei einem selbstsignierten Zertifikat gibt dir **CA herunterladen** die lokale CA
im PEM-Format. Installiere sie als vertrauenswürdige Stelle auf jedem Gerät, das
ohne Warnung verbinden soll. Ein hochgeladenes Zertifikat hat keine JustDNS-CA, der
Knopf bewirkt dort also nichts — diese Geräte vertrauen ihm über die von dir
hochgeladene Ausstellerin.

---

## Für die technisch Neugierigen

**Wo es liegt.** Der Speicher ist ein Verzeichnis, `JUSTDNS_CERTS_DIR`
(`/data/certs` im Image), mit einem Unterverzeichnis je Zertifikat, benannt nach
seiner stabilen Id. Jedes enthält `cert.pem` (Leaf + Kette), `key.pem` (den
privaten Schlüssel, Modus `0600`) und `meta.json` (Name, Typ und Erstellzeit); ein
selbstsignierter Eintrag hält zusätzlich `ca.pem` / `ca-key.pem`. Weil `/data` das
persistente Volume ist, **übersteht der Speicher Image-Updates und Neustarts**, ein
Fingerabdruck bleibt über Upgrades gleich und die CA wird einmal installiert.

**Migration.** Beim ersten Start nach dem Upgrade wird vorhandenes
Einzel-Zertifikat-Material im alten `JUSTDNS_TLS_DIR` (`/data/tls`) als Eintrag
**Standard** importiert (stabile Id `standard`), und DoT/DoH liefern es weiter aus.
Nichts geht verloren, und die CA muss nicht neu installiert werden.

**Gültigkeitsprüfung.** Ablauf, Betreff und SANs werden mit `x509-parser` aus dem
DER gelesen. Eine Erneuerung verweigert ein Zertifikat, dessen `notBefore` in der
Zukunft oder dessen `notAfter` in der Vergangenheit liegt, damit eine aktive
Funktion nie auf ein Zertifikat umgeschaltet wird, das keinen Handshake
abschließen kann.

**Hot-Swap.** Jede Funktion liest ihre TLS-Konfiguration je Verbindung aus einem
geteilten Slot (`ArcSwap<ServerConfig>`). Ein anderes Zertifikat zu wählen — oder
eines an Ort und Stelle zu erneuern — baut diese Konfiguration neu und tauscht sie
atomar; **kein Listener wird neu gebunden und keine Verbindung abgerissen.**

**Auswahl und Draht.** Eine Funktion speichert die gewählte **Id**, nicht das
Material. DoT und DoH halten ihre in den Encrypted-DNS-Einstellungen (`dotCertId` /
`dohCertId`), die Block-Seite hält `certId`. Eine leere / fehlende Auswahl bedeutet
„Speicher-Standard", sodass eine Funktion immer ein Zertifikat hat, selbst wenn das
benannte gelöscht wurde.

**Hochverfügbarkeit.** Der **Zertifikatsspeicher wird clusterweit geteilt**
([Cluster](high-availability.de.md)): Ein Zertifikat, das du auf einem Knoten
anlegst, hochlädst oder erneuerst, wird auf jeden anderen Knoten übertragen —
**inklusive privatem Schlüssel** —, sodass eine Speicher-Id überall dasselbe
Zertifikat meint. (Das Schlüsselmaterial reist nur über den eigenen,
authentifizierten Cluster-Sync, nie zu einem Client.) Weil die Ids übereinstimmen,
ist auch die Zertifikatsauswahl der **Block-Seite** geteilt. Die **DoT/DoH**-Auswahl
bleibt dagegen **knotenlokal**: Jeder Knoten antwortet unter seinem eigenen
Hostnamen und wählt daher das auf *seinen* Namen ausgestellte Zertifikat. Löscht du
ein Zertifikat auf einem Knoten, verschwindet es beim nächsten Abgleich clusterweit.

**Private Schlüssel und Sicherungen.** Schlüssel werden nur für den Eigentümer
lesbar geschrieben (`0600`) und sind **nicht** in einer
[Konfigurationssicherung](backup.de.md) enthalten — eine Sicherung trägt die
Auswahlen, nicht die private Identität des Servers. Nach dem Wiederherstellen auf
einer frischen Maschine die Zertifikate neu erzeugen oder hochladen.
