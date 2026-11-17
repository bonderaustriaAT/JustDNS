import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Toggle } from "../components/Toggle";
import { WriteGuard } from "../components/WriteGuard";
import {
  DEFAULT_NOTIFICATIONS,
  DEFAULT_PHISHING,
  api,
  type Phishing as PhishingSettings,
  type PhishingBrand,
  type PhishingMode,
  type Preset,
  type QueryRow,
  type ServerSettings,
} from "../lib/api";
import { displayName } from "../lib/dns";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Phishing.module.css";

/** Debounce before persisting a settings edit, so dragging a toggle or typing a
 *  brand does not post on every keystroke. */
const SAVE_DEBOUNCE_MS = 400;
/** How many recent queries to scan for phishing hits, and how often. */
const HITS_SCAN = 500;
const HITS_SHOWN = 20;
const HITS_REFRESH_MS = 5_000;

/** The shipped brand groups, in display order, with their heading message key. */
const GROUPS: { id: string; labelKey: "phishing.groupTechMail" | "phishing.groupBanksAt" | "phishing.groupBanksDe" }[] = [
  { id: "tech-mail", labelKey: "phishing.groupTechMail" },
  { id: "banks-at", labelKey: "phishing.groupBanksAt" },
  { id: "banks-de", labelKey: "phishing.groupBanksDe" },
];

/** A two-way warn/block segmented control. */
function ModeSegment({
  value,
  onChange,
  labelId,
}: {
  value: PhishingMode;
  onChange: (next: PhishingMode) => void;
  labelId: string;
}) {
  const { t } = useI18n();
  return (
    <div className={styles.segment} role="group" aria-labelledby={labelId}>
      {(["warn", "block"] as PhishingMode[]).map((mode) => (
        <button
          key={mode}
          type="button"
          className={styles.segmentButton}
          data-active={value === mode || undefined}
          data-mode={mode}
          aria-pressed={value === mode}
          onClick={() => onChange(mode)}
        >
          {t(mode === "warn" ? "phishing.modeWarn" : "phishing.modeBlock")}
        </button>
      ))}
    </div>
  );
}

