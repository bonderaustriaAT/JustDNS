import { useState, type FormEvent } from "react";

import { ApiError } from "../lib/api";
import { useAuth } from "../state/auth";
import styles from "./Auth.module.css";

export function Login() {
  const { login } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await login(username, password);
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 401) {
        setError("Benutzername oder Passwort ist falsch.");
      } else {
        setError("Anmeldung fehlgeschlagen. Bitte erneut versuchen.");
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
          <h1 className={styles.title}>Anmelden</h1>
          <p className={styles.lede}>Melde dich an, um das Dashboard zu öffnen.</p>
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
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </label>
          <button className={styles.submit} type="submit" disabled={busy}>
            {busy ? "Anmelden …" : "Anmelden"}
          </button>
        </form>
      </div>
    </div>
  );
}
