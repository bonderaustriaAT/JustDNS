# JustDNS aktualisieren

*Eine englische Fassung dieser Anleitung liegt unter [updates.md](updates.md).*

JustDNS prüft selbstständig auf neue Versionen und zeigt eine neue Ausgabe auf der
Seite **System & Diagnose** mit Änderungsprotokoll und einem **Jetzt updaten**-Knopf
an. Ein Klick legt eine Sicherung an, lädt die neue Version und startet den
Container neu — ohne SSH, ohne `docker`-Befehle.

## Erkennen, dass ein Update bereitsteht

Jede Instanz prüft leise auf GitHub nach dem neuesten Release-Tag (kein Konto, kein
Token nötig — es werden nur öffentliche Tags gelesen). Gibt es eine neuere Version:

- Oben im Dashboard erscheint für Admins ein **grünes Banner** mit Link zum Update.
- Die Seite **System & Diagnose** zeigt eine **Update**-Karte mit der neuen Version,
  den Neuerungen (direkt aus dem Änderungsprotokoll) und dem **Jetzt updaten**-Knopf.

Mit **Erneut prüfen** auf der Karte kannst du jederzeit sofort nachsehen. Die
Update-Prüfung lässt sich in den Einstellungen komplett abschalten (für eine
abgeschottete Installation) — ohne deinen Klick wird nie etwas geladen oder
installiert.

## Ein-Klick-Update durchführen

Drücke **Jetzt updaten**. Es öffnet sich ein Dialog, der:

1. Einen **Sicherungspfad vorschlägt** auf dem Datenträger (z. B.
   `/data/backups/justdns-backup-…json`). Er muss unter `/data` liegen, damit die
   Sicherung das Neuerstellen des Containers übersteht — genau darum geht es.
2. **Wiederherstellungsdatei herunterladen** anbietet, damit du zusätzlich eine
   Kopie deiner Konfiguration auf dem eigenen Rechner behältst.

Drücke **Update starten**. JustDNS schreibt die Sicherung, dann lädt der Updater das
neue Image und erstellt den Container neu. Die Karte zeigt den Fortschritt
(*laden → neu starten → auf Neustart warten*); das Dashboard verbindet sich kurz
neu, während die neue Version hochkommt, und meldet dann Erfolg.

### Nach dem Update

Nichts zu tun. Auf dem Host liegen immer genau zwei Images — die eben installierte
Version und die davor — ein manuelles Zurückrollen ist also stets einen Schritt
entfernt, während ältere Images automatisch verschwinden. Der Updater räumt sie
direkt nach einem erfolgreichen Update auf; es gibt keinen Knopf zu drücken.

## Installationen ohne Updater (manueller Ausweg)

Ein-Klick-Updates brauchen die offizielle Installation (siehe unten), die einen
kleinen **Updater**-Container mitbringt. Auf einer Installation ohne ihn zeigt der
**Jetzt updaten**-Dialog stattdessen den genauen Befehl für deinen Docker-Host —
und lässt dich die Wiederherstellungsdatei trotzdem zuerst herunterladen. Das
Update ist dann ein Copy-&-Paste.

## Die offizielle Installation (damit Ein-Klick-Updates gehen)

Das Ein-Klick-Update treibt ein **Updater-Sidecar**: ein winziger Container, der den
Docker-Socket hat und den JustDNS-Container für dich neu erstellt. Der DNS-Prozess
selbst bekommt den Socket nie. Installation mit dem beiliegenden Compose-File:

```sh
# aus dem Verzeichnis deploy/ eines Checkouts oder den Dateien unter /opt/justdns
cp .env.example .env     # JUSTDNS_IMAGE / JUSTDNS_TAG bei Bedarf anpassen
docker compose up -d
```

Das startet zwei Container: `justdns` (Resolver und Dashboard) und
`justdns-updater` (das Sidecar, ohne Ports und ohne eingehendes Netz). Ab da sind
Updates ein Klick im Dashboard.

Das Standard-Image kommt aus der **GitHub Container Registry** (`ghcr.io`), bei jedem
Release von GitHub Actions gebaut — läuft also überall ohne Login. Zum Selbst-Hosten
des Images `JUSTDNS_IMAGE` (und `JUSTDNS_UPDATER_IMAGE`) in der `.env` auf dein eigenes
Registry zeigen lassen.

---

## Für die technisch Neugierigen

**Der Prozess fasst Docker nie an.** Dem nach außen erreichbaren DNS-/API-Prozess
den Docker-Socket zu geben wäre ein unnötiges Risiko — und ein Container kann sich
ohnehin nicht selbst neu erstellen oder sein eigenes Image löschen. Deshalb *liest*
der Web-Prozess nur GitHub und *schreibt* eine kleine Anforderungsdatei auf das
gemeinsame Datenvolume (`/data/update/request.json`). Das isolierte Updater-Sidecar
beobachtet diese Datei, führt Pull/Neustart aus und räumt danach alte Images auf und
schreibt den Fortschritt nach `/data/update/status.json`. Eine Heartbeat-Datei sagt
der Oberfläche, dass das Sidecar lebt — so kann sie Ein-Klick statt des geführten
Befehls anbieten.

**Alte Images werden automatisch aufgeräumt.** Nach einem erfolgreichen Neustart
behält das Sidecar genau das neue Image und seinen direkten Vorgänger und entfernt
alles Ältere — so sammeln sich auf dem Host keine veralteten Images an, und ein
Zurückrollen in einem Schritt ist trotzdem jederzeit möglich. Der Admin muss nichts
entscheiden oder anklicken.

**Nur eine Version überquert die Grenze.** Die Anforderung trägt lediglich eine
Release-Version (als `vX.Y.Z` geprüft). Das Sidecar bildet den Image-Namen aus seiner
eigenen festen Umgebungsvariable `JUSTDNS_IMAGE`, sodass selbst ein kompromittierter
Web-Prozess es nicht dazu bringen kann, ein beliebiges Image zu ziehen oder zu
löschen. Das Neuerstellen ist ein schlichtes `docker compose up -d justdns` mit dem
neuen Tag in `.env`; der Updater läuft dabei durch — so kann er melden, dass das
Update fertig ist.

**Quelle und Änderungsprotokoll.** Die neueste Ausgabe ist der höchste `vX.Y.Z`-Tag
aus der GitHub-Tags-API (Vorabversionen und Nicht-Versions-Tags werden ignoriert);
gezeigt wird der passende Abschnitt der `CHANGELOG.md` zu diesem Tag. Beides ist
öffentlich, daher funktioniert es ohne Token out of the box. Für einen Fork lässt
sich das Repository per `JUSTDNS_UPDATE_REPO` übersteuern.

**Alles ist admin-only und auditiert.** Die `/api/update*`-Endpunkte erfordern die
Admin-Rolle; das Starten eines Updates wird im [Audit-Protokoll](audit.de.md)
festgehalten. Das Aufräumen alter Images ist die eigene Hauswirtschaft des Sidecars
und braucht keinen Endpunkt.
