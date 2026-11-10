# Verschlüsselung im Ruhezustand

*An English version of this guide is in [secrets-at-rest.md](secrets-at-rest.md).*

JustDNS bewahrt einige Geheimnisse auf seinem Datenträger auf: die Kontenliste
(Passwort-Hashes, TOTP-Secrets, Wiederherstellungscodes) und die geheimen Teile deiner
Einstellungen (ein Benachrichtigungs-Webhook oder Bot-Token). Ab dieser Version liegen
diese Dateien **verschlüsselt auf der Platte** — eine kopierte Daten-Partition, ein
Backup oder ein VM-Snapshot ist ohne den Schlüssel wertlos.

Es gibt **nichts einzuschalten und nichts einzustellen.** Es passiert von selbst.

## Wogegen das schützt — und wogegen nicht

Die Verschlüsselung im Ruhezustand schützt deine Geheimnisse, wenn die **Daten**
entwendet werden: jemand kopiert das `/data`-Volume, zieht ein Backup-Archiv oder klont
die Platte bzw. die VM. Zu sehen ist dann nur Geheimtext.

Sie schützt **nicht** vor einem voll kompromittierten, laufenden Server. Ein laufendes
JustDNS muss seinen Schlüssel unbeaufsichtigt lesen, um ohne Passworteingabe zu starten
— wer also bereits Root auf dem laufenden Host hat, liest ihn ebenfalls. Dagegen zu
schützen hieße, bei jedem Start ein Geheimnis einzutippen; das vermeiden wir bewusst,
weil es aus einem Routine-Neustart oder einem Ein-Klick-Update einen drohenden Lockout
machen würde. Das realistische Angriffsszenario für eine selbstgehostete Box ist ein
geklautes Volume oder Backup — und genau das schließt dies.

## Der Schlüssel und das eine, was du dir merken musst

Der Schlüssel ist ein zufälliger Wert, erzeugt beim ersten Start dieser Version. Er
liegt in **einem eigenen Volume** (`/keys`), getrennt von deinen Daten — diese Trennung
ist der ganze Sinn, damit ein kopiertes Daten-Volume nicht auch den Schlüssel preisgibt.

> **Sichere den Schlüssel, getrennt von deinen Daten.** Verlierst du den Schlüssel, sind
> die verschlüsselten Dateien nicht mehr lesbar — Kontenliste und Einstellungen sind
> weg, und du begännest mit einem frischen Admin-Konto neu. Die offizielle
> `docker compose`-Installation hält den Schlüssel in einem benannten Volume
> (`justdns-keys`), das Updates übersteht, sodass du ihn im Normalbetrieb nie anfässt.
> Nimm dieses Volume einfach in das auf, womit du die Box sicherst — und bewahre dieses
> Backup getrennt vom Daten-Backup auf, sonst hebst du die Trennung wieder auf.

## Beim Update

Beim ersten Start der neuen Version liest sie dein vorhandenes Klartext-`auth.json` und
`config.json`, schreibt sie **verschlüsselt** zurück und macht weiter. Du tust nichts,
und am Anmelden oder am Umgang mit dem Dashboard ändert sich nichts.

Das ist ein Einwegschritt. Eine **ältere** JustDNS-Version kann die verschlüsselten
Dateien nicht lesen; sobald ein Knoten auf dieser Version läuft, rolle sein Image also
nicht zurück, ohne zugleich ein Backup von vor dem Update wiederherzustellen. In einem
[Hochverfügbarkeits](high-availability.de.md)-Cluster verschlüsselt jeder Knoten seine
eigenen Dateien unabhängig; dort braucht das Update keine Abstimmung.

## Für die technisch Neugierigen

Der Schlüssel ist ein 32-Byte-Zufalls-Masterschlüssel, beim ersten Start mit dem
System-CSPRNG erzeugt und nach `JUSTDNS_SECRET_KEY_PATH` (Default `/keys/master.key`)
mit Besitzer-Rechten geschrieben, vor der Nutzung auf die Platte geflusht. Daraus leitet
jede Secret-Datei per **HKDF-SHA256** unter einem eigenen Label (`auth-v1`, `config-v1`)
ihren eigenen Teilschlüssel ab, sodass keine zwei Dateien denselben Schlüssel teilen.
Die Nutzlast wird mit **ChaCha20-Poly1305** versiegelt; der Umschlag auf der Platte ist
`Magic || Nonce (12 Byte) || Geheimtext || Tag`, mit frischer Zufalls-Nonce pro Schreiben.

Das Laden ist **fail-closed**: Eine Datei mit dem Versiegelungs-Magic, die sich nicht
öffnen lässt (fehlender oder falscher Schlüssel, oder manipuliert), wird als Fehler
gemeldet und der Knoten verweigert den Start — statt für „keine Konten" gehalten zu
werden und still leer zu starten, was dem, der zuerst die Einrichtungsseite erreicht,
die Kontrolle gäbe. Eine Klartext-Altdatei (ohne Magic) wird einmal gelesen und sofort
versiegelt zurückgeschrieben — das ist das transparente Update von oben.

Alles hängt an der Datei auf der Platte, der Mechanismus ist also unabhängig vom
HA-Sync (dessen Peer-Transport eine eigene, getrennte Verschlüsselung über den
gemeinsamen Cluster-Schlüssel hat).

Betreibst du JustDNS ohne die offizielle Compose-Datei — etwa per nacktem `docker run` —
mounte selbst ein persistentes Volume auf `/keys` (`-v justdns-keys:/keys`). Sonst wird
der Schlüssel bei jedem Neu-Erstellen des Containers neu erzeugt; weil das Laden
fail-closed ist, bekommst du dann beim nächsten Start einen klaren Fehler „Schlüssel
fehlt oder passt nicht" statt stillem Datenverlust, und das Mounten des ursprünglichen
Schlüssel-Volumes stellt ihn wieder her.
