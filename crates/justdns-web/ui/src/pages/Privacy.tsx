import { useEffect, useMemo, useState } from "react";

import {
  api,
  ApiError,
  type CompanyContact,
  type CompanyDevice,
  type CompanyDomain,
  type DeviceTrackers,
  type GeoWatch,
  type NetworkReport,
  type ServerSettings,
  type TrackerReport,
  type TrackerWindow,
} from "../lib/api";
import { Toggle } from "../components/Toggle";
import { WorldMap } from "../components/WorldMap";
import { countryName, flagEmoji } from "../lib/geo";
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

/** "Where your devices phone home": the destinations the network's answers
 *  resolve into over the window, grouped by country and ASN, from the offline
 *  GeoIP database. Hidden entirely when no database is loaded (the endpoint 404s)
 *  or nothing resolved, so an install without the feature sees no empty section. */
function NetworksSection({
  window,
  t,
  locale,
  nf,
  canWrite,
}: {
  window: TrackerWindow;
  t: ReturnType<typeof useI18n>["t"];
  locale: string;
  nf: Intl.NumberFormat;
  canWrite: boolean;
}) {
  const [report, setReport] = useState<NetworkReport | null>(null);
  const [available, setAvailable] = useState(true);
  const [country, setCountry] = useState<string | null>(null);
  // The full settings object, held so a watch toggle can PUT it back with only
  // `geoWatch` changed. Only loaded/used by an admin on the writable node.
  const [settings, setSettings] = useState<ServerSettings | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    setReport(null);
    setCountry(null);
    api
      .privacyNetworks(window, controller.signal)
      .then((data) => {
        setReport(data);
        setAvailable(true);
      })
      .catch((error) => {
        if (controller.signal.aborted) return;
        // Only a 404 means the feature is genuinely off (no GeoIP database):
        // hide the section for good. Any other error (a transient scan failure,
        // a dropped connection) must not permanently hide a working feature, so
        // leave it available — the next window change or remount retries.
        if (error instanceof ApiError && error.status === 404) {
          setAvailable(false);
        }
      });
    return () => controller.abort();
  }, [window]);

  // Load settings once, for the watch toggles. Only an admin on the writable node
  // can change them, so skip the fetch otherwise.
  useEffect(() => {
    if (!canWrite) return;
    const controller = new AbortController();
    api
      .settings(controller.signal)
      .then(setSettings)
      .catch(() => {
        /* a transient failure just leaves the toggles unavailable this load */
      });
    return () => controller.abort();
  }, [canWrite]);

  // Aggregate the per-ASN networks by country for the choropleth, memoised on the
  // report so an unrelated re-render (selecting a country, a watch toggle, the
  // settings fetch resolving) does not hand WorldMap a fresh array and force a full
  // SVG re-paint. A network with no country (private/unknown address) never lands
  // on the map. Computed before the early return below to keep hook order stable.
  const { mapCountries, mapMax } = useMemo(() => {
    const totals = new Map<string, number>();
    for (const net of report?.networks ?? []) {
      if (!net.country) continue;
      const code = net.country.toLowerCase();
      totals.set(code, (totals.get(code) ?? 0) + net.contacts);
    }
    const countries = Array.from(totals, ([code, contacts]) => ({ code, contacts }));
    return { mapCountries: countries, mapMax: countries.reduce((peak, c) => Math.max(peak, c.contacts), 0) };
  }, [report]);

  if (!available || !report || report.totalContacts === 0 || report.networks.length === 0) {
    return null;
  }
  const max = report.networks.reduce((peak, n) => Math.max(peak, n.contacts), 0);

  // Selecting a country on the map filters the network list to it.
  const shownNetworks = country
    ? report.networks.filter((net) => net.country?.toLowerCase() === country)
    : report.networks;
  const selectedLabel = country
    ? `${flagEmoji(country)} ${countryName(country, locale) || country.toUpperCase()}`.trim()
    : "";

  // The current global watch set, for the toggles. Writing is gated on `canWrite`
  // and a loaded settings object; otherwise the map and list are read-only.
  const geoWatch: GeoWatch = settings?.geoWatch ?? { countries: [], asns: [] };
  const watchedCountries = new Set(geoWatch.countries.map((c) => c.toLowerCase()));
  const watchedAsns = new Set(geoWatch.asns);
  const showWatch = canWrite && settings !== null;

  // Persist a changed watch set by PUTting the whole settings back with only
  // `geoWatch` replaced. Optimistic: on failure the previous settings are kept.
  const persistWatch = (next: GeoWatch) => {
    if (!settings || saving) return;
    const previous = settings;
    const updated = { ...settings, geoWatch: next };
    setSettings(updated);
    setSaving(true);
    api
      .saveSettings(updated)
      .then((saved) => setSettings(saved))
      .catch(() => setSettings(previous))
      .finally(() => setSaving(false));
  };
  const toggleCountry = (code: string) => {
    const lower = code.toLowerCase();
    const countries = watchedCountries.has(lower)
      ? geoWatch.countries.filter((c) => c.toLowerCase() !== lower)
      : [...geoWatch.countries, lower];
    persistWatch({ ...geoWatch, countries });
  };
  const toggleAsn = (asn: number) => {
    const asns = watchedAsns.has(asn)
      ? geoWatch.asns.filter((a) => a !== asn)
      : [...geoWatch.asns, asn];
    persistWatch({ ...geoWatch, asns });
  };

  return (
    <section className={styles.card}>
      <h2 className={styles.sectionTitle}>{t("privacy.networksTitle")}</h2>
      <p className={styles.captionHint}>
        {t("privacy.networksHint", { window: t(`privacy.window.${window}`) })}
      </p>
      {mapCountries.length > 0 && (
        <WorldMap
          countries={mapCountries}
          max={mapMax}
          selected={country}
          onSelect={setCountry}
          locale={locale}
          t={t}
          nf={nf}
        />
      )}
      {country && (
        <div className={styles.mapFilter}>
          <span className={styles.mapFilterName}>{selectedLabel}</span>
          {showWatch && (
            <span className={styles.watchControl}>
              <Toggle
                checked={watchedCountries.has(country)}
                onChange={() => toggleCountry(country)}
                label={t("privacy.watchCountryLabel", { country: selectedLabel })}
                disabled={saving}
              />
              <span className={styles.watchText}>{t("privacy.watchCountry")}</span>
            </span>
          )}
          <button type="button" className={styles.mapFilterClear} onClick={() => setCountry(null)}>
            {t("privacy.mapClear")}
          </button>
        </div>
      )}
      <ul className={styles.companyList}>
        {shownNetworks.map((net, i) => {
          const width = max === 0 ? 0 : (net.contacts / max) * 100;
          const flag = flagEmoji(net.country);
          const land = countryName(net.country, locale);
          const primary = net.org ?? land ?? t("privacy.networkUnknown");
          return (
            <li key={i} className={styles.companyRow}>
              <span className={styles.companyName}>
                <span className={styles.companyOrg}>
                  {flag ? `${flag} ` : ""}
                  {primary || t("privacy.networkUnknown")}
                </span>
                {net.asn != null && <span className={styles.companyCat}>AS{net.asn}</span>}
                {net.org && land && <span className={styles.companyCat}>{land}</span>}
              </span>
              <span className={styles.bar} aria-hidden="true">
                <span className={styles.barTrack} style={{ inlineSize: `${Math.max(width, 3)}%` }} />
              </span>
              <span className={styles.companyMeta}>
                {showWatch && country && net.asn != null && (
                  <span className={styles.watchControl}>
                    <Toggle
                      checked={watchedAsns.has(net.asn)}
                      onChange={() => toggleAsn(net.asn!)}
                      label={t("privacy.watchAsnLabel", { asn: `AS${net.asn}` })}
                      disabled={saving}
                    />
                    <span className={styles.watchText}>{t("privacy.watchAsn")}</span>
                  </span>
                )}
                <span className={styles.companyContacts}>{nf.format(net.contacts)}</span>
              </span>
            </li>
          );
        })}
      </ul>
      {mapCountries.length > 0 && <p className={styles.mapCredit}>{t("privacy.mapCredit")}</p>}
    </section>
  );
}

