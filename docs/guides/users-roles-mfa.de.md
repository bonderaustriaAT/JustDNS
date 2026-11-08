# Benutzer, Rollen & Zwei-Faktor

*Eine englische Fassung dieser Anleitung liegt in [users-roles-mfa.md](users-roles-mfa.md).*

JustDNS startete mit einem einzigen Administrator. Ab dieser Version kannst du
**weitere Konten** anlegen, jedem eine **Rolle** geben, die festlegt, was es ändern
darf, und Anmeldungen per **Zwei-Faktor-Authentifizierung (MFA)** absichern —
wahlweise **verpflichtend** für alle oder für einzelne Personen.

Das findest du an zwei Stellen: dein eigenes **Konto** liegt im Konto-Menü oben,
und — nur für Administratoren — **Benutzer** und das **Audit-Log** stehen in der
Gruppe **System** der Seitenleiste (am Handy fährt die Seitenleiste über den
Menü-Knopf oben links aus).

## Rollen

Jedes Konto hat eine von drei Rollen, von der mächtigsten zur eingeschränktesten:

| Rolle | Darf |
| --- | --- |
| **Administrator** | Alles: Benutzerverwaltung, Einstellungen, TLS, Backups — plus alles darunter. |
| **Bearbeiter** | Filterinhalte ändern: Block-/Allow-Listen, DNS-Rewrites, Geräte, Gruppen, Zeitpläne und Cache leeren. Nicht Einstellungen, Benutzer, TLS oder Backups. |
| **Betrachter** | Nur lesen. Jede Seite sehen, nichts ändern. |

Eine höhere Rolle kann immer alles, was eine niedrigere kann. Auf einer Seite, die
deine Rolle nur lesen darf, erscheint oben ein kurzer **Nur-Lese**-Hinweis, damit
du siehst, dass eine Änderung abgelehnt würde, bevor du es versuchst. Der Server
erzwingt jede Rolle selbst — ein ausgeblendeter Knopf ist Bequemlichkeit, nicht die
Sperre.

Dein bestehender Administrator bleibt unverändert: ein Konto von vor diesem Feature
lädt als **Administrator** mit Vollzugriff.

> **Der letzte Administrator ist geschützt.** JustDNS weigert sich, den einzigen
> verbleibenden Administrator herabzustufen oder zu löschen – du kannst dich also
> nie selbst aus der Verwaltung aussperren.

## Zwei-Faktor-Authentifizierung (MFA)

Zwei-Faktor fügt der Anmeldung einen zweiten Schritt hinzu: nach deinem Passwort
einen **6-stelligen Code** aus einer Authenticator-App auf dem Handy (Google
Authenticator, Microsoft Authenticator, Aegis und ähnliche). Selbst wer dein
Passwort kennt, kommt ohne dein Handy nicht hinein.

Beim Einrichten bekommst du außerdem **zehn Wiederherstellungscodes**. Jeder
funktioniert **einmal** anstelle des App-Codes, falls du das Handy nicht zur Hand
hast. Sie werden **nur einmal** angezeigt – bewahre sie sicher auf.

### Für dich selbst aktivieren

1. Öffne **Konto** im Menü.
2. Klick unter **Zwei-Faktor-Authentifizierung** auf **Einrichten**.
3. **Scanne den QR-Code** mit deiner Authenticator-App (oder gib den Schlüssel von
   Hand ein).
4. **Speichere deine Wiederherstellungscodes.**
5. Gib den 6-stelligen Code ein, den die App nun zeigt, und klick auf **Zwei-Faktor
   aktivieren**.

Ab der nächsten Anmeldung fragt JustDNS nach deinem Passwort einen Code ab.

### Deaktivieren oder neue Wiederherstellungscodes

Auf der **Konto**-Seite bei aktivem MFA:

- **Wiederherstellungscodes neu erzeugen** – erzeugt einen neuen Satz und macht den
  alten ungültig. Mit dem Passwort bestätigen.
- **Deaktivieren** – entfernt MFA (mit Passwort bestätigen). Nicht verfügbar, wenn
  MFA für dein Konto **vorgeschrieben** ist.

## MFA erzwingen (Administratoren)

Als Administrator kannst du MFA auf zwei Wegen verpflichtend machen – sie
kombinieren sich (der Server erzwingt das **Oder** aus beiden, eines davon genügt
also, um MFA für ein Konto vorzuschreiben):

- **Für alle** – das Banner **Zwei-Faktor für alle Konten verpflichtend** oben auf
  der **Benutzer**-Seite. Schalte es ein, und jedes Konto muss MFA nutzen. Solange
  es an ist, ist der Schalter je Person wirkungslos und zeigt stattdessen
  **Erzwungen**.
- **Für eine Person** – auf der **Benutzer**-Seite den Schalter in der Spalte
  **Pflicht** neben dem Konto umlegen.

Ist MFA für jemanden vorgeschrieben, der es nicht eingerichtet hat, führt die
nächste Anmeldung durch die Einrichtung – **bevor** das Dashboard erscheint. Es
lässt sich nicht überspringen, und solange es Pflicht bleibt, kann die Person MFA
danach nicht abschalten.

## Benutzer verwalten (Administratoren)

Öffne **Benutzer** in der Gruppe **System** der Seitenleiste.

### Jemanden hinzufügen

Klick auf **+ Benutzer anlegen**. Trage im Dialog einen **Benutzernamen**, ein
erstes **Passwort** (mindestens 8 Zeichen) ein, wähl eine **Rolle**, leg optional
den Schalter **Zwei-Faktor verpflichtend** um und klick auf **Anlegen**. Teile das Passwort über
einen vertrauenswürdigen Kanal mit; die Person kann es auf ihrer eigenen
Konto-Seite ändern.

