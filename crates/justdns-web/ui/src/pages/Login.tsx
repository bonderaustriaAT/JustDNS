import { useState, type FormEvent } from "react";

import { api, ApiError } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useAuth } from "../state/auth";
import { MfaEnroll } from "../components/MfaEnroll";
import styles from "./Auth.module.css";

type Phase = "password" | "mfa" | "enroll";

export function Login() {
  const { login, completeLogin } = useAuth();
  const { t } = useI18n();
  const [phase, setPhase] = useState<Phase>("password");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onPassword(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const status = await login(username, password);
      if (status === "mfa") setPhase("mfa");
      else if (status === "enroll") setPhase("enroll");
      // "authenticated" re-renders the app into the dashboard; nothing to do.
      setBusy(false);
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 429) {
        setError(t("auth.tooManyAttempts"));
      } else if (cause instanceof ApiError && cause.status === 401) {
        setError(t("auth.wrongCredentials"));
      } else {
        setError(t("auth.loginFailed"));
      }
      setBusy(false);
    }
  }

  async function onMfa(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await api.loginMfa(code.trim());
      await completeLogin();
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 429) {
        setError(t("auth.tooManyAttempts"));
      } else {
        setError(t("auth.invalidCode"));
      }
      setBusy(false);
    }
  }

  function backToPassword() {
    setPhase("password");
    setPassword("");
    setCode("");
    setError(null);
    setBusy(false);
  }

  return (
    <div className={styles.screen}>
      <div className={styles.card}>
        <div className={styles.brand}>
          <span className={styles.mark} aria-hidden="true" />
          <span className={styles.wordmark}>JustDNS</span>
        </div>

        {phase === "password" && (
          <>
            <div className={styles.head}>
              <h1 className={styles.title}>{t("auth.loginTitle")}</h1>
              <p className={styles.lede}>{t("auth.loginLede")}</p>
            </div>
            <form className={styles.form} onSubmit={onPassword}>
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
                  autoComplete="current-password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                />
              </label>
              <button className={styles.submit} type="submit" disabled={busy}>
                {busy ? t("auth.signingIn") : t("auth.signIn")}
              </button>
            </form>
          </>
        )}

        {phase === "mfa" && (
          <>
            <div className={styles.head}>
              <h1 className={styles.title}>{t("auth.mfaTitle")}</h1>
              <p className={styles.lede}>{t("auth.mfaLede")}</p>
            </div>
            <form className={styles.form} onSubmit={onMfa}>
              {error && (
                <p className={styles.error} role="alert">
                  {error}
                </p>
              )}
              <label className={styles.field}>
                <span className={styles.label}>{t("auth.code")}</span>
                <input
                  className={styles.input}
                  type="text"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  autoFocus
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  placeholder="000000"
                  required
                />
              </label>
              <button className={styles.submit} type="submit" disabled={busy}>
                {busy ? t("auth.verifying") : t("auth.verify")}
              </button>
              <button type="button" className={styles.linkButton} onClick={backToPassword}>
                {t("auth.backToLogin")}
              </button>
            </form>
          </>
        )}

        {phase === "enroll" && (
          <>
            <div className={styles.head}>
              <h1 className={styles.title}>{t("auth.enrollTitle")}</h1>
              <p className={styles.lede}>{t("auth.enrollLede")}</p>
            </div>
            <MfaEnroll
              begin={() => api.enrollBegin()}
              confirm={async (c) => {
                await api.enrollConfirm(c);
              }}
              onDone={() => completeLogin()}
              doneLabel={t("auth.verify")}
            />
            <button type="button" className={styles.linkButton} onClick={backToPassword}>
              {t("auth.backToLogin")}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
