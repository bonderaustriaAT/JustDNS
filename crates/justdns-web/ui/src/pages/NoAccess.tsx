// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Link } from "react-router-dom";

import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useAuth } from "../state/auth";
import type { Role } from "../lib/api";
import styles from "./NoAccess.module.css";

/** The display label for each role, reused from the user management page. */
const ROLE_LABELS: Record<Role, MessageKey> = {
  admin: "users.roleAdmin",
  editor: "users.roleEditor",
  viewer: "users.roleViewer",
};

/** What a signed-in non-admin sees when they follow a deep link into an
 *  admin-only area (a bookmark, a shared URL, a notification pointing at the
 *  cluster page): a plain explanation with their own role, instead of a silent
 *  redirect to the overview that reads like a broken link. On a read-only
 *  follower the note adds where the cluster is actually managed. */
export function NoAccess() {
  const { t } = useI18n();
  const { role, isPrimary, primaryUrl } = useAuth();
  const label = role ? t(ROLE_LABELS[role]) : "";

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>{t("common.noAccessTitle")}</h1>
      <p className={styles.lead}>{t("common.noAccessBody", { role: label })}</p>
      {!isPrimary && (
        <p className={styles.note}>
          {t("common.followerReadOnly")}
          {primaryUrl ? ` ${primaryUrl}` : ""}
        </p>
      )}
      <Link className={styles.back} to="/">
        {t("common.backToOverview")}
      </Link>
    </div>
  );
}