### Ein Konto ändern

Jede Zeile erlaubt dir:

- Die **Rolle** im Auswahlmenü ändern.
- Den Schalter in der Spalte **Pflicht** umlegen, um MFA für die Person zu verlangen
  (hinter **Erzwungen** verborgen, solange die globale Pflicht oben an ist).
- **Passwort zurücksetzen** – ein neues setzen (z. B. nach einer Aussperrung).
- **Zwei-Faktor zurücksetzen** – MFA nach einem verlorenen Handy löschen. Ist MFA
  weiter Pflicht, richtet die Person es bei der nächsten Anmeldung neu ein.
- Das Konto **löschen**. Dein eigenes kannst du nicht löschen, und der letzte
  Administrator ist geschützt.

## Das Audit-Log (Administratoren)

**Audit-Log** in der Gruppe **System** der Seitenleiste zeigt die jüngsten sicherheitsrelevanten Ereignisse,
neueste zuerst: wer sich angemeldet hat (und fehlgeschlagene Anmeldungen),
Abmeldungen, Konto- und MFA-Änderungen, Benutzerverwaltung, Einstellungs­änderungen
und eingespielte Backups. Jede Zeile zeigt die Zeit, den **Akteur**, die **Aktion**,
das **Ziel** und ob sie **erfolgreich** war.

Es enthält nie ein Passwort, ein Geheimnis oder einen Code – nur, *dass* etwas
passiert ist. Damit beantwortest du „wer hat das wann geändert".

## Gut zu wissen

- Sitzungen liegen nicht auf der Platte, ein **Neustart von JustDNS meldet also alle
  ab** – sie melden sich einfach neu an. MFA und die Konten selbst bleiben erhalten.
- Wiederholt falsche Passwörter oder Codes werden einige Minuten **ratenbegrenzt**,
  um Rateversuche auszubremsen.
- Konten und MFA sind **nicht** Teil eines Konfigurations-Backups – bewusst: ein
  Backup, das du weitergibst, soll keine Passwort-Hashes enthalten. Ein Restore
  rührt die Kontenliste nie an.

---

## Für die technisch Neugierigen

**Wo es liegt.** Konten liegen in `auth.json` auf dem `/data`-Volume: Benutzername,
ein Argon2id-Passwort-Hash, die Rolle und – nach Einrichtung – der MFA-Zustand (ein
base32-TOTP-Secret, ein Bestätigt-Flag und Argon2-Hashes der ungenutzten
Wiederherstellungscodes). Jedes für dieses Feature ergänzte Feld hat einen
serde-Default, sodass eine ältere `auth.json` (ein einzelner Admin ohne Rolle oder
MFA) unverändert als vollwertiger Administrator lädt. Der globale Schalter
*Zwei-Faktor verpflichtend* ist ein Feld in `config.json`.

**TOTP.** Codes sind Standard-TOTP nach RFC 6238: SHA-1, 6 Stellen, 30-Sekunden-
Schritt, mit ±1 Fenster für Uhr-Drift. Der QR-Code ist eine `otpauth://`-URI, die
der Server selbst als SVG rendert – das Secret verlässt die Box also nie über einen
fremden QR-Dienst. Wiederherstellungscodes sind zehn `XXXXX-XXXXX`-Strings; nur ihre
Argon2-Hashes werden gespeichert, und ein benutzter wird sofort entfernt.

**Anmelde-Ablauf.** Die Passwortprüfung erzeugt eine kurzlebige *Pending*-Sitzung.
Hat die Person MFA, wartet die Sitzung im Zustand `PendingMfa`, bis ein gültiger
TOTP- oder Wiederherstellungscode sie hochstuft; ist MFA vorgeschrieben, aber nicht
eingerichtet, wartet sie in `PendingEnroll`, bis die Person sich einrichtet. Nur
eine vollständig hochgestufte Sitzung passiert den Auth-Wächter – ein Pending-Cookie
öffnet nichts. Volle Sitzungen halten eine Woche, Pending-Sitzungen laufen nach zehn
Minuten ab.

**Autorisierung.** Eine Middleware läuft bei jeder API-Anfrage hinter dem Wächter:
sie löst die Sitzung zu einem Benutzer auf, schlägt die Rolle live nach (eine
Rollenänderung greift also sofort) und vergleicht sie mit dem Minimum, das der
Endpunkt braucht – Lesen ist für jeden Angemeldeten offen, Inhaltsänderungen
brauchen Bearbeiter, und Einstellungen, TLS, Backups, Benutzerverwaltung und das
Audit-Log brauchen Administrator. Die Selbstbedienungs-Endpunkte fürs eigene Konto
sind für den Inhaber offen und wirken nur auf ihn selbst.

**Audit-Log.** Ereignisse werden an `audit.log` auf `/data` angehängt, ein
JSON-Objekt pro Zeile, begrenzt auf die jüngsten Einträge, damit die Datei nicht
grenzenlos wächst. Sie wird bei jedem Ereignis atomar neu geschrieben und enthält
keine Geheimnisse – nur Akteur, Aktion, Ziel, Ergebnis und ein kurzes, geheimnisloses
Detail. Sie ist bewusst von Backups ausgenommen.

**Ratenbegrenzung.** Fehlgeschlagene Passwort- und Zweitfaktor-Versuche werden pro
Benutzername im Speicher über ein gleitendes Fünf-Minuten-Fenster gezählt; über
einem Schwellwert werden weitere Versuche mit `429` abgelehnt, bis das Fenster
abläuft. Sie wird bei einem Neustart zurückgesetzt – passend für ein LAN-Gerät,
nicht für einen öffentlichen Server.
