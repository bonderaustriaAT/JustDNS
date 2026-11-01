import { useState, type FormEvent } from "react";

import { ApiError } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useAuth } from "../state/auth";
import styles from "./Auth.module.css";

/// Minimum password length, mirrored from the server's own check so the user
/// hears about a too-short password before the request is sent.
const MIN_PASSWORD = 8;

export function CreateAdmin() {
  const { setup } = useAuth();
  const { t } = useI18n();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);

    if (password.length < MIN_PASSWORD) {
      setError(t("auth.passwordTooShort", { min: MIN_PASSWORD }));
      return;
    }
    if (password !== confirm) {
      setError(t("auth.passwordsDiffer"));
      return;
    }

    setBusy(true);
    try {
      await setup(username, password);
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 409) {
        // Someone else finished setup first — reload lands on the login screen.
        setError(t("auth.alreadyConfigured"));
      } else {
        setError(t("auth.setupFailed"));
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
          <h1 className={styles.title}>{t("auth.setupTitle")}</h1>
          <p className={styles.lede}>{t("auth.setupLede")}</p>
        </div>
        <form className={styles.form} onSubmit={onSubmit}>
          {error && (
            <p className={styles.error} role="alert">
              {error}
            </p>
          )}
          <label className={styles.field}>
            <span className={styles.label}>{t("auth.username")}</span>
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
            <span className={styles.label}>{t("auth.password")}</span>
            <input
              className={styles.input}
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
            <span className={styles.hint}>{t("auth.minChars", { min: MIN_PASSWORD })}</span>
          </label>
          <label className={styles.field}>
            <span className={styles.label}>{t("auth.repeatPassword")}</span>
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
            {busy ? t("auth.creating") : t("auth.createAdmin")}
          </button>
        </form>
      </div>
    </div>
  );
}
