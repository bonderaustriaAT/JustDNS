import { useState, type FormEvent } from "react";

import { ApiError, api, type BlockLevel, type SpeedProfile } from "../lib/api";
import { LANGS, useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useAuth } from "../state/auth";
import authStyles from "./Auth.module.css";
import styles from "./Wizard.module.css";

/// Mirrored from the server's own check so a too-short password is caught here.
const MIN_PASSWORD = 8;

/// The levels the wizard offers, in order of strength. `custom` is never a
/// choice — it only ever results from hand-tuning later.
const LEVELS: Exclude<BlockLevel, "custom">[] = ["off", "minimal", "standard", "extreme"];

/// The upstream profiles, reusing the settings page's choice.
const PROFILES: SpeedProfile[] = ["privacy", "balanced", "speed"];

/// How long to keep query-log entries, in days.
const RETENTIONS = [7, 30, 90, 365];

/// Coarse false-positive risk per level, driving the coloured note.
const LEVEL_RISK: Record<string, "low" | "medium" | "high"> = {
  off: "low",
  minimal: "low",
  standard: "low",
  extreme: "high",
};

/// The wizard's four steps, in order.
const STEPS = ["account", "protection", "upstream", "finish"] as const;
type Step = number;

export function SetupWizard() {
  const { t, lang, setLang } = useI18n();
  const { completeSetup } = useAuth();

  const [step, setStep] = useState<Step>(0);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [instanceName, setInstanceName] = useState("");
  const [blockLevel, setBlockLevel] = useState<Exclude<BlockLevel, "custom">>("standard");
  const [speedProfile, setSpeedProfile] = useState<SpeedProfile>("balanced");
  const [retention, setRetention] = useState(7);

  // Once the admin exists its credentials cannot change, so the fields lock and
  // a second setup call is skipped if the user steps back and forward again.
  const [accountCreated, setAccountCreated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const isLast = step === STEPS.length - 1;

  async function createAccount(): Promise<boolean> {
    if (password.length < MIN_PASSWORD) {
      setError(t("auth.passwordTooShort", { min: MIN_PASSWORD }));
      return false;
    }
    if (password !== confirm) {
      setError(t("auth.passwordsDiffer"));
      return false;
    }
    try {
      await api.setup(username, password);
      setAccountCreated(true);
      return true;
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 409) {
        setError(t("auth.alreadyConfigured"));
      } else {
        setError(t("auth.setupFailed"));
      }
      return false;
    }
  }

  async function applyConfig(): Promise<boolean> {
    try {
      // Start from the stored settings so untouched fields keep their defaults.
      const current = await api.settings();
      await api.saveSettings({
        ...current,
        instanceName: instanceName.trim() || "JustDNS",
        speedProfile,
        logRetentionDays: retention,
      });
      await api.setBlockLevel(blockLevel);
      return true;
    } catch {
      setError(t("auth.setupFailed"));
      return false;
    }
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      if (step === 0 && !accountCreated) {
        if (!(await createAccount())) return;
      }
      if (isLast) {
        if (await applyConfig()) {
          completeSetup(username); // Flips the gate → dashboard.
        }
        return;
      }
      setStep((s) => s + 1);
    } finally {
      setBusy(false);
    }
  }

  const titleKey = `wizard.${STEPS[step]}.title` as MessageKey;
  const ledeKey = `wizard.${STEPS[step]}.lede` as MessageKey;

  return (
    <div className={authStyles.screen}>
      <div className={`${authStyles.card} ${styles.card}`}>
        <div className={styles.brandRow}>
          <div className={authStyles.brand}>
            <span className={authStyles.mark} aria-hidden="true" />
            <span className={authStyles.wordmark}>JustDNS</span>
          </div>
          <div className={styles.langSwitch} role="group" aria-label={t("lang.label")}>
            {LANGS.map((code) => (
              <button
                key={code}
                type="button"
                className={styles.langButton}
                data-active={code === lang || undefined}
                aria-pressed={code === lang}
                onClick={() => setLang(code)}
              >
                {t(`lang.${code}`)}
              </button>
            ))}
          </div>
        </div>

        <div className={styles.steps} aria-hidden="true">
          {STEPS.map((name, i) => (
            <span
              key={name}
              className={styles.step}
              data-state={i === step ? "current" : i < step ? "done" : "todo"}
            />
          ))}
          <span className={styles.stepName}>
            {t("wizard.stepCounter", { n: step + 1, total: STEPS.length })}
          </span>
        </div>

        <div className={authStyles.head}>
          <h1 className={authStyles.title}>{t(titleKey)}</h1>
          <p className={authStyles.lede}>{t(ledeKey)}</p>
        </div>

        <form className={authStyles.form} onSubmit={onSubmit}>
          {error && (
            <p className={authStyles.error} role="alert">
              {error}
            </p>
          )}

          {step === 0 && (
            <>
              <label className={authStyles.field}>
                <span className={authStyles.label}>{t("wizard.instanceName")}</span>
                <input
                  className={authStyles.input}
                  type="text"
                  value={instanceName}
                  placeholder="JustDNS"
                  onChange={(e) => setInstanceName(e.target.value)}
                />
                <span className={authStyles.hint}>{t("wizard.instanceNameHint")}</span>
              </label>
              <label className={authStyles.field}>
                <span className={authStyles.label}>{t("auth.username")}</span>
                <input
                  className={authStyles.input}
                  type="text"
                  autoComplete="username"
                  autoFocus
                  value={username}
                  disabled={accountCreated}
                  onChange={(e) => setUsername(e.target.value)}
                  required
                />
              </label>
              <label className={authStyles.field}>
                <span className={authStyles.label}>{t("auth.password")}</span>
                <input
                  className={authStyles.input}
                  type="password"
                  autoComplete="new-password"
                  value={password}
                  disabled={accountCreated}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                />
                <span className={authStyles.hint}>{t("auth.minChars", { min: MIN_PASSWORD })}</span>
              </label>
              <label className={authStyles.field}>
                <span className={authStyles.label}>{t("auth.repeatPassword")}</span>
                <input
                  className={authStyles.input}
                  type="password"
                  autoComplete="new-password"
                  value={confirm}
                  disabled={accountCreated}
                  onChange={(e) => setConfirm(e.target.value)}
                  required
                />
              </label>
            </>
          )}

          {step === 1 && (
            <>
              <div className={styles.segmented} role="group" aria-label={t("wizard.protection.title")}>
                {LEVELS.map((level) => (
                  <button
                    key={level}
                    type="button"
                    className={styles.segment}
                    data-active={blockLevel === level || undefined}
                    aria-pressed={blockLevel === level}
                    onClick={() => setBlockLevel(level)}
                  >
                    {t(`wizard.level.${level}` as MessageKey)}
                  </button>
                ))}
              </div>
              <p className={styles.choiceDesc}>{t(`wizard.level.${blockLevel}Desc` as MessageKey)}</p>
              <p className={styles.fpNote}>
                <span className={styles.fpLabel}>{t("wizard.fpLabel")}</span>
                <span className={styles.fpValue} data-risk={LEVEL_RISK[blockLevel]}>
                  {t(`wizard.risk.${LEVEL_RISK[blockLevel]}` as MessageKey)}
                </span>
                <span className={styles.fpLabel}>· {t(`wizard.level.${blockLevel}Fp` as MessageKey)}</span>
              </p>
            </>
          )}

          {step === 2 && (
            <>
              <div className={styles.segmented} role="group" aria-label={t("wizard.upstream.title")}>
                {PROFILES.map((profile) => (
                  <button
                    key={profile}
                    type="button"
                    className={styles.segment}
                    data-active={speedProfile === profile || undefined}
                    aria-pressed={speedProfile === profile}
                    onClick={() => setSpeedProfile(profile)}
                  >
                    {t(`settings.profile.${profile}` as MessageKey)}
                  </button>
                ))}
              </div>
              <p className={styles.choiceDesc}>
                {t(`settings.profile.${speedProfile}Desc` as MessageKey)}
              </p>
            </>
          )}

          {step === 3 && (
            <>
              <label className={authStyles.field}>
                <span className={authStyles.label}>{t("wizard.retentionLabel")}</span>
                <select
                  className={styles.select}
                  value={retention}
                  onChange={(e) => setRetention(Number(e.target.value))}
                >
                  {RETENTIONS.map((days) => (
                    <option key={days} value={days}>
                      {t("wizard.retentionDays", { n: days })}
                    </option>
                  ))}
                </select>
              </label>
              <ul className={styles.summary}>
                <li className={styles.summaryRow}>
                  <span className={styles.summaryLabel}>{t("wizard.summary.name")}</span>
                  <span className={styles.summaryValue}>{instanceName.trim() || "JustDNS"}</span>
                </li>
                <li className={styles.summaryRow}>
                  <span className={styles.summaryLabel}>{t("wizard.summary.protection")}</span>
                  <span className={styles.summaryValue}>
                    {t(`wizard.level.${blockLevel}` as MessageKey)}
                  </span>
                </li>
                <li className={styles.summaryRow}>
                  <span className={styles.summaryLabel}>{t("wizard.summary.upstream")}</span>
                  <span className={styles.summaryValue}>
                    {t(`settings.profile.${speedProfile}` as MessageKey)}
                  </span>
                </li>
                <li className={styles.summaryRow}>
                  <span className={styles.summaryLabel}>{t("wizard.summary.retention")}</span>
                  <span className={styles.summaryValue}>{t("wizard.retentionDays", { n: retention })}</span>
                </li>
              </ul>
            </>
          )}

          <div className={styles.nav}>
            {step > 0 && (
              <button
                type="button"
                className={styles.back}
                onClick={() => setStep((s) => Math.max(0, s - 1))}
                disabled={busy}
              >
                {t("wizard.back")}
              </button>
            )}
            <button className={styles.next} type="submit" disabled={busy}>
              {busy
                ? t("wizard.working")
                : isLast
                  ? t("wizard.finishButton")
                  : t("wizard.next")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
