import { useCallback, useEffect, useRef, useState } from "react";

import { ApiError, api, type UpdateInfo } from "../lib/api";
import { useI18n } from "../lib/i18n";
import styles from "./UpdateCard.module.css";

/** How often to poll progress while an update runs. */
const POLL_MS = 2_000;

type Flow =
  | { kind: "idle" }
  | { kind: "dialog" }
  | { kind: "running"; from: string; to: string }
  | { kind: "done"; to: string }
  | { kind: "failed"; message: string };

/** Trigger a browser download of the current configuration as a restore file. */
async function downloadRestore(): Promise<void> {
  const blob = await api.downloadBackup();
  const url = URL.createObjectURL(blob);
  const stamp = new Date().toISOString().slice(0, 10);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `justdns-restore-${stamp}.json`;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

/** The update section on the System page: it checks GitHub, shows a new release
 *  nicely, and runs a one-click update (backup → pull → recreate) via the updater
 *  sidecar — or the guided command when no sidecar is present. The sidecar prunes
 *  old images on its own, keeping the new version and its predecessor, so there is
 *  nothing to clean up here. Admin-only; mounted only for admins. */
export function UpdateCard() {
  const { t, locale } = useI18n();
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [flow, setFlow] = useState<Flow>({ kind: "idle" });
  const [backupPath, setBackupPath] = useState("");
  const [downloaded, setDownloaded] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);

  // Load the cached status once on mount.
  useEffect(() => {
    const controller = new AbortController();
    api
      .getUpdate(controller.signal)
      .then((next) => {
        setInfo(next);
        setBackupPath(next.defaultBackupPath);
      })
      .catch(() => {
        /* The System page already surfaces an unreachable backend. */
      });
    return () => controller.abort();
  }, []);

  async function recheck() {
    setChecking(true);
    try {
      const next = await api.checkUpdate();
      setInfo(next);
      setBackupPath(next.defaultBackupPath);
    } catch {
      /* Leave the previous state; the error field carries the reason. */
    } finally {
      setChecking(false);
    }
  }

  // While an update runs, poll the light status endpoint. The justdns container
  // restarts mid-flight, so fetches fail transiently — that is the "restarting"
  // state, not an error. Completion is the new version coming up, or the sidecar
  // reporting done/failed.
  const running = flow.kind === "running";
  const [phase, setPhase] = useState<string>("pulling");
  const flowRef = useRef(flow);
  flowRef.current = flow;

  useEffect(() => {
    if (!running) return;
    let active = true;
    const timer = setInterval(async () => {
      try {
        const status = await api.updateStatus();
        if (!active) return;
        const current = flowRef.current;
        if (current.kind !== "running") return;
        // The new container is up once it reports the target version.
        if (status.currentVersion === current.to) {
          setFlow({ kind: "done", to: current.to });
          return;
        }
        if (status.progress) {
          setPhase(status.progress.phase);
          if (status.progress.phase === "failed") {
            setFlow({
              kind: "failed",
              message: status.progress.message ?? "",
            });
          } else if (status.progress.phase === "done") {
            setFlow({ kind: "done", to: current.to });
          }
        }
      } catch {
        // Backend briefly gone while the container restarts: keep waiting.
        if (active) setPhase("restarting");
      }
    }, POLL_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [running]);

  const openDialog = useCallback(() => {
    setDownloaded(false);
    setDialogError(null);
    setFlow({ kind: "dialog" });
  }, []);

  async function startUpdate() {
    if (!info?.latestVersion) return;
    setDialogError(null);
    try {
      // Always hand the user a restore file, then kick off the update.
      await downloadRestore().catch(() => {
        /* Non-fatal: the server still writes its own safety backup. */
      });
      await api.startUpdate(backupPath);
      setFlow({
        kind: "running",
        from: info.currentVersion,
        to: info.latestVersion,
      });
      setPhase("pulling");
    } catch (error) {
      setDialogError(
        error instanceof ApiError ? error.message : t("common.unknownError"),
      );
    }
  }

  async function doDownload() {
    try {
      await downloadRestore();
      setDownloaded(true);
    } catch {
      setDialogError(t("update.downloadFailed"));
    }
  }

  if (!info) return null;

  // The instance's real data directory, read off the suggested backup path, so
  // the hint is accurate even when JUSTDNS_DATA_DIR is not the default /data.
  const dataDir = info.defaultBackupPath.split("/backups/")[0] || "/data";

  const lastChecked = info.checkedAtMs
    ? new Date(info.checkedAtMs).toLocaleString(locale)
    : t("update.never");

  // The guided fallback command for an install without the sidecar.
  const manualCommand = info.latestVersion
    ? `cd /opt/justdns && sed -i 's/^JUSTDNS_TAG=.*/JUSTDNS_TAG=${info.latestVersion}/' .env && docker compose pull justdns && docker compose up -d justdns`
    : "";

  return (
    <section className={styles.card} aria-label={t("update.cardTitle")}>
      <div className={styles.head}>
        <h2 className={styles.title}>{t("update.cardTitle")}</h2>
        <button
          type="button"
          className={styles.recheck}
          onClick={recheck}
          disabled={checking || flow.kind === "running"}
        >
          {checking ? t("update.checking") : t("update.recheck")}
        </button>
      </div>

      {/* Running / done / failed take over the body; otherwise the status. */}
      {flow.kind === "running" ? (
        <div className={styles.progress} role="status" aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" />
          <div>
            <strong className={styles.progressTitle}>{t("update.progressTitle")}</strong>
            <span className={styles.progressPhase}>
              {t(`update.phase.${phase}` as "update.phase.pulling")}
            </span>
          </div>
        </div>
      ) : flow.kind === "done" ? (
        <div className={styles.done} role="status">
          {t("update.phase.done", { version: flow.to })}
        </div>
      ) : flow.kind === "failed" ? (
        <div className={styles.failed} role="alert">
          {t("update.phase.failed", { message: flow.message })}
        </div>
      ) : info.error && !info.latestVersion ? (
        <p className={styles.note}>{t("update.checkFailed", { error: info.error })}</p>
      ) : info.updateAvailable && info.latestVersion ? (
        <>
          <div className={styles.available}>
            <span className={styles.badge}>{t("update.available", { version: info.latestVersion })}</span>
            <span className={styles.current}>
              {t("update.currentIs", { version: info.currentVersion })}
            </span>
          </div>
          {info.releaseNotes ? (
            <div className={styles.changelog}>
              <span className={styles.changelogTitle}>{t("update.changelogTitle")}</span>
              <pre className={styles.changelogBody}>{info.releaseNotes}</pre>
            </div>
          ) : null}
          <div className={styles.actions}>
            <button type="button" className={styles.update} onClick={openDialog}>
              {t("update.updateButton")}
            </button>
            {info.releaseUrl ? (
              <a
                className={styles.releaseLink}
                href={info.releaseUrl}
                target="_blank"
                rel="noreferrer noopener"
              >
                {t("update.openRelease")}
              </a>
            ) : null}
          </div>
        </>
      ) : (
        <p className={styles.note}>
          {t("update.upToDate")} · {t("update.currentIs", { version: info.currentVersion })}
        </p>
      )}

      <p className={styles.checked}>{t("update.lastChecked", { time: lastChecked })}</p>

      {/* The pre-update dialog: one-click (sidecar) or the guided command. */}
      {flow.kind === "dialog" && info.latestVersion ? (
        <div
          className={styles.overlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setFlow({ kind: "idle" });
          }}
        >
          <div className={styles.dialog} role="dialog" aria-modal="true" aria-label={t("update.dialogTitle", { version: info.latestVersion })}>
            <h3 className={styles.dialogTitle}>
              {t("update.dialogTitle", { version: info.latestVersion })}
            </h3>

            {info.updaterPresent ? (
              <>
                <p className={styles.dialogIntro}>{t("update.dialogIntro")}</p>
                <label className={styles.field}>
                  <span className={styles.fieldLabel}>{t("update.backupPathLabel")}</span>
                  <input
                    className={styles.input}
                    value={backupPath}
                    onChange={(event) => setBackupPath(event.target.value)}
                    spellCheck={false}
                  />
                  <span className={styles.fieldHint}>
                    {t("update.backupPathHint", { dir: dataDir })}
                  </span>
                </label>
                <button type="button" className={styles.secondary} onClick={doDownload}>
                  {downloaded ? t("update.downloadDone") : t("update.downloadRestore")}
                </button>
                {dialogError ? <p className={styles.dialogError}>{dialogError}</p> : null}
                <div className={styles.dialogActions}>
                  <button type="button" className={styles.cancel} onClick={() => setFlow({ kind: "idle" })}>
                    {t("update.cancel")}
                  </button>
                  <button type="button" className={styles.update} onClick={startUpdate}>
                    {t("update.start")}
                  </button>
                </div>
              </>
            ) : (
              <>
                <p className={styles.dialogIntro}>{t("update.manualTitle")}</p>
                <p className={styles.dialogIntro}>{t("update.manualIntro")}</p>
                <pre className={styles.command}>{manualCommand}</pre>
                <p className={styles.fieldHint}>{t("update.manualHint")}</p>
                <button type="button" className={styles.secondary} onClick={doDownload}>
                  {downloaded ? t("update.downloadDone") : t("update.downloadRestore")}
                </button>
                {dialogError ? <p className={styles.dialogError}>{dialogError}</p> : null}
                <div className={styles.dialogActions}>
                  <button type="button" className={styles.cancel} onClick={() => setFlow({ kind: "idle" })}>
                    {t("update.cancel")}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      ) : null}

    </section>
  );
}
