import { useMemo } from "react";

import { ActivityChart } from "../components/ActivityChart";
import { useI18n } from "../lib/i18n";
import { useOverview } from "../lib/useOverview";
import styles from "./Overview.module.css";

function timeAgo(seconds: number, t: ReturnType<typeof useI18n>["t"]): string {
  if (seconds < 5) return t("time.justNow");
  if (seconds < 60) return t("time.secondsAgo", { n: seconds });
  const minutes = Math.round(seconds / 60);
  if (minutes === 1) return t("time.oneMinuteAgo");
  if (minutes < 60) return t("time.minutesAgo", { n: minutes });
  const hours = Math.round(minutes / 60);
  return hours === 1 ? t("time.oneHourAgo") : t("time.hoursAgo", { n: hours });
}

export function Overview() {
  const { data, error, loading } = useOverview();
  const { t, locale } = useI18n();
  const nf = useMemo(() => new Intl.NumberFormat(locale), [locale]);

  if (!data) {
    return (
      <div className={styles.page}>
        <p className={styles.caption}>
          {error
            ? t("overview.unreachable", { error })
            : loading
              ? t("overview.loading")
              : t("common.noData")}
        </p>
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <section className={styles.hero}>
        <p className={styles.status}>
          <span className={styles.dot} aria-hidden="true" />
          {t("overview.protectionActive")}
        </p>
        <h1 className={styles.figure}>{nf.format(data.total)}</h1>
        <p className={styles.caption}>{t("overview.resolved")}</p>
      </section>

      <ActivityChart values={data.activity} />

      <dl className={styles.stats}>
        <div className={styles.stat}>
          <dt>{t("overview.fromCache")}</dt>
          <dd>{data.cacheRate}&thinsp;%</dd>
        </div>
        <div className={styles.stat}>
          <dt>{t("overview.blocked")}</dt>
          <dd>{nf.format(data.blocked)}</dd>
        </div>
        <div className={styles.stat}>
          <dt>{t("overview.activeDevices")}</dt>
          <dd>{data.activeClients}</dd>
        </div>
      </dl>

      <section className={styles.recent}>
        <h2 className={styles.recentTitle}>{t("overview.recentTitle")}</h2>
        {data.recent.length === 0 ? (
          <p className={styles.caption}>{t("overview.noneRecorded")}</p>
        ) : (
          <ul className={styles.list}>
            {data.recent.map((entry, index) => (
              <li key={`${entry.name}-${index}`} className={styles.row}>
                <span className={styles.domain}>{entry.name}</span>
                <span className={styles.client}>{entry.client}</span>
                <span className={styles.tag}>{entry.type}&thinsp;·&thinsp;{entry.source}</span>
                <span className={styles.ago}>{timeAgo(entry.secondsAgo, t)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
