import { useEffect, useMemo, useState } from "react";

import { api, type CompanyContact, type DeviceTrackers, type TrackerReport, type TrackerWindow } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Privacy.module.css";

/** The window choices offered in the segmented control, in order. */
const WINDOWS: TrackerWindow[] = ["day", "week", "month"];
const WINDOW_KEY = "justdns.privacyWindow";

/** The catalogue categories we colour-code, mapped to their i18n label key.
 *  Anything unexpected falls back to its raw slug. */
const CATEGORY_LABEL: Record<string, MessageKey> = {
  advertising: "privacy.cat.advertising",
  analytics: "privacy.cat.analytics",
  social: "privacy.cat.social",
  fingerprinting: "privacy.cat.fingerprinting",
};

function readWindow(): TrackerWindow {
  try {
    const saved = localStorage.getItem(WINDOW_KEY);
    if (saved === "day" || saved === "week" || saved === "month") return saved;
  } catch {
    /* storage blocked — fall through to the default */
  }
  return "day";
}

/** The share of a company's contacts that were blocked, 0–100, rounded. */
function blockedPercent(contacts: number, blocked: number): number {
  if (contacts === 0) return 0;
  return Math.round((blocked / contacts) * 100);
}

/** One company row: a category dot, the name, a split blocked/through bar sized
 *  against the busiest company in the list, and the raw counts. */
