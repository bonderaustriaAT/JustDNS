// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState, type FormEvent } from "react";

import { api, ApiError, type Role, type UserInfo } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useAuth } from "../state/auth";
import { useSettings } from "../state/settings";
import { Modal } from "../components/Modal";
import { Toggle } from "../components/Toggle";
import styles from "./Account.module.css";

const ROLES: Role[] = ["admin", "editor", "viewer"];
const MIN_PASSWORD = 8;

function roleLabelKey(role: Role) {
  return role === "admin" ? "users.roleAdmin" : role === "editor" ? "users.roleEditor" : "users.roleViewer";
}

/** User management (administrators only): create accounts, change roles, require
 *  two-factor, reset a password or a lost second factor, and delete accounts. New
 *  accounts are created through a dialog so the page stays a clean account list. */
export function Users() {
  const { t } = useI18n();
  const { username: me } = useAuth();
  const { settings, update } = useSettings();
  const globalMfa = settings.requireMfa;
  const [users, setUsers] = useState<UserInfo[]>([]);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  // Add-user dialog.
  const [addOpen, setAddOpen] = useState(false);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<Role>("viewer");
  const [requireMfa, setRequireMfa] = useState(false);
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);

  async function refresh() {
    try {
      setUsers(await api.users());
    } catch {
      // Keep the last-known list on a transient failure.
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  function report(error: unknown) {
    setMsg({ ok: false, text: error instanceof ApiError ? error.message : t("auth.loginFailed") });
  }

  function openAdd() {
    setAddError(null);
    setName("");
    setPassword("");
    setRole("viewer");
    setRequireMfa(false);
    setAddOpen(true);
  }

  async function onAdd(event: FormEvent) {
    event.preventDefault();
    setAddError(null);
    if (password.length < MIN_PASSWORD) {
      setAddError(t("auth.passwordTooShort", { min: MIN_PASSWORD }));
      return;
    }
    setAdding(true);
    try {
      await api.addUser(name.trim(), password, role, requireMfa);
      setAddOpen(false);
      setMsg({ ok: true, text: t("users.saved") });
      await refresh();
    } catch (cause) {
      setAddError(cause instanceof ApiError ? cause.message : t("auth.loginFailed"));
    } finally {
      setAdding(false);
    }
  }

  async function changeRole(user: string, nextRole: Role) {
    setMsg(null);
    try {
      await api.setUserRole(user, nextRole);
      setMsg({ ok: true, text: t("users.saved") });
      await refresh();
    } catch (cause) {
      report(cause);
      await refresh();
    }
  }

  async function toggleRequired(user: string, required: boolean) {
    setMsg(null);
    try {
      await api.setUserMfaRequired(user, required);
      setMsg({ ok: true, text: t("users.saved") });
      await refresh();
    } catch (cause) {
      report(cause);
      await refresh();
    }
  }

  async function resetPassword(user: string) {
    const nextPw = window.prompt(t("users.newPasswordFor", { name: user }));
    if (nextPw === null) return;
    setMsg(null);
    if (nextPw.length < MIN_PASSWORD) {
      setMsg({ ok: false, text: t("auth.passwordTooShort", { min: MIN_PASSWORD }) });
      return;
    }
    try {
      await api.resetUserPassword(user, nextPw);
      setMsg({ ok: true, text: t("users.passwordReset") });
    } catch (cause) {
      report(cause);
    }
  }

  async function resetMfa(user: string) {
    if (!window.confirm(t("users.confirmResetMfa", { name: user }))) return;
    setMsg(null);
    try {
      await api.resetUserMfa(user);
      setMsg({ ok: true, text: t("users.mfaResetDone") });
      await refresh();
    } catch (cause) {
      report(cause);
    }
  }

  async function remove(user: string) {
    if (!window.confirm(t("users.confirmDelete", { name: user }))) return;
    setMsg(null);
    try {
      await api.removeUser(user);
      setMsg({ ok: true, text: t("users.deleted") });
      await refresh();
    } catch (cause) {
      report(cause);
    }
  }

  return (
    <div className={styles.page}>
      <div className={styles.topRow}>
        <header className={styles.header}>
          <h1 className={styles.title}>{t("users.title")}</h1>
          <p className={styles.lead}>{t("users.lede")}</p>
        </header>
        <button type="button" className={styles.buttonPrimary} onClick={openAdd}>
          {t("users.addButton")}
        </button>
      </div>

      {/* Global two-factor requirement: the OR counterpart to each account's own
          flag, so an admin can enforce it for everyone in one place. */}
      <section className={styles.mfaBanner} data-on={globalMfa || undefined}>
        <span className={styles.mfaShield} aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round">
            <path d="M12 3l7 3v5c0 4.2-2.8 7.4-7 9-4.2-1.6-7-4.8-7-9V6l7-3z" />
            <path d="M9 12l2 2 4-4" strokeLinecap="round" />
          </svg>
        </span>
        <div className={styles.mfaBannerText}>
          <span className={styles.mfaBannerTitle}>{t("users.globalMfa")}</span>
          <span className={styles.mfaBannerDesc}>
            {globalMfa ? t("users.globalMfaActive") : t("users.globalMfaDesc")}
          </span>
        </div>
        <Toggle
          checked={globalMfa}
          onChange={(on) => update("requireMfa", on)}
          label={t("users.globalMfa")}
        />
      </section>

      {msg && (
        <p className={msg.ok ? styles.noticeOk : styles.noticeErr} role="alert">
          {msg.text}
        </p>
      )}

      {/* The account list. */}
      <section className={styles.cardWide}>
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th className={styles.th}>{t("users.username")}</th>
                <th className={styles.th}>{t("users.role")}</th>
                <th className={styles.th}>{t("users.mfaColumn")}</th>
                <th className={styles.th}>{t("users.pflichtColumn")}</th>
                <th className={styles.th} aria-label="actions" />
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.username}>
                  <td className={styles.td}>
                    <strong>{u.username}</strong>
                    {u.username === me && <span className={styles.you}> · {t("users.you")}</span>}
                  </td>
                  <td className={styles.td}>
                    <select
                      className={styles.select}
                      value={u.role}
                      onChange={(e) => changeRole(u.username, e.target.value as Role)}
                      aria-label={t("users.role")}
                    >
                      {ROLES.map((r) => (
                        <option key={r} value={r}>
                          {t(roleLabelKey(r))}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td className={styles.td}>
                    <span className={u.mfaEnabled ? styles.badgeOn : styles.badge}>
                      {u.mfaEnabled && <span className={styles.badgeDot} aria-hidden="true" />}
                      {u.mfaEnabled ? t("users.mfaOn") : t("users.mfaOff")}
                    </span>
                  </td>
                  <td className={styles.td}>
                    {globalMfa ? (
                      <span className={styles.enforcedBadge} title={t("users.enforcedGlobally")}>
                        {t("users.enforced")}
                      </span>
                    ) : (
                      <Toggle
                        checked={u.mfaRequired}
                        onChange={(on) => toggleRequired(u.username, on)}
                        label={t("users.requireMfa")}
                      />
                    )}
                  </td>
                  <td className={styles.td}>
                    <div className={styles.rowActions}>
                      <button
                        type="button"
                        className={styles.button}
                        onClick={() => resetPassword(u.username)}
                      >
                        {t("users.resetPassword")}
                      </button>
                      {u.mfaEnabled && (
                        <button
                          type="button"
                          className={styles.button}
                          onClick={() => resetMfa(u.username)}
                        >
                          {t("users.resetMfa")}
                        </button>
                      )}
                      <button
                        type="button"
                        className={styles.buttonDanger}
                        onClick={() => remove(u.username)}
                        disabled={u.username === me}
                      >
                        {t("users.delete")}
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      {/* Create a user. */}
      <Modal open={addOpen} onClose={() => setAddOpen(false)} title={t("users.addDialog")}>
        <form className={styles.form} onSubmit={onAdd}>
          {addError && (
            <p className={styles.noticeErr} role="alert">
              {addError}
            </p>
          )}
          <label className={styles.field}>
            <span className={styles.label}>{t("users.username")}</span>
            <input
              className={styles.input}
              type="text"
              autoComplete="off"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </label>
          <label className={styles.field}>
            <span className={styles.label}>{t("users.password")}</span>
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
            <span className={styles.label}>{t("users.role")}</span>
            <select
              className={styles.select}
              value={role}
              onChange={(e) => setRole(e.target.value as Role)}
            >
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {t(roleLabelKey(r))}
                </option>
              ))}
            </select>
            <span className={styles.hint}>
              {t(
                role === "admin"
                  ? "users.roleAdminHint"
                  : role === "editor"
                    ? "users.roleEditorHint"
                    : "users.roleViewerHint",
              )}
            </span>
          </label>
          <label className={styles.toggleField}>
            <input
              type="checkbox"
              className={styles.toggleInput}
              checked={requireMfa}
              onChange={(e) => setRequireMfa(e.target.checked)}
            />
            <span className={styles.toggleTrack} aria-hidden="true">
              <span className={styles.toggleKnob} />
            </span>
            {t("users.requireMfa")}
          </label>
          <div className={styles.actions}>
            <button className={styles.buttonPrimary} type="submit" disabled={adding}>
              {adding ? t("users.creating") : t("users.create")}
            </button>
            <button type="button" className={styles.button} onClick={() => setAddOpen(false)}>
              {t("account.cancel")}
            </button>
          </div>
        </form>
      </Modal>
    </div>
  );
}
