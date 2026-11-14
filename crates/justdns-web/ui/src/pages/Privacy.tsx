import { useEffect, useMemo, useState } from "react";

import {
  api,
  type CompanyContact,
  type CompanyDevice,
  type CompanyDomain,
  type DeviceTrackers,
  type TrackerReport,
  type TrackerWindow,
} from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useAuth } from "../state/auth";
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

/** What the detail pane is focused on: one device, or one company. */
type Selection = { kind: "device"; id: string } | { kind: "company"; id: string };

function readWindow(): TrackerWindow {
  try {
    const saved = localStorage.getItem(WINDOW_KEY);
    if (saved === "day" || saved === "week" || saved === "month") return saved;
  } catch {
    /* storage blocked — fall through to the default */
  }
  return "day";
}

/** The share of contacts that were blocked, 0–100, rounded. */
function blockedPercent(contacts: number, blocked: number): number {
  if (contacts === 0) return 0;
  return Math.round((blocked / contacts) * 100);
}

/** A single meter row: an optional category dot, a label (with optional
 *  sub-label), a split blocked/through bar sized against the busiest row in its
 *  list, and the raw counts. Shared by every list in the page. */
function MeterRow({
  label,
  mono,
  dotCat,
  sublabel,
  contacts,
  blocked,
  max,
  title,
  t,
  nf,
}: {
  label: string;
  mono?: boolean;
  dotCat?: string;
  sublabel?: string;
  contacts: number;
  blocked: number;
  max: number;
  title?: string;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const width = max === 0 ? 0 : (contacts / max) * 100;
  const blockedWidth = contacts === 0 ? 0 : (blocked / contacts) * 100;
  return (
    <li className={styles.companyRow}>
      <span className={styles.companyName}>
        {dotCat && <span className={styles.dot} data-cat={dotCat} aria-hidden="true" />}
        <span className={mono ? styles.domainName : styles.companyOrg} title={title}>
          {label}
        </span>
        {sublabel && <span className={styles.companyCat}>{sublabel}</span>}
      </span>
      <span className={styles.bar} aria-hidden="true">
        <span className={styles.barTrack} style={{ inlineSize: `${Math.max(width, 3)}%` }}>
          <span className={styles.barBlocked} style={{ inlineSize: `${blockedWidth}%` }} />
        </span>
      </span>
      <span className={styles.companyMeta}>
        <span className={styles.companyContacts}>{nf.format(contacts)}</span>
        <span className={styles.companyShare}>
          {t("privacy.contactsOf", {
            blocked: nf.format(blocked),
            total: nf.format(contacts),
          })}
        </span>
      </span>
    </li>
  );
}

function categoryLabel(category: string, t: ReturnType<typeof useI18n>["t"]): string {
  const key = CATEGORY_LABEL[category];
  return key ? t(key) : category;
}

/** One company row inside a device view: the meter plus, for editors, a button
 *  that blocks that company for this one device. */
function DeviceCompanyRow({
  company,
  max,
  canEdit,
  busy,
  onBlock,
  t,
  nf,
}: {
  company: CompanyContact;
  max: number;
  canEdit: boolean;
  busy: boolean;
  onBlock: () => void;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const width = max === 0 ? 0 : (company.contacts / max) * 100;
  const blockedWidth = company.contacts === 0 ? 0 : (company.blocked / company.contacts) * 100;
  return (
    <li className={styles.deviceCompanyRow}>
      <span className={styles.companyName}>
        <span className={styles.dot} data-cat={company.category} aria-hidden="true" />
        <span className={styles.companyOrg}>{company.org}</span>
        <span className={styles.companyCat}>{categoryLabel(company.category, t)}</span>
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
      {canEdit && (
        <button
          type="button"
          className={styles.blockSmall}
          disabled={busy}
          aria-label={t("privacy.blockForDeviceLabel", { org: company.org })}
          onClick={onBlock}
        >
          {t("privacy.blockForDevice")}
        </button>
      )}
    </li>
  );
}

/** The focused view for one device: its companies, busiest first, with — for
 *  editors — one-click blocking of a company for just this device. */
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
  const { canEdit } = useAuth();
  const label = device.name ?? device.client;
  const max = device.companies.reduce((peak, c) => Math.max(peak, c.contacts), 0);

  // Which company block is in flight, and the result message once it returns.
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);

  const runBlock = (org: string) => {
    setBusy(org);
    setResult(null);
    api
      .privacyBlock({ org, client: device.client })
      .then(({ added }) => {
        setResult({
          text:
            added === 0
              ? t("privacy.blockNone")
              : added === 1
                ? t("privacy.blockForDeviceDoneOne")
                : t("privacy.blockForDeviceDone", { n: added }),
          error: false,
        });
      })
      .catch((err: unknown) => {
        setResult({
          text: t("privacy.blockFailed", {
            error: err instanceof Error ? err.message : String(err),
          }),
          error: true,
        });
      })
      .finally(() => setBusy(null));
  };

  return (
    <section className={styles.card}>
      <button type="button" className={styles.back} onClick={onBack}>
        <span aria-hidden="true">‹</span> {t("privacy.allDevices")}
      </button>
      <h2 className={styles.sectionTitle}>{t("privacy.deviceCompanies", { device: label })}</h2>
      {device.name && <p className={styles.deviceAddr}>{device.client}</p>}
      {canEdit && <p className={styles.captionHint}>{t("privacy.blockForDeviceHint")}</p>}
      {result && (
        <p className={styles.blockResult} data-error={result.error || undefined} role="status">
          {result.text}
        </p>
      )}
      <ul className={styles.companyList}>
        {device.companies.map((company) => (
          <DeviceCompanyRow
            key={company.org}
            company={company}
            max={max}
            canEdit={canEdit}
            busy={busy !== null}
            onBlock={() => runBlock(company.org)}
            t={t}
            nf={nf}
          />
        ))}
      </ul>
    </section>
  );
}

