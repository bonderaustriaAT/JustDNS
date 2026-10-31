import { useState, type FormEvent } from "react";

import { ApiError } from "../lib/api";
import { useAuth } from "../state/auth";
import styles from "./Auth.module.css";

/// Minimum password length, mirrored from the server's own check so the user
/// hears about a too-short password before the request is sent.
const MIN_PASSWORD = 8;

export function CreateAdmin() {
  const { setup } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);

    if (password.length < MIN_PASSWORD) {
      setError(`Das Passwort muss mindestens ${MIN_PASSWORD} Zeichen haben.`);
      return;
    }
    if (password !== confirm) {
      setError("Die beiden Passwörter stimmen nicht überein.");
      return;
    }

    setBusy(true);
    try {
      await setup(username, password);
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 409) {
        // Someone else finished setup first — reload lands on the login screen.
        setError("Es wurde bereits ein Administrator eingerichtet. Bitte Seite neu laden.");
      } else {
        setError("Einrichtung fehlgeschlagen. Bitte erneut versuchen.");
      }
      setBusy(false);
    }
  }

  return (
    <div className={styles.screen}>
      <div className={styles.card}>
        <div className={styles.brand}>
          <span className={styles.mark} aria-hidden="true" />
          <span className={styles.wordmark}>JustDNS</span>
        </div>
        <div className={styles.head}>
          <h1 className={styles.title}>Administrator einrichten</h1>
          <p className={styles.lede}>
            Dies ist die erste Anmeldung. Lege das Administratorkonto an, mit dem du das Dashboard
            künftig öffnest.
          </p>
        </div>
        <form className={styles.form} onSubmit={onSubmit}>
          {error && (
            <p className={styles.error} role="alert">
              {error}
            </p>
          )}
          <label className={styles.field}>
            <span className={styles.label}>Benutzername</span>
            <input
              className={styles.input}
              type="text"
              autoComplete="username"
              autoFocus
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
            />
          </label>
          <label className={styles.field}>
            <span className={styles.label}>Passwort</span>
            <input
              className={styles.input}
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
            <span className={styles.hint}>Mindestens {MIN_PASSWORD} Zeichen.</span>
          </label>
          <label className={styles.field}>
            <span className={styles.label}>Passwort wiederholen</span>
            <input
              className={styles.input}
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              required
            />
          </label>
          <button className={styles.submit} type="submit" disabled={busy}>
            {busy ? "Einrichten …" : "Administrator anlegen"}
          </button>
        </form>
      </div>
    </div>
  );
}
