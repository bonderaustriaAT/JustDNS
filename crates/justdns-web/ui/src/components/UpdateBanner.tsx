import { useEffect, useState } from "react";
import { Link } from "react-router-dom";

import { api } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useAuth } from "../state/auth";
import styles from "./UpdateBanner.module.css";

const DISMISS_KEY = "justdns.updateDismissed";

/** A quiet, dismissible banner that appears for admins when a newer release is
 *  available, linking to the System page where the one-click update lives. The
 *  dismissal remembers the version, so the banner returns when a newer one ships.
 *  Non-admins never see it (only an admin can update). */
export function UpdateBanner() {
  const { t } = useI18n();
  const { isAdmin } = useAuth();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    if (!isAdmin) return;
    const controller = new AbortController();
    api
      .getUpdate(controller.signal)
      .then((info) => {
        if (!info.updateAvailable || !info.latestVersion) return;
        let dismissed: string | null = null;
        try {
          dismissed = localStorage.getItem(DISMISS_KEY);
        } catch {
          dismissed = null;
        }
        if (dismissed !== info.latestVersion) setVersion(info.latestVersion);
      })
      .catch(() => {
        /* No banner if the check is unavailable. */
      });
    return () => controller.abort();
  }, [isAdmin]);

  if (!version) return null;

  function dismiss() {
    try {
      if (version) localStorage.setItem(DISMISS_KEY, version);
    } catch {
      /* A browser refusing storage simply re-shows the banner next load. */
    }
    setVersion(null);
  }

  return (
    <div className={styles.banner} role="region" aria-label={t("update.cardTitle")}>
      <span className={styles.title}>{t("update.bannerTitle", { version })}</span>
      <div className={styles.actions}>
        <Link className={styles.view} to="/system" onClick={() => setVersion(null)}>
          {t("update.bannerAction")}
        </Link>
        <button type="button" className={styles.later} onClick={dismiss}>
          {t("update.bannerDismiss")}
        </button>
      </div>
    </div>
  );
}
