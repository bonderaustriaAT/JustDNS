import { useEffect, useState } from "react";

import { api, type DnssecResponse, type DnssecStatus } from "../lib/api";
import { displayName } from "../lib/dns";
import { useI18n } from "../lib/i18n";
import styles from "./Dnssec.module.css";

/** How many validated queries the table shows at once. */
const RECENT_LIMIT = 100;
/** How often to pull fresh verdicts. */
const REFRESH_MS = 2_000;

/** The four verdicts, in the order the counter cards are laid out. */
const STATES: DnssecStatus[] = ["secure", "insecure", "bogus", "indeterminate"];

/** Load the DNSSEC verdict counts and the most recent validated queries,
 *  filtered to `filter` when one is set, refreshing every couple of seconds.
 *  Rows already on screen stay put during a refetch so the table never flashes
 *  empty between polls. */
function useDnssec(filter: DnssecStatus | null) {
  const { t } = useI18n();
  const [data, setData] = useState<DnssecResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const controller = new AbortController();

    async function load() {
      try {
        const next = await api.dnssec(filter ?? undefined, RECENT_LIMIT, controller.signal);
        if (!active) return;
        setData(next);
        setError(null);
        setLoading(false);
      } catch (err) {
        if (!active || controller.signal.aborted) return;
        setError(err instanceof Error ? err.message : t("common.unknownError"));
        setLoading(false);
      }
    }

    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => {
      active = false;
      controller.abort();
      clearInterval(timer);
    };
  }, [filter, t]);

  return { data, error, loading };
}

export function Dnssec() {
  const { t, locale } = useI18n();
  const [filter, setFilter] = useState<DnssecStatus | null>(null);
  const { data, error, loading } = useDnssec(filter);

  const timeFmt = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });

  const counts = data
    ? {
        secure: data.secure,
        insecure: data.insecure,
        bogus: data.bogus,
        indeterminate: data.indeterminate,
      }
    : null;
  const total = counts
    ? counts.secure + counts.insecure + counts.bogus + counts.indeterminate
    : 0;
  const rows = data?.recent ?? null;

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <h1 className={styles.title}>{t("dnssec.title")}</h1>
        <p className={styles.intro}>{t("dnssec.intro")}</p>
      </header>

      {counts && counts.bogus > 0 && (
        <div className={styles.alert} role="alert">
          <svg
            className={styles.alertIcon}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="M12 3l9 16H3l9-16z" />
            <path d="M12 10v4M12 17.5v.01" />
          </svg>
          <div className={styles.alertText}>
            <strong className={styles.alertTitle}>
              {counts.bogus === 1
                ? t("dnssec.bogusBannerOne")
                : t("dnssec.bogusBanner", { n: counts.bogus })}
            </strong>
            <span className={styles.alertHint}>{t("dnssec.bogusBannerHint")}</span>
          </div>
        </div>
      )}

      <div className={styles.cards} role="group" aria-label={t("dnssec.filterHint")}>
        {STATES.map((state) => {
          const active = filter === state;
          return (
            <button
              key={state}
              type="button"
              className={styles.card}
              data-state={state}
              data-active={active || undefined}
              aria-pressed={active}
              title={t(`dnssec.${state}Desc`)}
              onClick={() => setFilter(active ? null : state)}
            >
              <span className={styles.cardCount}>{counts ? counts[state] : "—"}</span>
              <span className={styles.cardLabel}>{t(`dnssec.${state}`)}</span>
            </button>
          );
        })}
      </div>

      <p className={styles.hint}>
        {filter ? (
          <>
            <span className={styles.filtered}>
              {t("dnssec.filteredTo", { status: t(`dnssec.${filter}`) })}
            </span>
            <button type="button" className={styles.clear} onClick={() => setFilter(null)}>
              {t("dnssec.clearFilter")}
            </button>
          </>
        ) : (
          t("dnssec.filterHint")
        )}
      </p>

      {rows === null ? (
        <p className={styles.note}>
          {error ? t("dnssec.unreachable", { error }) : loading ? t("dnssec.loading") : t("common.noData")}
        </p>
      ) : rows.length === 0 ? (
        <p className={styles.note}>
          {filter
            ? t("dnssec.noneForState")
            : total === 0
              ? t("dnssec.disabledHint")
              : t("dnssec.noneRecorded")}
        </p>
      ) : (
        <ol className={styles.list}>
          <li className={styles.columns} aria-hidden="true">
            <span>{t("logs.colTime")}</span>
            <span>{t("logs.colClient")}</span>
            <span>{t("logs.colDomain")}</span>
            <span className={styles.statusHead}>{t("dnssec.colStatus")}</span>
          </li>
          {rows.map((row, index) => (
            <li
              key={`${row.at_ms}-${index}`}
              className={styles.item}
              data-bogus={row.dnssec === "bogus" || undefined}
            >
              <div className={styles.row}>
                <span className={styles.time}>{timeFmt.format(row.at_ms)}</span>
                <span className={styles.client} title={row.device_name ? row.client : undefined}>
                  {row.device_name ?? row.client}
                </span>
                <span className={styles.domain} title={row.name}>
                  {displayName(row.name)}
                </span>
                <span className={styles.statusCell}>
                  {row.dnssec && (
                    <span className={styles.badge} data-state={row.dnssec}>
                      {t(`dnssec.${row.dnssec}`)}
                    </span>
                  )}
                </span>
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
