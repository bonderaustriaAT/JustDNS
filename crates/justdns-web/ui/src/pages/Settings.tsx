import { useMemo, useState, type ReactNode } from "react";

import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import { api } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useSettings, type SpeedProfile, type UpstreamMode } from "../state/settings";
import styles from "./Settings.module.css";

/** The "clear DNS cache" action: a one-shot button that drops every cached
 *  answer, with a short confirmation once it is done. */
function ClearCacheButton() {
  const { t } = useI18n();
  const [state, setState] = useState<"idle" | "busy" | "done" | "error">("idle");

  async function clear() {
    setState("busy");
    try {
      await api.clearCache();
      setState("done");
      window.setTimeout(() => setState("idle"), 2500);
    } catch {
      setState("error");
    }
  }

  return (
    <div className={styles.action}>
      <button
        type="button"
        className={styles.actionButton}
        onClick={clear}
        disabled={state === "busy"}
      >
        {state === "busy" ? t("settings.clearCacheBusy") : t("settings.clearCacheButton")}
      </button>
      {state === "done" ? (
        <span className={styles.actionDone}>{t("settings.clearCacheDone")}</span>
      ) : null}
      {state === "error" ? (
        <span className={styles.actionError}>{t("settings.clearCacheError")}</span>
      ) : null}
    </div>
  );
}

interface CategoryMeta {
  id: string;
  titleKey: MessageKey;
}

function Category({
  id,
  title,
  description,
  children,
}: {
  id: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className={styles.category} aria-labelledby={`${id}-title`}>
      <div className={styles.categoryHead}>
        <h2 id={`${id}-title`} className={styles.categoryTitle}>
          {title}
        </h2>
        <p className={styles.categoryDescription}>{description}</p>
      </div>
      <div className={styles.rows}>{children}</div>
    </section>
  );
}

function Row({
  label,
  description,
  details,
  control,
  disabled = false,
}: {
  label: string;
  description: string;
  details: string;
  control: ReactNode;
  disabled?: boolean;
}) {
  return (
    <div className={styles.row} data-disabled={disabled || undefined}>
      <div className={styles.text}>
        <span className={styles.labelLine}>
          <span className={styles.label}>{label}</span>
          <InfoHint label={label} text={details} />
        </span>
        <span className={styles.description}>{description}</span>
      </div>
      <div className={styles.control}>{control}</div>
    </div>
  );
}

/** The upstream selection modes, in the order the dropdown lists them. Each maps
 *  to a `settings.mode.<mode>` label key. */
const UPSTREAM_MODES: UpstreamMode[] = ["racing", "fallback", "balanced", "rules"];

/** The speed profiles, in the order the switch lists them: most private first,
 *  fastest last. Each maps to `settings.profile.<profile>` label and
 *  `settings.profile.<profile>Desc` description keys. */
const PROFILES: SpeedProfile[] = ["privacy", "balanced", "speed"];

/** The resolvers each profile uses. Mirrors `profile_resolvers` in the Rust
 *  binary, so the list shown here matches what actually answers queries. */
const PROFILE_RESOLVERS: Record<SpeedProfile, { ip: string; host: string }[]> = {
  privacy: [
    { ip: "86.54.11.1", host: "protective.joindns4.eu" },
    { ip: "9.9.9.9", host: "dns.quad9.net" },
  ],
  balanced: [
    { ip: "86.54.11.1", host: "protective.joindns4.eu" },
    { ip: "1.1.1.1", host: "cloudflare-dns.com" },
  ],
  speed: [
    { ip: "1.1.1.1", host: "cloudflare-dns.com" },
    { ip: "8.8.8.8", host: "dns.google" },
  ],
};