function CompanyRow({
  company,
  max,
  t,
  nf,
}: {
  company: CompanyContact;
  max: number;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const labelKey = CATEGORY_LABEL[company.category];
  const categoryLabel = labelKey ? t(labelKey) : company.category;
  const width = max === 0 ? 0 : (company.contacts / max) * 100;
  const blockedWidth = company.contacts === 0 ? 0 : (company.blocked / company.contacts) * 100;
  return (
    <li className={styles.companyRow}>
      <span className={styles.companyName}>
        <span className={styles.dot} data-cat={company.category} aria-hidden="true" />
        <span className={styles.companyOrg}>{company.org}</span>
        <span className={styles.companyCat}>{categoryLabel}</span>
      </span>
      <span className={styles.bar} aria-hidden="true">
        <span className={styles.barTrack} style={{ inlineSize: `${Math.max(width, 3)}%` }}>
          <span className={styles.barBlocked} style={{ inlineSize: `${blockedWidth}%` }} />
        </span>
      </span>
      <span className={styles.companyMeta}>
        <span className={styles.companyContacts}>{nf.format(company.contacts)}</span>
        <span className={styles.companyShare}>
          {t("privacy.contactsOf", {
            blocked: nf.format(company.blocked),
            total: nf.format(company.contacts),
          })}
        </span>
      </span>
    </li>
  );
}

/** The focused view for one device: its companies, busiest first. */
function DeviceDetail({
  device,
  onBack,
  t,
  nf,
}: {
  device: DeviceTrackers;
  onBack: () => void;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const label = device.name ?? device.client;
  const max = device.companies.reduce((peak, c) => Math.max(peak, c.contacts), 0);
  return (
    <section className={styles.card}>
      <button type="button" className={styles.back} onClick={onBack}>
        <span aria-hidden="true">‹</span> {t("privacy.allDevices")}
      </button>
      <h2 className={styles.sectionTitle}>{t("privacy.deviceCompanies", { device: label })}</h2>
      {device.name && <p className={styles.deviceAddr}>{device.client}</p>}
      <ul className={styles.companyList}>
        {device.companies.map((company) => (
          <CompanyRow key={company.org} company={company} max={max} t={t} nf={nf} />
        ))}
      </ul>
    </section>
  );
}

export function Privacy() {
  const { t, locale } = useI18n();
  const nf = useMemo(() => new Intl.NumberFormat(locale), [locale]);

  const [window, setWindow] = useState<TrackerWindow>(readWindow);
  const [report, setReport] = useState<TrackerReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    try {
      localStorage.setItem(WINDOW_KEY, window);
    } catch {
      /* a viewer with storage blocked just loses the remembered choice */
    }
  }, [window]);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    api
      .privacyTrackers(window, controller.signal)
      .then((data) => {
        setReport(data);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (controller.signal.aborted) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => controller.abort();
  }, [window]);

  // Changing the window invalidates the drilldown, since a device may drop out.
  useEffect(() => setSelected(null), [window]);

  const windowSwitch = (
    <div className={styles.switch} role="group" aria-label={t("privacy.windowLabel")}>
      {WINDOWS.map((w) => (
        <button
          key={w}
          type="button"
          className={styles.switchButton}
          data-active={w === window || undefined}
          aria-pressed={w === window}
          onClick={() => setWindow(w)}
        >
          {t(`privacy.window.${w}`)}
        </button>
      ))}
    </div>
  );

  const header = (
    <header className={styles.head}>
      <div>
        <h1 className={styles.title}>{t("privacy.title")}</h1>
        <p className={styles.intro}>{t("privacy.intro")}</p>
      </div>
      {windowSwitch}
    </header>
  );

  if (!report || report.totalContacts === 0) {
    return (
      <div className={styles.page}>
        {header}
        <p className={styles.caption}>
          {error
            ? t("privacy.unreachable", { error })
            : loading
              ? t("privacy.loading")
              : t("privacy.empty")}
        </p>
        {!loading && !error && report && (
          <p className={styles.captionHint}>{t("privacy.emptyHint")}</p>
        )}
      </div>
    );
  }

  const blockedShare = blockedPercent(report.totalContacts, report.totalBlocked);
  const companyMax = report.companies.reduce((peak, c) => Math.max(peak, c.contacts), 0);
  const selectedDevice = selected
    ? report.devices.find((d) => d.client === selected) ?? null
    : null;

  // The one-line "story" for non-technical readers: the busiest device and the
  // company it reached most. Devices and companies are already sorted busiest
  // first, so the first of each is the headline.
  const topDevice = report.devices[0];
  const storyCompany = topDevice?.companies[0];
  const story =
    topDevice && storyCompany
      ? t("privacy.story", {
          device: topDevice.name ?? topDevice.client,
          org: storyCompany.org,
          count: nf.format(storyCompany.contacts),
        })
      : null;

  return (
    <div className={styles.page}>
      {header}

      <div className={styles.tiles}>
        <section className={`${styles.tile} ${styles.tileHero}`}>
          <span className={styles.tileLabel}>{t("privacy.totalContacts")}</span>
          <strong className={styles.tileFigure}>{nf.format(report.totalContacts)}</strong>
          <span className={styles.tileHint}>{t("privacy.totalContactsHint")}</span>
        </section>
        <section className={styles.tile}>
          <span className={styles.tileLabel}>{t("privacy.blocked")}</span>
          <strong className={styles.tileFigure} data-tone="blocked">
            {nf.format(report.totalBlocked)}
          </strong>
          <span className={styles.tileHint}>{t("privacy.blockedShare", { n: blockedShare })}</span>
        </section>
        <section className={styles.tile}>
          <span className={styles.tileLabel}>{t("privacy.allowed")}</span>
          <strong className={styles.tileFigure} data-tone="allowed">
            {nf.format(report.totalAllowed)}
          </strong>
          <span className={styles.tileHint}>{t("privacy.allowedHint")}</span>
        </section>
      </div>

      {story && <p className={styles.story}>{story}</p>}

      {selectedDevice ? (
        <DeviceDetail
          device={selectedDevice}
          onBack={() => setSelected(null)}
          t={t}
          nf={nf}
        />
      ) : (
        <div className={styles.columns}>
          <section className={styles.card}>
            <h2 className={styles.sectionTitle}>{t("privacy.topCompanies")}</h2>
            <p className={styles.captionHint}>
              {t("privacy.companiesHint", { window: t(`privacy.window.${window}`) })}
            </p>
            <ul className={styles.companyList}>
              {report.companies.map((company) => (
                <CompanyRow key={company.org} company={company} max={companyMax} t={t} nf={nf} />
              ))}
            </ul>
          </section>

          <section className={styles.card}>
            <h2 className={styles.sectionTitle}>{t("privacy.devices")}</h2>
            <p className={styles.captionHint}>{t("privacy.devicesHint")}</p>
            <ul className={styles.deviceList}>
              {report.devices.map((device) => {
                const label = device.name ?? device.client;
                const share = blockedPercent(device.contacts, device.blocked);
                const top = device.companies[0];
                return (
                  <li key={device.client}>
                    <button
                      type="button"
                      className={styles.deviceRow}
                      onClick={() => setSelected(device.client)}
                    >
                      <span className={styles.deviceMain}>
                        <span className={styles.deviceName} title={device.name ? device.client : undefined}>
                          {label}
                        </span>
                        {top && (
                          <span className={styles.devicePreview}>
                            <span className={styles.dot} data-cat={top.category} aria-hidden="true" />
                            {top.org}
                          </span>
                        )}
                      </span>
                      <span className={styles.deviceMeta}>
                        <span className={styles.deviceContacts}>
                          {nf.format(device.contacts)} {t("privacy.contacts")}
                        </span>
                        <span className={styles.deviceShare}>
                          {t("privacy.blockedShare", { n: share })}
                        </span>
                      </span>
                      <span className={styles.chevron} aria-hidden="true">
                        ›
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>
        </div>
      )}

      <p className={styles.note}>{t("privacy.note")}</p>
    </div>
  );
}
