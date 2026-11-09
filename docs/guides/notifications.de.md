# Benachrichtigungen & Alerts

*Die englische Fassung dieses Leitfadens steht in [notifications.md](notifications.md).*

JustDNS kann sich **von selbst bei dir melden**, wenn im Netz etwas Aufmerksamkeit
braucht, statt darauf zu warten, dass du es im Dashboard bemerkst. Einschalten, einen
Webhook hinterlegen — und JustDNS schickt eine kurze Nachricht, wenn eine Cluster-Node
ausfällt, eine neue Version bereitsteht, ein TLS-Zertifikat bald abläuft oder ein
einzelnes Gerät plötzlich verdächtig viel blockiert.

Alles wird unter **Einstellungen → Benachrichtigungen** konfiguriert. Es gibt kein
separates Konto und keinen Cloud-Dienst dazwischen: JustDNS sendet direkt an den von dir
gewählten Webhook, von jeder Node aus.

## Kanal wählen

JustDNS stellt über einen **Webhook** zu — eine URL, die per HTTP-POST eine Nachricht
empfängt. Wähle das **Format** passend zu deinem Empfänger:

- **Generisch / ntfy (JSON)** — ein schlichter `{ "title", "message" }`-Body. Das nehmen
  [ntfy](https://ntfy.sh), [Gotify](https://gotify.net) und die meisten selbstgehosteten
  Empfänger an. Die einfachste Wahl und ein guter Standard.
- **ntfy (Klartext)** — die Nachricht als Klartext-Body mit dem Titel als
  `Title`-Header, wie ntfys eigenes Publish-Format.
- **Slack** und **Discord** — deren *Incoming-Webhook*-Format. Lege im Kanal einen
  Incoming-Webhook an, füge dessen URL ein, und die Nachrichten landen in diesem Kanal.
- **Telegram** — sprich mit [@BotFather](https://t.me/BotFather), um einen Bot anzulegen,
  trage dessen **Token** ins Token-Feld und die **Chat-ID** der Person oder Gruppe ins
  Chat-ID-Feld ein.

Die URL (bzw. der Telegram-Token) enthält oft ein Geheimnis, darum ist das Feld wie ein
Passwort **maskiert**. Gespeichert und gesendet wird trotzdem normal — die Maskierung ist
nur auf dem Bildschirm.

Mit **Test senden** verschickst du eine einzelne Nachricht mit deinen gespeicherten
Einstellungen. Erst speichern (das Formular sichert sich kurz nach dem Tippen von selbst),
dann senden: Der Test nutzt die gespeicherte Konfiguration, nicht das ungespeicherte
Formular. Lehnt der Webhook die Nachricht ab, bekommst du den Fehler direkt angezeigt und
kannst URL oder Format korrigieren, bevor du dich darauf verlässt.

## Worüber JustDNS dich informiert

Vier Ereignisse, jedes einzeln ein- oder ausschaltbar:

- **Node ausgefallen / wieder da** — eine Node in deinem
  [Hochverfügbarkeits](high-availability.de.md)-Cluster antwortet ihren Peers nicht mehr.
  Du bekommst eine Nachricht beim Ausfall und eine weitere bei der Rückkehr, sodass ein
  kurzer Aussetzer und ein echter Ausfall unterscheidbar sind.
- **Update verfügbar** — eine neue JustDNS-Version ist veröffentlicht und lässt sich über
  die Seite [Updates](updates.de.md) einspielen.
- **Zertifikat läuft bald ab** — ein gespeichertes TLS-[Zertifikat](certificates.de.md)
  liegt innerhalb seiner **Vorwarnzeit** vor dem Ablauf (standardmäßig 14 Tage). Der
  Anstoß, rechtzeitig zu erneuern, bevor DoT, DoH oder die [Block-Seite](block-page.de.md)
  ein abgelaufenes Zertifikat ausliefern.
- **Geräte-Traffic-Spike** — ein einzelnes Gerät wird in kurzer Zeit ungewöhnlich oft
  geblockt. Ein Gerät, das plötzlich Hunderte geblockte Anfragen macht, ist ein klassisches
  Zeichen für Malware, die zu Domains auf deinen Blocklisten „nach Hause telefoniert“. Das
  ist eine **Heuristik**, darum **standardmäßig aus**, mit zwei Stellschrauben: der
  **Schwelle** (Blocks pro Gerät) und dem **Fenster** (Minuten). Fang großzügig an und zieh
  es enger, sobald du das Normalmaß deines Netzes kennst.

## Wer was sendet — im Cluster

Jede Node wertet selbst aus und sendet ihre **eigenen** Alerts, und jede Nachricht trägt
den **Instanznamen** der sendenden Node als Präfix, damit du sie auseinanderhältst.
Node-Ausfall, Zert-Ablauf und Geräte-Spike sind Dinge, die jede Node über ihre eigene Welt
weiß — also meldet jede ihr Eigenes. „Update verfügbar“ ist auf jeder Node identisch; damit
du es nicht pro Node bekommst, **sendet es nur eine Node**, die anderen bleiben still.

---

## Für die technisch Neugierigen

**Zugestellt** wird pro Alert mit einem einzelnen blockierenden HTTP-POST, über denselben
`ureq`-Client, den der Cluster für den Peer-Sync nutzt. Jedes Format formt die Anfrage
anders: Generisch, Slack und Discord senden einen kleinen JSON-Body (`{title, message}`,
`{text}` bzw. `{content}`); ntfy sendet den Body als Klartext mit `Title`-Header; Telegram
postet an `https://api.telegram.org/bot<token>/sendMessage` mit Chat-ID und Text. Es gibt
keine Warteschlange und keinen Retry — ein fehlgeschlagener Versand wird protokolliert, und
die nächste Auswertung versucht es erneut.

**Der Evaluator** ist ein Hintergrund-Thread, aus derselben Familie wie Update-Prüfung,
HA-Sync und Aufbewahrungs-Prune. Er wacht etwa einmal pro Minute auf und vergleicht, wenn
Benachrichtigungen an sind, die aktuelle Welt mit einem kleinen In-Memory-Gedächtnis
dessen, worüber er zuletzt alarmiert hat — er feuert also auf **Übergänge**, nicht bei
jedem Tick:

- *Node ausgefallen* kommt aus dem HA-Peer-Roster — ein Peer gilt als down, wenn er einen
  verzeichneten Fehler hat **und** sein letzter erfolgreicher Sync älter als rund drei
  Sync-Intervalle ist. Der Alert feuert, wenn ein Peer in diese Menge eintritt oder sie
  verlässt, nicht während er darin bleibt.
- *Update* feuert einmal pro neuer Versionszeichenkette, aus demselben Update-Snapshot, den
  die Updates-Seite liest.
- *Zert-Ablauf* wird aus der Not-After-Zeit jedes gespeicherten Zertifikats gegen die
  eingestellte Vorwarnzeit berechnet; es alarmiert einmal pro Zertifikat und Tag, solange
  es im Fenster bleibt, nicht jede Minute.
- *Geräte-Spike* zählt per `GROUP BY client` über die geblockten Zeilen im Fenster, nur
  node-lokal (es liest die eigenen Zeilen dieser Node, nicht die clusterweite Vereinigung),
  und alarmiert ein Gerät über der Schwelle mit einer Pro-Gerät-Abklingzeit, damit ein
  lautes Gerät dich nicht zuspammt.

**Die Regel „nur eine Node sendet den Update-Alert“** ist eine einfache Entdopplung: Die
Node mit der kleinsten erreichbaren Node-id im HA-Roster sendet die clusterweit identischen
Alerts. Keine Wahl und kein gemeinsames Lock — nur eine deterministische Entscheidung, die
jede Node aus dem ohnehin vorhandenen Roster treffen kann, sodass genau eine sendet und der
Rest es überspringt.

**Die Konfiguration** reitet in den [Einstellungen](settings.de.md) mit, also synchronisiert
sie im Cluster wie jede andere Einstellung — jede Node ist sich über Kanal, Format und
aktive Ereignisse einig. Webhook-URL und Telegram-Token liegen im Klartext in der
`config.json` auf dem `/data`-Volume, wie die übrige Konfiguration; einen separaten
Secret-Store gibt es in dieser Version nicht. Die Webhook-URL ist ein schreibseitiges
Geheimnis: `GET /api/settings` liefert sie nur einem **Administrator**, für niedrigere
Rollen wird sie geleert, und ein **heruntergeladenes** Backup enthält sie nicht (nach einem
Restore einmal neu eintragen; das knotenlokale Pre-Update-Backup auf `/data` behält sie). Als
Ziel sind außerdem **Link-local-/Cloud-Metadata-Adressen** (etwa `169.254.169.254`) gesperrt;
ein selbstgehostetes ntfy/Gotify im eigenen LAN bleibt erlaubt.