export function Phishing() {
  const { t, locale } = useI18n();
  const [settings, setSettings] = useState<ServerSettings | null>(null);
  const [brands, setBrands] = useState<PhishingBrand[]>([]);
  const [presets, setPresets] = useState<Preset[]>([]);
  const [hits, setHits] = useState<QueryRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const saveTimer = useRef<number | undefined>(undefined);

  // Initial load: settings, the shipped brand catalogue and the lists view (for
  // the NRD feed presets). A failure leaves the page in its error state.
  useEffect(() => {
    const controller = new AbortController();
    Promise.all([
      api.settings(controller.signal),
      api.phishingBrands(controller.signal),
      api.lists(controller.signal),
    ])
      .then(([server, brandList, lists]) => {
        setSettings(server);
        setBrands(brandList);
        setPresets(lists.presets);
        setError(null);
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        setError(err instanceof Error ? err.message : t("common.unknownError"));
      });
    return () => controller.abort();
  }, [t]);

  // Poll the recent query ring for rows the detector flagged, for the preview.
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    async function load() {
      try {
        const rows = await api.queries(HITS_SCAN, "", controller.signal);
        if (!active) return;
        setHits(rows.filter((row) => row.phishing).slice(0, HITS_SHOWN));
      } catch {
        // A failed poll keeps the last preview; the next tick retries.
      }
    }
    load();
    const timer = window.setInterval(load, HITS_REFRESH_MS);
    return () => {
      active = false;
      controller.abort();
      window.clearInterval(timer);
    };
  }, []);

  // Persist the whole settings object (debounced), so optional fields like
  // geoWatch are never dropped by a partial write.
  const scheduleSave = useCallback((next: ServerSettings) => {
    if (saveTimer.current !== undefined) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      api.saveSettings(next).catch(() => {
        // A failed save leaves the optimistic UI in place; the next edit retries.
      });
    }, SAVE_DEBOUNCE_MS);
  }, []);

  const phishing: PhishingSettings = settings?.phishing ?? DEFAULT_PHISHING;
  const onPhishing = settings?.notifications?.onPhishing ?? false;
  const hasWebhook = Boolean(settings?.notifications?.webhookUrl?.trim());

  const patch = useCallback(
    (partial: Partial<PhishingSettings>) => {
      setSettings((prev) => {
        if (!prev) return prev;
        const base = prev.phishing ?? DEFAULT_PHISHING;
        const next: ServerSettings = { ...prev, phishing: { ...base, ...partial } };
        scheduleSave(next);
        return next;
      });
    },
    [scheduleSave],
  );

  const setAlert = useCallback(
    (on: boolean) => {
      setSettings((prev) => {
        if (!prev) return prev;
        const notifications = { ...DEFAULT_NOTIFICATIONS, ...prev.notifications, onPhishing: on };
        const next: ServerSettings = { ...prev, notifications };
        scheduleSave(next);
        return next;
      });
    },
    [scheduleSave],
  );

  const toggleBuiltin = useCallback(
    (id: string, on: boolean) => {
      const off = new Set(phishing.disabledBuiltinBrands);
      if (on) off.delete(id);
      else off.add(id);
      patch({ disabledBuiltinBrands: [...off] });
    },
    [patch, phishing.disabledBuiltinBrands],
  );

  const addBrand = useCallback(() => {
    const value = draft.trim().toLowerCase();
    if (!value || phishing.brands.includes(value)) {
      setDraft("");
      return;
    }
    patch({ brands: [...phishing.brands, value] });
    setDraft("");
  }, [draft, patch, phishing.brands]);

  const removeBrand = useCallback(
    (value: string) => patch({ brands: phishing.brands.filter((b) => b !== value) }),
    [patch, phishing.brands],
  );

  const togglePreset = useCallback(async (key: string, on: boolean) => {
    try {
      const lists = await api.setPreset(key, on);
      setPresets(lists.presets);
    } catch {
      // Leave the catalogue as-is; the row keeps its previous state.
    }
  }, []);

  const nrdPresets = useMemo(() => presets.filter((p) => p.category === "nrd"), [presets]);
  const disabled = new Set(phishing.disabledBuiltinBrands);
  const timeFmt = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });

  if (!settings) {
    return (
      <div className={styles.page}>
        <header className={styles.head}>
          <h1 className={styles.title}>{t("phishing.title")}</h1>
        </header>
        <p className={styles.note}>
          {error ? t("phishing.unreachable", { error }) : t("phishing.loading")}
        </p>
      </div>
    );
  }

  return (
    <WriteGuard>
      <div className={styles.page}>
        <header className={styles.head}>
          <h1 className={styles.title}>{t("phishing.title")}</h1>
          <p className={styles.intro}>{t("phishing.intro")}</p>
        </header>

        <div className={styles.master}>
          <div className={styles.masterText}>
            <span className={styles.masterLabel}>{t("phishing.masterLabel")}</span>
            <span className={styles.masterHint}>{t("phishing.masterHint")}</span>
          </div>
          <Toggle
            checked={phishing.enabled}
            onChange={(on) => patch({ enabled: on })}
            label={t("phishing.masterLabel")}
          />
        </div>

        {!phishing.enabled ? (
          <p className={styles.note}>{t("phishing.offHint")}</p>
        ) : (
          <>
            {/* Lookalike / Punycode card */}
            <section className={styles.card}>
              <div className={styles.cardHead}>
                <h2 className={styles.cardTitle}>{t("phishing.lookalikeTitle")}</h2>
                <p className={styles.cardHint}>{t("phishing.lookalikeHint")}</p>
              </div>

              <div className={styles.modeRow}>
                <span id="homoglyph-mode" className={styles.modeLabel}>
                  {t("phishing.modeLabel")}
                </span>
                <ModeSegment
                  value={phishing.homoglyphMode}
                  onChange={(mode) => patch({ homoglyphMode: mode })}
                  labelId="homoglyph-mode"
                />
                <span className={styles.modeDesc}>
                  {t(
                    phishing.homoglyphMode === "warn"
                      ? "phishing.modeWarnHint"
                      : "phishing.modeBlockHint",
                  )}
                </span>
              </div>

              <div className={styles.brandBlock}>
                <h3 className={styles.subTitle}>{t("phishing.brandsTitle")}</h3>
                <p className={styles.cardHint}>{t("phishing.brandsHint")}</p>
                {GROUPS.map((group) => {
                  const inGroup = brands.filter((b) => b.group === group.id);
                  if (inGroup.length === 0) return null;
                  return (
                    <div key={group.id} className={styles.group}>
                      <span className={styles.groupLabel}>{t(group.labelKey)}</span>
                      <div className={styles.chips}>
                        {inGroup.map((brand) => {
                          const on = !disabled.has(brand.id);
                          return (
                            <button
                              key={brand.id}
                              type="button"
                              className={styles.chip}
                              data-on={on || undefined}
                              aria-pressed={on}
                              onClick={() => toggleBuiltin(brand.id, !on)}
                            >
                              {brand.name}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  );
                })}
              </div>

              <div className={styles.brandBlock}>
                <h3 className={styles.subTitle}>{t("phishing.customTitle")}</h3>
                <p className={styles.cardHint}>{t("phishing.customHint")}</p>
                <div className={styles.addRow}>
                  <input
                    type="text"
                    className={styles.input}
                    value={draft}
                    placeholder={t("phishing.addBrandPlaceholder")}
                    onChange={(e) => setDraft(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        addBrand();
                      }
                    }}
                  />
                  <button type="button" className={styles.addButton} onClick={addBrand}>
                    {t("phishing.addBrand")}
                  </button>
                </div>
                {phishing.brands.length === 0 ? (
                  <p className={styles.empty}>{t("phishing.customNone")}</p>
                ) : (
                  <div className={styles.chips}>
                    {phishing.brands.map((brand) => (
                      <span key={brand} className={styles.customChip}>
                        {brand}
                        <button
                          type="button"
                          className={styles.remove}
                          aria-label={t("phishing.removeBrand", { brand })}
                          onClick={() => removeBrand(brand)}
                        >
                          ×
                        </button>
                      </span>
                    ))}
                  </div>
                )}
              </div>
            </section>

            {/* NRD card */}
            <section className={styles.card}>
              <div className={styles.cardHead}>
                <div className={styles.cardHeadRow}>
                  <h2 className={styles.cardTitle}>{t("phishing.nrdTitle")}</h2>
                  <Toggle
                    checked={phishing.nrdEnabled}
                    onChange={(on) => patch({ nrdEnabled: on })}
                    label={t("phishing.nrdEnableLabel")}
                  />
                </div>
                <p className={styles.cardHint}>{t("phishing.nrdHint")}</p>
              </div>

              {phishing.nrdEnabled && (
                <>
                  <div className={styles.modeRow}>
                    <span id="nrd-mode" className={styles.modeLabel}>
                      {t("phishing.modeLabel")}
                    </span>
                    <ModeSegment
                      value={phishing.nrdMode}
                      onChange={(mode) => patch({ nrdMode: mode })}
                      labelId="nrd-mode"
                    />
                    <span className={styles.modeDesc}>
                      {t(
                        phishing.nrdMode === "warn"
                          ? "phishing.modeWarnHint"
                          : "phishing.modeBlockHint",
                      )}
                    </span>
                  </div>

                  <div className={styles.brandBlock}>
                    <h3 className={styles.subTitle}>{t("phishing.nrdFeedsTitle")}</h3>
                    <p className={styles.cardHint}>{t("phishing.nrdFeedsHint")}</p>
                    {nrdPresets.length === 0 ? (
                      <p className={styles.empty}>{t("phishing.nrdNoFeeds")}</p>
                    ) : (
                      <ul className={styles.feeds}>
                        {nrdPresets.map((preset) => (
                          <li key={preset.key} className={styles.feed}>
                            <div className={styles.feedText}>
                              <span className={styles.feedName}>
                                {t(`phishing.feed.${preset.key}.name` as MessageKey)}
                              </span>
                              <span className={styles.feedDesc}>
                                {t(`phishing.feed.${preset.key}.desc` as MessageKey)}
                              </span>
                            </div>
                            <Toggle
                              checked={preset.active && preset.enabled}
                              onChange={(on) => togglePreset(preset.key, on)}
                              label={preset.name}
                            />
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                </>
              )}
            </section>

            {/* Alert + recent hits */}
            <section className={styles.card}>
              <div className={styles.alertRow}>
                <div className={styles.masterText}>
                  <span className={styles.masterLabel}>{t("phishing.alertLabel")}</span>
                  <span className={styles.masterHint}>
                    {hasWebhook ? t("phishing.alertHint") : t("phishing.alertNeedsWebhook")}
                  </span>
                </div>
                <Toggle
                  checked={onPhishing}
                  onChange={setAlert}
                  disabled={!hasWebhook}
                  label={t("phishing.alertLabel")}
                />
              </div>
            </section>

            <section className={styles.card}>
              <h2 className={styles.cardTitle}>{t("phishing.recentTitle")}</h2>
              {hits.length === 0 ? (
                <p className={styles.empty}>{t("phishing.recentNone")}</p>
              ) : (
                <ol className={styles.hits}>
                  {hits.map((row, index) => {
                    const flag = row.phishing!;
                    const kind =
                      flag.kind === "lookalike"
                        ? t("phishing.kindLookalike")
                        : flag.kind === "punycode"
                          ? t("phishing.kindPunycode")
                          : t("phishing.kindNrd");
                    return (
                      <li key={`${row.at_ms}-${index}`} className={styles.hit}>
                        <span className={styles.hitTime}>{timeFmt.format(row.at_ms)}</span>
                        <span className={styles.hitName} title={row.name}>
                          {displayName(row.name)}
                        </span>
                        <span className={styles.hitKind}>
                          {flag.brand ? `${kind} · ${flag.brand}` : kind}
                        </span>
                        <span
                          className={styles.hitBadge}
                          data-mode={flag.blocked ? "block" : "warn"}
                        >
                          {flag.blocked ? t("phishing.badgeBlock") : t("phishing.badgeWarn")}
                        </span>
                      </li>
                    );
                  })}
                </ol>
              )}
            </section>
          </>
        )}
      </div>
    </WriteGuard>
  );
}