/** How many rows the companies and devices lists show before "show more". Kept
 *  short so the world map below them stays within quick reach. */
const COLLAPSED_ROWS = 5;

/** The "show more / show less" toggle under a capped list. Renders nothing when
 *  the list already fits within {@link COLLAPSED_ROWS}; otherwise a button that
 *  reports its expanded state to assistive tech and controls the given list. */
function ShowMoreToggle({
  total,
  expanded,
  onToggle,
  controls,
  t,
}: {
  total: number;
  expanded: boolean;
  onToggle: () => void;
  controls: string;
  t: ReturnType<typeof useI18n>["t"];
}) {
  if (total <= COLLAPSED_ROWS) return null;
  return (
    <button
      type="button"
      className={styles.showMore}
      aria-expanded={expanded}
      aria-controls={controls}
      onClick={onToggle}
    >
      {expanded ? t("privacy.showLess") : t("privacy.showMore", { n: total - COLLAPSED_ROWS })}
    </button>
  );
}

export function Privacy() {
  const { t, locale } = useI18n();
  const { isAdmin, canEdit } = useAuth();
  // Watching is a global setting change: admin only, and (on an HA cluster) only
  // on the primary — the same gate `canEdit` applies to the writable node.
  const canWriteGeo = isAdmin && canEdit;
  const nf = useMemo(() => new Intl.NumberFormat(locale), [locale]);

  const [window, setWindow] = useState<TrackerWindow>(readWindow);
  const [report, setReport] = useState<TrackerReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<Selection | null>(null);
  // The companies and devices lists start collapsed to a handful of rows so the
  // world map below them stays within quick reach; "show more" expands each.
  const [companiesShown, setCompaniesShown] = useState(false);
  const [devicesShown, setDevicesShown] = useState(false);

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
  // Keep the top lists short by default so the world map below is quickly reached.
  const shownCompanies = companiesShown
    ? report.companies
    : report.companies.slice(0, COLLAPSED_ROWS);
  const shownDevices = devicesShown ? report.devices : report.devices.slice(0, COLLAPSED_ROWS);

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
        <>
        <div className={styles.columns}>
          <section className={styles.card}>
            <h2 className={styles.sectionTitle}>{t("privacy.topCompanies")}</h2>
            <p className={styles.captionHint}>
              {t("privacy.companiesHint", { window: t(`privacy.window.${window}`) })}
            </p>
            <ul id="privacy-companies" className={styles.companyList}>
              {shownCompanies.map((company) => {
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
            <ShowMoreToggle
              total={report.companies.length}
              expanded={companiesShown}
              onToggle={() => setCompaniesShown((shown) => !shown)}
              controls="privacy-companies"
              t={t}
            />
          </section>

          <section className={styles.card}>
            <h2 className={styles.sectionTitle}>{t("privacy.devices")}</h2>
            <p className={styles.captionHint}>{t("privacy.devicesHint")}</p>
            <ul id="privacy-devices" className={styles.deviceList}>
              {shownDevices.map((device) => {
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
            <ShowMoreToggle
              total={report.devices.length}
              expanded={devicesShown}
              onToggle={() => setDevicesShown((shown) => !shown)}
              controls="privacy-devices"
              t={t}
            />
          </section>
        </div>
        <NetworksSection window={window} t={t} locale={locale} nf={nf} canWrite={canWriteGeo} />
        </>
      )}

      <p className={styles.note}>{t("privacy.note")}</p>
    </div>
  );
}
