// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState, type FormEvent } from "react";

import { api, ApiError, type AccountInfo } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { MfaEnroll } from "../components/MfaEnroll";
import styles from "./Account.module.css";

const MIN_PASSWORD = 8;

/** A signed-in user's own account: change the password, and turn two-factor on
 *  or off (unless it is required for them). */
export function Account() {
  const { t } = useI18n();
  const [info, setInfo] = useState<AccountInfo | null>(null);

  // Change-password form.
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [repeat, setRepeat] = useState("");
  const [pwMsg, setPwMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [pwBusy, setPwBusy] = useState(false);

  // MFA sub-flows.
  const [enrolling, setEnrolling] = useState(false);
  const [confirmPassword, setConfirmPassword] = useState<null | "disable" | "recovery">(null);
  const [password, setPassword] = useState("");
  const [mfaMsg, setMfaMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [mfaBusy, setMfaBusy] = useState(false);
  const [newRecovery, setNewRecovery] = useState<string[] | null>(null);

  async function refresh() {
    try {
      setInfo(await api.account());
    } catch {
      // A transient failure leaves the last-known state in place.
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function onChangePassword(event: FormEvent) {
    event.preventDefault();
    setPwMsg(null);
    if (next.length < MIN_PASSWORD) {
      setPwMsg({ ok: false, text: t("auth.passwordTooShort", { min: MIN_PASSWORD }) });
      return;
    }
    if (next !== repeat) {
      setPwMsg({ ok: false, text: t("auth.passwordsDiffer") });
      return;
    }
    setPwBusy(true);
    try {
      await api.changeOwnPassword(current, next);
      setPwMsg({ ok: true, text: t("account.passwordChanged") });
      setCurrent("");
      setNext("");
      setRepeat("");
    } catch (cause) {
      setPwMsg({
        ok: false,
        text:
          cause instanceof ApiError && cause.status === 401
            ? t("account.passwordWrong")
            : t("auth.loginFailed"),
      });
    } finally {
      setPwBusy(false);
    }
  }

  async function onConfirmPassword(event: FormEvent) {
    event.preventDefault();
    setMfaMsg(null);
    setMfaBusy(true);
    try {
      if (confirmPassword === "disable") {
        await api.accountMfaDisable(password);
        setMfaMsg({ ok: true, text: t("account.mfaDisabled") });
        setConfirmPassword(null);
        setPassword("");
        await refresh();
      } else if (confirmPassword === "recovery") {
        const result = await api.accountMfaRegenerateRecovery(password);
        setNewRecovery(result.recoveryCodes);
        setMfaMsg({ ok: true, text: t("account.recoveryRegenerated") });
        setConfirmPassword(null);
        setPassword("");
      }
    } catch (cause) {
      setMfaMsg({
        ok: false,
        text:
          cause instanceof ApiError && cause.status === 401
            ? t("account.passwordWrong")
            : t("auth.loginFailed"),
      });
    } finally {
      setMfaBusy(false);
    }
  }

  if (!info) {
    return (
      <div className={styles.page}>
        <p className={styles.hint}>…</p>
      </div>
    );
  }

  const roleLabel = t(
    info.role === "admin"
      ? "users.roleAdmin"
      : info.role === "editor"
        ? "users.roleEditor"
        : "users.roleViewer",
  );

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t("account.title")}</h1>
        <p className={styles.lead}>{t("account.lede")}</p>
      </header>

      {/* Identity + role. */}
      <section className={styles.card}>
        <div className={styles.row}>
          <div className={styles.field}>
            <span className={styles.label}>{t("auth.username")}</span>
            <strong>{info.username}</strong>
          </div>
          <div className={styles.field}>
            <span className={styles.label}>{t("account.roleLabel")}</span>
            <span className={styles.badge}>{roleLabel}</span>
          </div>
        </div>
      </section>

      {/* Change password. */}
      <section className={styles.card}>
        <h2 className={styles.sectionTitle}>{t("account.changePassword")}</h2>
        <form className={styles.formNarrow} onSubmit={onChangePassword}>
          {pwMsg && (
            <p className={pwMsg.ok ? styles.noticeOk : styles.noticeErr} role="alert">
              {pwMsg.text}
            </p>
          )}
          <label className={styles.field}>
            <span className={styles.label}>{t("account.currentPassword")}</span>
            <input
              className={styles.input}
              type="password"
              autoComplete="current-password"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
              required
            />
          </label>
          <label className={styles.field}>
            <span className={styles.label}>{t("account.newPassword")}</span>
            <input
              className={styles.input}
              type="password"
              autoComplete="new-password"
              value={next}
              onChange={(e) => setNext(e.target.value)}
              required
            />
            <span className={styles.hint}>{t("auth.minChars", { min: MIN_PASSWORD })}</span>
          </label>
          <label className={styles.field}>
            <span className={styles.label}>{t("account.repeatNewPassword")}</span>
            <input
              className={styles.input}
              type="password"
              autoComplete="new-password"
              value={repeat}
              onChange={(e) => setRepeat(e.target.value)}
              required
            />
          </label>
          <div className={styles.actions}>
            <button className={styles.buttonPrimary} type="submit" disabled={pwBusy}>
              {pwBusy ? t("account.saving") : t("account.save")}
            </button>
          </div>
        </form>
      </section>

      {/* Two-factor. */}
      <section className={styles.card}>
        <h2 className={styles.sectionTitle}>
          {t("account.mfaTitle")}
          <span className={info.mfaEnabled ? styles.badgeOn : styles.badge}>
            {info.mfaEnabled ? t("account.mfaOn") : t("account.mfaOff")}
          </span>
        </h2>

        {mfaMsg && (
          <p className={mfaMsg.ok ? styles.noticeOk : styles.noticeErr} role="alert">
            {mfaMsg.text}
          </p>
        )}

        {info.mfaRequired && <p className={styles.hint}>{t("account.mfaRequiredNote")}</p>}

        {/* Freshly regenerated recovery codes, shown once. */}
        {newRecovery && (
          <div className={styles.step}>
            <span className={styles.stepTitle}>{t("account.newRecoveryTitle")}</span>
            <span className={styles.hint}>{t("mfa.recoveryHint")}</span>
            <div className={styles.codeGrid}>
              {newRecovery.map((rc) => (
                <span key={rc}>{rc}</span>
              ))}
            </div>
            <div className={styles.actions}>
              <button type="button" className={styles.button} onClick={() => setNewRecovery(null)}>
                {t("account.confirm")}
              </button>
            </div>
          </div>
        )}

        {!info.mfaEnabled && !enrolling && (
          <div className={styles.actions}>
            <button
              type="button"
              className={styles.buttonPrimary}
              onClick={() => {
                setMfaMsg(null);
                setEnrolling(true);
              }}
            >
              {t("account.setUpMfa")}
            </button>
          </div>
        )}

        {!info.mfaEnabled && enrolling && (
          <MfaEnroll
            begin={() => api.accountMfaBegin()}
            confirm={async (c) => {
              await api.accountMfaConfirm(c);
            }}
            onDone={() => {
              setEnrolling(false);
              setMfaMsg({ ok: true, text: t("account.mfaEnabledMsg") });
              void refresh();
            }}
            doneLabel={t("mfa.enable")}
          />
        )}

        {info.mfaEnabled && confirmPassword === null && (
          <div className={styles.actions}>
            <button
              type="button"
              className={styles.button}
              onClick={() => {
                setMfaMsg(null);
                setNewRecovery(null);
                setConfirmPassword("recovery");
              }}
            >
              {t("account.regenerateRecovery")}
            </button>
            {!info.mfaRequired && (
              <button
                type="button"
                className={styles.buttonDanger}
                onClick={() => {
                  setMfaMsg(null);
                  setConfirmPassword("disable");
                }}
              >
                {t("account.disableMfa")}
              </button>
            )}
          </div>
        )}

        {info.mfaEnabled && confirmPassword !== null && (
          <form className={styles.form} onSubmit={onConfirmPassword}>
            <label className={styles.field}>
              <span className={styles.label}>{t("account.passwordToConfirm")}</span>
              <input
                className={styles.input}
                type="password"
                autoComplete="current-password"
                autoFocus
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </label>
            <div className={styles.actions}>
              <button
                className={confirmPassword === "disable" ? styles.buttonDanger : styles.buttonPrimary}
                type="submit"
                disabled={mfaBusy}
              >
                {t("account.confirm")}
              </button>
              <button
                type="button"
                className={styles.button}
                onClick={() => {
                  setConfirmPassword(null);
                  setPassword("");
                }}
              >
                {t("account.cancel")}
              </button>
            </div>
          </form>
        )}
      </section>
    </div>
  );
}