/** One domain row inside a company detail: the meter plus, for editors, a small
 *  button that blocks just this domain. */
function DomainRow({
  domain,
  max,
  canEdit,
  busy,
  onBlock,
  t,
  nf,
}: {
  domain: CompanyDomain;
  max: number;
  canEdit: boolean;
  busy: boolean;
  onBlock: () => void;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const width = max === 0 ? 0 : (domain.contacts / max) * 100;
  const blockedWidth = domain.contacts === 0 ? 0 : (domain.blocked / domain.contacts) * 100;
  return (
    <li className={styles.domainRow}>
      <span className={styles.domainName} title={domain.name}>
        {domain.name}
      </span>
      <span className={styles.bar} aria-hidden="true">
        <span className={styles.barTrack} style={{ inlineSize: `${Math.max(width, 3)}%` }}>
          <span className={styles.barBlocked} style={{ inlineSize: `${blockedWidth}%` }} />
        </span>
      </span>
      <span className={styles.domainCount}>{nf.format(domain.contacts)}</span>
      {canEdit && (
        <button
          type="button"
          className={styles.blockSmall}
          disabled={busy}
          aria-label={t("privacy.blockDomainLabel", { name: domain.name })}
          onClick={onBlock}
        >
          {t("privacy.block")}
        </button>
      )}
    </li>
  );
}

/** The focused view for one company: which devices reached it and which exact
 *  names under it were looked up, plus — for editors — one-click blocking of the
 *  whole company or a single domain. */
function CompanyDetail({
  company,
  onBack,
  t,
  nf,
}: {
  company: CompanyContact;
  onBack: () => void;
  t: ReturnType<typeof useI18n>["t"];
  nf: Intl.NumberFormat;
}) {
  const { canEdit } = useAuth();
  const devices: CompanyDevice[] = company.devices ?? [];
  const domains: CompanyDomain[] = company.domains ?? [];
  const deviceMax = devices.reduce((peak, d) => Math.max(peak, d.contacts), 0);
  const domainMax = domains.reduce((peak, d) => Math.max(peak, d.contacts), 0);

  // Which block call is in flight (the company, or one domain), and the result
  // message to show once it returns.
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);

  const runBlock = (key: string, target: { org: string } | { domain: string; note?: string }) => {
    setBusy(key);
    setResult(null);
    api
      .privacyBlock(target)
      .then(({ added }) => {
        setResult({
          text:
            added === 0
              ? t("privacy.blockNone")
              : added === 1
                ? t("privacy.blockDoneOne")
                : t("privacy.blockDone", { n: added }),
          error: false,
        });
      })
      .catch((err: unknown) => {
        setResult({
          text: t("privacy.blockFailed", {
            error: err instanceof Error ? err.message : String(err),
          }),
          error: true,
        });
      })
      .finally(() => setBusy(null));
  };

  return (
    <section className={styles.card}>
      <button type="button" className={styles.back} onClick={onBack}>
        <span aria-hidden="true">‹</span> {t("privacy.allCompanies")}
      </button>
      <div className={styles.companyHead}>
        <h2 className={styles.sectionTitle}>
          <span className={styles.dot} data-cat={company.category} aria-hidden="true" />{" "}
          {t("privacy.companyContacts", { org: company.org })}
        </h2>
        {canEdit && (
          <button
            type="button"
            className={styles.blockButton}
            disabled={busy !== null}
            title={t("privacy.blockHint")}
            onClick={() => runBlock("org", { org: company.org })}
          >
            {t("privacy.block")}
          </button>
        )}
      </div>
      {result && (
        <p className={styles.blockResult} data-error={result.error || undefined} role="status">
          {result.text}
        </p>
      )}

      {devices.length > 0 && (
        <div className={styles.subSection}>
          <h3 className={styles.subTitle}>{t("privacy.companyDevicesTitle")}</h3>
          <p className={styles.captionHint}>{t("privacy.companyDevicesHint")}</p>
          <ul className={styles.companyList}>
            {devices.map((device) => (
              <MeterRow
                key={device.client}
                label={device.name ?? device.client}
                title={device.name ? device.client : undefined}
                contacts={device.contacts}
                blocked={device.blocked}
                max={deviceMax}
                t={t}
                nf={nf}
              />
            ))}
          </ul>
        </div>
      )}

      {domains.length > 0 && (
        <div className={styles.subSection}>
          <h3 className={styles.subTitle}>{t("privacy.companyDomainsTitle")}</h3>
          <p className={styles.captionHint}>{t("privacy.companyDomainsHint")}</p>
          <ul className={styles.companyList}>
            {domains.map((domain) => (
              <DomainRow
                key={domain.name}
                domain={domain}
                max={domainMax}
                canEdit={canEdit}
                busy={busy !== null}
                onBlock={() =>
                  runBlock(`domain:${domain.name}`, { domain: domain.name, note: company.org })
                }
                t={t}
                nf={nf}
              />
            ))}
          </ul>
        </div>
      )}
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
  const [selected, setSelected] = useState<Selection | null>(null);

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

  // Changing the window invalidates the drilldown, since a device or company may
  // drop out of the new window.
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

  const selectedDevice =
    selected?.kind === "device"
      ? (report.devices.find((d) => d.client === selected.id) ?? null)
      : null;
  const selectedCompany =
    selected?.kind === "company"
      ? (report.companies.find((c) => c.org === selected.id) ?? null)
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

      {selectedCompany ? (
        <CompanyDetail company={selectedCompany} onBack={() => setSelected(null)} t={t} nf={nf} />
      ) : selectedDevice ? (
        <DeviceDetail device={selectedDevice} onBack={() => setSelected(null)} t={t} nf={nf} />
      ) : (
        <div className={styles.columns}>
          <section className={styles.card}>
            <h2 className={styles.sectionTitle}>{t("privacy.topCompanies")}</h2>
            <p className={styles.captionHint}>
              {t("privacy.companiesHint", { window: t(`privacy.window.${window}`) })}
            </p>
            <ul className={styles.companyList}>
              {report.companies.map((company) => {
                const width = companyMax === 0 ? 0 : (company.contacts / companyMax) * 100;
                const blockedWidth =
                  company.contacts === 0 ? 0 : (company.blocked / company.contacts) * 100;
                return (
                  <li key={company.org}>
                    <button
                      type="button"
                      className={styles.companyButton}
                      onClick={() => setSelected({ kind: "company", id: company.org })}
                    >
                      <span className={styles.companyName}>
                        <span className={styles.dot} data-cat={company.category} aria-hidden="true" />
                        <span className={styles.companyOrg}>{company.org}</span>
                        <span className={styles.companyCat}>
                          {categoryLabel(company.category, t)}
                        </span>
                      </span>
                      <span className={styles.bar} aria-hidden="true">
                        <span
                          className={styles.barTrack}
                          style={{ inlineSize: `${Math.max(width, 3)}%` }}
                        >
                          <span
                            className={styles.barBlocked}
                            style={{ inlineSize: `${blockedWidth}%` }}
                          />
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
                      <span className={styles.chevron} aria-hidden="true">
                        ›
                      </span>
                    </button>
                  </li>
                );
              })}
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
                      onClick={() => setSelected({ kind: "device", id: device.client })}
                    >
                      <span className={styles.deviceMain}>
                        <span
                          className={styles.deviceName}
                          title={device.name ? device.client : undefined}
                        >
                          {label}
                        </span>
                        {top && (
                          <span className={styles.devicePreview}>
                            <span className={styles.dot} data-cat={top.category} aria-hidden="true" />
                            <span className={styles.devicePreviewOrg}>{top.org}</span>
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
