import { useEffect, useMemo, useState } from "react";

import { ActivityChart } from "../components/ActivityChart";
import { useI18n } from "../lib/i18n";
import { useOverview } from "../lib/useOverview";
import styles from "./Overview.module.css";

/** Which unit the live rate is shown in. Remembered per viewer. */
type RateUnit = "sec" | "min";
const RATE_UNIT_KEY = "justdns.rateUnit";

function readRateUnit(): RateUnit {
  try {
    return localStorage.getItem(RATE_UNIT_KEY) === "min" ? "min" : "sec";
  } catch {
    return "sec";
  }
}

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
  const nf1 = useMemo(
    () => new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }),
    [locale],
  );

  const [rateUnit, setRateUnit] = useState<RateUnit>(readRateUnit);
  useEffect(() => {
    try {
      localStorage.setItem(RATE_UNIT_KEY, rateUnit);
    } catch {
      // A viewer with storage blocked just loses the remembered choice.
    }
  }, [rateUnit]);

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

  const rate = data.liveRatePerSec;
  const rateDisplay =
    rate == null
      ? "—"
      : rateUnit === "min"
        ? nf.format(Math.round(rate * 60))
        : nf1.format(rate);
  const rateUnitLabel = t(rateUnit === "min" ? "overview.perMinuteUnit" : "overview.perSecondUnit");

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

      <section className={styles.liveRate}>
        <div className={styles.liveRateHead}>
          <span className={styles.rateLabel}>{t("overview.liveRate")}</span>
          <div className={styles.rateSwitch} role="group" aria-label={t("overview.liveRate")}>
            {(["sec", "min"] as const).map((unit) => (
              <button
                key={unit}
                type="button"
                className={styles.rateButton}
                data-active={unit === rateUnit || undefined}
                aria-pressed={unit === rateUnit}
                onClick={() => setRateUnit(unit)}
              >
                {t(unit === "min" ? "overview.perMinute" : "overview.perSecond")}
              </button>
            ))}
          </div>
        </div>
        <p className={styles.rateValue}>
          {rateDisplay}
          <span className={styles.rateUnit}>{rateUnitLabel}</span>
        </p>
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

      <section className={styles.day}>
        <h2 className={styles.sectionTitle}>{t("overview.last24h")}</h2>
        <dl className={styles.stats}>
          <div className={styles.stat}>
            <dt>{t("overview.last24hQueries")}</dt>
            <dd>{nf.format(data.day.total)}</dd>
          </div>
          <div className={styles.stat}>
            <dt>{t("overview.blocked")}</dt>
            <dd>{nf.format(data.day.blocked)}</dd>
          </div>
          <div className={styles.stat}>
            <dt>{t("overview.medianResponse")}</dt>
            {data.day.total === 0 ? (
              <dd>—</dd>
            ) : (
              <dd>
                {nf1.format(data.day.medianDurationMs)}&thinsp;
                <span className={styles.unit}>ms</span>
              </dd>
            )}
          </div>
        </dl>
      </section>

      <section className={styles.clients}>
        <h2 className={styles.sectionTitle}>{t("overview.topClients")}</h2>
        {data.topClients.length === 0 ? (
          <p className={styles.caption}>{t("overview.noClients")}</p>
        ) : (
          <ul className={styles.clientList}>
            {data.topClients.map((entry) => {
              const share = data.day.total === 0 ? 0 : entry.count / data.day.total;
              return (
                <li key={entry.client} className={styles.clientRow}>
                  <span
                    className={styles.clientIp}
                    title={entry.name ? entry.client : undefined}
                  >
                    {entry.name ?? entry.client}
                  </span>
                  <span className={styles.clientBar} aria-hidden="true">
                    <span
                      className={styles.clientBarFill}
                      style={{ inlineSize: `${Math.max(share * 100, 2)}%` }}
                    />
                  </span>
                  <span className={styles.clientMeta}>
                    {nf.format(entry.count)}
                    <span className={styles.clientShare}>{Math.round(share * 100)}&thinsp;%</span>
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section className={styles.resolvers}>
        <h2 className={styles.sectionTitle}>{t("overview.resolvers")}</h2>
        {data.resolvers.length === 0 ? (
          <p className={styles.caption}>{t("overview.noResolvers")}</p>
        ) : (
          <ul className={styles.resolverList}>
            {data.resolvers.map((entry) => (
              <li
                key={entry.resolver}
                className={styles.resolverRow}
                title={t("overview.resolverP95Title", {
                  n: nf1.format(entry.p95_duration_ms),
                })}
              >
                <span className={styles.resolverName}>{entry.resolver}</span>
                <span className={styles.resolverTime}>
                  {nf1.format(entry.median_duration_ms)}&thinsp;
                  <span className={styles.unit}>ms</span>
                </span>
                <span className={styles.resolverCount}>
                  {t("overview.resolverAnswers", { n: nf.format(entry.count) })}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className={styles.recent}>
        <h2 className={styles.recentTitle}>{t("overview.recentTitle")}</h2>
        {data.recent.length === 0 ? (
          <p className={styles.caption}>{t("overview.noneRecorded")}</p>
        ) : (
          <ul className={styles.list}>
            {data.recent.map((entry, index) => (
              <li key={`${entry.name}-${index}`} className={styles.row}>
                <span className={styles.domain}>{entry.name}</span>
                <span className={styles.client} title={entry.clientTitle}>
                  {entry.client}
                </span>
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
