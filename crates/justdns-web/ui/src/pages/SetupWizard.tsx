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

/// Which way the user starts: a fresh guided setup, or restoring a backup file.
type Mode = "fresh" | "restore";

/// The wizard's steps depend on the chosen mode. The first screen is always the
/// mode choice; both modes then create the admin account (a backup carries no
/// login). A fresh setup walks the protection/upstream/finish steps; a restore
/// only needs the backup file.
const STEPS_BY_MODE: Record<Mode, readonly string[]> = {
  fresh: ["mode", "account", "protection", "upstream", "finish"],
  restore: ["mode", "account", "restore"],
};

export function SetupWizard() {
  const { t, lang, setLang } = useI18n();
  const { completeSetup } = useAuth();

  const [mode, setMode] = useState<Mode | null>(null);
  const [step, setStep] = useState(0);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [instanceName, setInstanceName] = useState("");
  const [blockLevel, setBlockLevel] = useState<Exclude<BlockLevel, "custom">>("standard");
  const [speedProfile, setSpeedProfile] = useState<SpeedProfile>("balanced");
  const [retention, setRetention] = useState(7);
  const [backupFile, setBackupFile] = useState<File | null>(null);

  // Once the admin exists its credentials cannot change, so the fields lock and
  // a second setup call is skipped if the user steps back and forward again.
  const [accountCreated, setAccountCreated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Before a mode is picked only the first screen exists; afterwards the steps
  // follow the chosen branch.
  const steps = mode ? STEPS_BY_MODE[mode] : (["mode"] as const);
  const stepName = steps[step];
  const isLast = step === steps.length - 1;

  function chooseMode(next: Mode) {
    setMode(next);
    setError(null);
    setStep(1);
  }

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

  async function restoreBackup(): Promise<boolean> {
    if (!backupFile) {
      setError(t("wizard.restore.invalid"));
      return false;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(await backupFile.text());
    } catch {
      setError(t("wizard.restore.invalid"));
      return false;
    }
    try {
      // The admin account was created on the previous step, so this call is
      // authenticated. The server validates the envelope and rejects anything
      // that is not a JustDNS backup with a 400.
      await api.restoreBackup(parsed);
      return true;
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 400) {
        setError(t("wizard.restore.invalid"));
      } else {
        setError(t("auth.setupFailed"));
      }
      return false;
    }
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      if (stepName === "account" && !accountCreated) {
        if (!(await createAccount())) return;
      }
      if (isLast) {
        const done = stepName === "restore" ? await restoreBackup() : await applyConfig();
        if (done) completeSetup(username); // Flips the gate → dashboard.
        return;
      }
      setStep((s) => s + 1);
    } finally {
      setBusy(false);
    }
  }

  const titleKey = `wizard.${stepName}.title` as MessageKey;
  const ledeKey = `wizard.${stepName}.lede` as MessageKey;

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

        {stepName !== "mode" && (
          <div className={styles.steps} aria-hidden="true">
            {steps.map((name, i) => (
              <span
                key={name}
                className={styles.step}
                data-state={i === step ? "current" : i < step ? "done" : "todo"}
              />
            ))}
            <span className={styles.stepName}>
              {t("wizard.stepCounter", { n: step + 1, total: steps.length })}
            </span>
          </div>
        )}

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

          {stepName === "mode" && (
            <div className={styles.choices} role="group" aria-label={t("wizard.mode.title")}>
              {(["fresh", "restore"] as const).map((option) => (
                <button
                  key={option}
                  type="button"
                  className={styles.choice}
                  data-active={mode === option || undefined}
                  onClick={() => chooseMode(option)}
                >
                  <span className={styles.choiceTitle}>{t(`wizard.mode.${option}` as MessageKey)}</span>
                  <span className={styles.choiceText}>
                    {t(`wizard.mode.${option}Desc` as MessageKey)}
                  </span>
                </button>
              ))}
            </div>
          )}

          {stepName === "account" && (
            <>
              {mode === "fresh" && (
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
              )}
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

          {stepName === "restore" && (
            <label className={authStyles.field}>
              <span className={authStyles.label}>{t("wizard.restore.fileLabel")}</span>
              <input
                className={styles.file}
                type="file"
                accept="application/json,.json"
                onChange={(e) => setBackupFile(e.target.files?.[0] ?? null)}
              />
              <span className={authStyles.hint}>{t("wizard.restore.fileHint")}</span>
            </label>
          )}

          {stepName === "protection" && (
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

          {stepName === "upstream" && (
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

          {stepName === "finish" && (
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

          {stepName !== "mode" && (
            <div className={styles.nav}>
              <button
                type="button"
                className={styles.back}
                onClick={() => setStep((s) => Math.max(0, s - 1))}
                disabled={busy}
              >
                {t("wizard.back")}
              </button>
              <button
                className={styles.next}
                type="submit"
                disabled={busy || (stepName === "restore" && !backupFile)}
              >
                {busy
                  ? t("wizard.working")
                  : isLast
                    ? stepName === "restore"
                      ? t("wizard.restore.finishButton")
                      : t("wizard.finishButton")
                    : t("wizard.next")}
              </button>
            </div>
          )}
        </form>
      </div>
    </div>
  );
}
