// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useState } from "react";
import { Link } from "react-router-dom";

import { useI18n } from "../lib/i18n";
import { FEATURES, type FeatureEntry } from "../lib/features";
import type { MessageKey } from "../lib/messages";
import { useAuth } from "../state/auth";
import styles from "./WhatsNew.module.css";

/** Which version's card the user has dismissed. The UpdateBanner pattern: one
 *  localStorage slot, so the card returns with the next release and never for
 *  the one it was dismissed for. */
const DISMISSED_KEY = "justdns.whatsNewDismissed";

/** The running release, as baked into the bundle. Local builds say "dev" and
 *  show no card — "new in this version" only means something for a real
 *  release. */
const APP_VERSION: string = __APP_VERSION__;
const RELEASE = /^v?\d+\.\d+\.\d+$/;

function bareVersion(): string | null {
  return RELEASE.test(APP_VERSION) ? APP_VERSION.replace(/^v/, "") : null;
}

/** One summary line per release, for what the release brought beyond its
 *  catalogued features (a fix, a hardening). A version without an entry here
 *  simply has its card speak only through its feature list. */
const SUMMARIES: Record<string, MessageKey> = {
  "0.49.1": "whatsnew.summary.0.49.1",
};

/** Whether the card should appear for this release: a real release version,
 *  a different one than the last dismissal (the "only after an actual update"
 *  rule — a first install deliberately sees nothing, since everything is new
 *  there and the guide covers discovery), and something to say. */
function dueHere(version: string): boolean {
  let dismissed: string | null = null;
  try {
    dismissed = localStorage.getItem(DISMISSED_KEY);
  } catch {
    return false;
  }
  if (dismissed === null) return false;
  if (dismissed === version) return false;
  return FEATURES.some((entry) => entry.since === version) || version in SUMMARIES;
}

/** The post-update note on the overview: "New in {version}" with the features
 *  this release added, deep links into their pages, and a dismissal that
 *  remembers the version. Shown to every role, listing only what that role can
 *  reach. */
export function WhatsNew() {
  const { t } = useI18n();
  const { role } = useAuth();
  const [dismissedHere, setDismissedHere] = useState(false);

  const version = bareVersion();
  if (version === null || dismissedHere || !dueHere(version)) return null;

  const fresh: FeatureEntry[] = FEATURES.filter(
    (entry) => entry.since === version && role !== null && entry.roles.includes(role),
  );
  const summary = SUMMARIES[version];
  if (fresh.length === 0 && summary === undefined) return null;

  function dismiss() {
    try {
      localStorage.setItem(DISMISSED_KEY, version ?? "");
    } catch {
      /* Storage refused: the card simply returns next load. */
    }
    setDismissedHere(true);
  }

  return (
    <section className={styles.card} aria-label={t("whatsnew.title", { version })}>
      <div className={styles.head}>
        <h2 className={styles.title}>{t("whatsnew.title", { version })}</h2>
        <button type="button" className={styles.dismiss} onClick={dismiss}>
          {t("whatsnew.dismiss")}
        </button>
      </div>
      {summary !== undefined && <p className={styles.summary}>{t(summary)}</p>}
      {fresh.length > 0 && (
        <ul className={styles.list}>
          {fresh.map((entry) => (
            <li key={entry.id} className={styles.item}>
              <Link to={entry.route} className={styles.link}>
                {t(entry.titleKey)}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