export function Settings() {
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const [selected, setSelected] = useState("schutz");

  const categories = useMemo<CategoryMeta[]>(() => {
    // "Upstream" now hosts the speed profile — the headline user-facing choice —
    // so it stays visible even outside expert mode; the rest stay technical.
    const base: CategoryMeta[] = [
      { id: "schutz", titleKey: "settings.cat.schutz" },
      { id: "upstream", titleKey: "settings.cat.upstream" },
    ];
    if (settings.expertMode) {
      base.push(
        { id: "verschluesselung", titleKey: "settings.cat.verschluesselung" },
        { id: "netzwerk", titleKey: "settings.cat.netzwerk" },
        { id: "cache", titleKey: "settings.cat.cache" },
        { id: "protokoll", titleKey: "settings.cat.protokoll" },
      );
    }
    return base;
  }, [settings.expertMode]);

  // Fall back to the first category when the selected one is hidden (e.g. the
  // expert mode that revealed it was switched off).
  const active = categories.some((c) => c.id === selected) ? selected : categories[0].id;

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>{t("settings.title")}</h1>

      <section className={styles.expertBand}>
        <div className={styles.text}>
          <span className={styles.label}>{t("settings.expertMode")}</span>
          <span className={styles.description}>{t("settings.expertModeDesc")}</span>
        </div>
        <Toggle
          label={t("settings.expertMode")}
          checked={settings.expertMode}
          onChange={(v) => update("expertMode", v)}
        />
      </section>

      <div className={styles.layout}>
        <nav className={styles.rail} aria-label={t("settings.categoriesAria")}>
          {categories.map((category) => (
            <button
              key={category.id}
              type="button"
              className={active === category.id ? styles.railActive : styles.railLink}
              aria-current={active === category.id || undefined}
              onClick={() => setSelected(category.id)}
            >
              {t(category.titleKey)}
            </button>
          ))}
          {!settings.expertMode && <p className={styles.railHint}>{t("settings.railHint")}</p>}
        </nav>

        <div className={styles.sections}>
          {active === "schutz" && (
            <Category
              id="schutz"
              title={t("settings.cat.schutz")}
              description={t("settings.schutzDesc")}
            >
              <Row
                label={t("settings.adsLabel")}
                description={t("settings.adsDesc")}
                details={t("settings.adsDetails")}
                control={
                  <Toggle
                    label={t("settings.adsLabel")}
                    checked={settings.blockAdsTrackers}
                    onChange={(v) => update("blockAdsTrackers", v)}
                  />
                }
              />
              <Row
                label={t("settings.malwareLabel")}
                description={t("settings.malwareDesc")}
                details={t("settings.malwareDetails")}
                control={
                  <Toggle
                    label={t("settings.malwareLabel")}
                    checked={settings.blockMalware}
                    onChange={(v) => update("blockMalware", v)}
                  />
                }
              />
              <Row
                label={t("settings.safeSearchLabel")}
                description={t("settings.safeSearchDesc")}
                details={t("settings.safeSearchDetails")}
                control={
                  <Toggle
                    label={t("settings.safeSearchLabel")}
                    checked={settings.safeSearch}
                    onChange={(v) => update("safeSearch", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "upstream" && (
            <Category
              id="upstream"
              title={t("settings.cat.upstream")}
              description={t("settings.upstreamDesc")}
            >
              <div className={styles.profile}>
                <span className={styles.labelLine}>
                  <span className={styles.label}>{t("settings.profileLabel")}</span>
                  <InfoHint label={t("settings.profileLabel")} text={t("settings.profileDetails")} />
                </span>
                <div
                  className={styles.profileSwitch}
                  role="group"
                  aria-label={t("settings.profileLabel")}
                >
                  {PROFILES.map((profile) => (
                    <button
                      key={profile}
                      type="button"
                      className={styles.profileButton}
                      data-active={settings.speedProfile === profile || undefined}
                      aria-pressed={settings.speedProfile === profile}
                      onClick={() => update("speedProfile", profile)}
                    >
                      {t(`settings.profile.${profile}` as MessageKey)}
                    </button>
                  ))}
                </div>
                <p className={styles.profileDesc}>
                  {t(`settings.profile.${settings.speedProfile}Desc` as MessageKey)}
                </p>

                <span className={styles.resolversTitle}>{t("settings.resolversTitle")}</span>
                <ul className={styles.resolvers}>
                  {PROFILE_RESOLVERS[settings.speedProfile].map((resolver) => (
                    <li key={resolver.ip} className={styles.resolver}>
                      <span className={styles.resolverIp}>{resolver.ip}</span>
                      <span className={styles.resolverHost}>{resolver.host}</span>
                      <span
                        className={styles.resolverTag}
                        data-warn={!settings.encryptUpstream || undefined}
                      >
                        {settings.encryptUpstream ? t("settings.dot") : t("settings.plaintext")}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>

              {settings.expertMode && (
                <Row
                  label={t("settings.modeLabel")}
                  description={t("settings.modeDesc")}
                  details={t("settings.modeDetails")}
                  control={
                    <select
                      className={styles.select}
                      aria-label={t("settings.modeLabel")}
                      value={settings.upstreamMode}
                      onChange={(e) => update("upstreamMode", e.target.value as UpstreamMode)}
                    >
                      {UPSTREAM_MODES.map((mode) => (
                        <option key={mode} value={mode}>
                          {t(`settings.mode.${mode}` as MessageKey)}
                        </option>
                      ))}
                    </select>
                  }
                />
              )}
            </Category>
          )}

          {active === "verschluesselung" && (
            <Category
              id="verschluesselung"
              title={t("settings.cat.verschluesselung")}
              description={t("settings.encryptionDesc")}
            >
              <Row
                label={t("settings.encryptLabel")}
                description={t("settings.encryptDesc")}
                details={t("settings.encryptDetails")}
                control={
                  <Toggle
                    label={t("settings.encryptLabel")}
                    checked={settings.encryptUpstream}
                    onChange={(v) => update("encryptUpstream", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "netzwerk" && (
            <Category
              id="netzwerk"
              title={t("settings.cat.netzwerk")}
              description={t("settings.networkDesc")}
            >
              <Row
                label={t("settings.ipv6Label")}
                description={t("settings.ipv6Desc")}
                details={t("settings.ipv6Details")}
                control={
                  <Toggle
                    label={t("settings.ipv6Label")}
                    checked={settings.ipv6}
                    onChange={(v) => update("ipv6", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "cache" && (
            <Category
              id="cache"
              title={t("settings.cat.cache")}
              description={t("settings.cacheDesc")}
            >
              <Row
                label={t("settings.clearCacheLabel")}
                description={t("settings.clearCacheDesc")}
                details={t("settings.clearCacheDetails")}
                control={<ClearCacheButton />}
              />
            </Category>
          )}

          {active === "protokoll" && (
            <Category
              id="protokoll"
              title={t("settings.cat.protokoll")}
              description={t("settings.logDesc")}
            >
              <Row
                label={t("settings.queryLogLabel")}
                description={t("settings.queryLogDesc")}
                details={t("settings.queryLogDetails")}
                control={
                  <Toggle
                    label={t("settings.queryLogLabel")}
                    checked={settings.queryLog}
                    onChange={(v) => update("queryLog", v)}
                  />
                }
              />
              <Row
                label={t("settings.retentionLabel")}
                description={t("settings.retentionDesc")}
                details={t("settings.retentionDetails")}
                disabled={!settings.queryLog}
                control={
                  <label className={styles.number}>
                    <input
                      type="number"
                      min={1}
                      max={365}
                      value={settings.logRetentionDays}
                      disabled={!settings.queryLog}
                      onChange={(e) => update("logRetentionDays", Number(e.target.value))}
                      aria-label={t("settings.retentionAria")}
                    />
                    <span>{t("settings.retentionUnit")}</span>
                  </label>
                }
              />
            </Category>
          )}
        </div>
      </div>
    </div>
  );
}
