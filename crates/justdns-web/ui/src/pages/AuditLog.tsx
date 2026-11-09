// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useState } from "react";

import { api, type AuditEntry } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Account.module.css";

/** The actions we have a translated label for; any other is shown verbatim. */
const KNOWN_ACTIONS = new Set([
  "login.success",
  "login.failure",
  "login.throttled",
  "logout",
  "setup.admin",
  "account.password",
  "mfa.enable",
  "mfa.disable",
  "mfa.recovery",
  "user.create",
  "user.delete",
  "user.role",
  "user.password",
  "user.mfaRequired",
  "user.mfaReset",
  "settings.update",
  "backup.restore",
]);

/** The security audit trail (administrators only): recent sign-ins and account
 *  and settings changes, newest first. */
export function AuditLog() {
  const { t, lang } = useI18n();
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(() => {
    setLoading(true);
    api
      .audit(300)
      .then(setEntries)
      .catch(() => {
        /* keep the last-known entries */
      })
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  function actionLabel(action: string): string {
    return KNOWN_ACTIONS.has(action) ? t(`audit.a.${action}` as MessageKey) : action;
  }

  function formatTime(iso: string): string {
    const date = new Date(iso);
    return Number.isNaN(date.getTime()) ? iso : date.toLocaleString(lang);
  }

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t("audit.title")}</h1>
        <p className={styles.lead}>{t("audit.lede")}</p>
      </header>

      <section className={styles.cardWide}>
        <div className={styles.actions}>
          <button type="button" className={styles.button} onClick={refresh} disabled={loading}>
            {t("audit.refresh")}
          </button>
        </div>

        {entries.length === 0 ? (
          <p className={styles.empty}>{t("audit.empty")}</p>
        ) : (
          <div className={styles.tableWrap}>
            <table className={styles.table}>
              <thead>
                <tr>
                  <th className={styles.th}>{t("audit.time")}</th>
                  <th className={styles.th}>{t("audit.actor")}</th>
                  <th className={styles.th}>{t("audit.action")}</th>
                  <th className={styles.th}>{t("audit.target")}</th>
                  <th className={styles.th}>{t("audit.outcome")}</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((e, i) => (
                  <tr key={`${e.time}-${i}`}>
                    <td className={styles.td}>{formatTime(e.time)}</td>
                    <td className={styles.td}>{e.actor}</td>
                    <td className={styles.td}>
                      {actionLabel(e.action)}
                      {e.detail && <span className={styles.auditDetail}> · {e.detail}</span>}
                    </td>
                    <td className={styles.td}>{e.target ?? "—"}</td>
                    <td className={styles.td}>
                      <span className={e.outcome === "success" ? styles.badgeOn : styles.badge}>
                        {e.outcome === "success" ? t("audit.success") : t("audit.failure")}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
