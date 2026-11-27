import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";

import {
  ApiError,
  api,
  type HaStatus,
  type UpdateChannel,
  type UpdateInfo,
} from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useClusterUpdate } from "../lib/useClusterUpdate";
import { downloadRestore } from "../lib/downloadRestore";
import { ClusterUpdateProgress } from "./ClusterUpdateProgress";
import { ConfirmDialog } from "./ConfirmDialog";
import styles from "./UpdateCard.module.css";

/** How often to poll progress while an update runs. */
const POLL_MS = 2_000;

type Flow =
  | { kind: "idle" }
  | { kind: "dialog" }
  | { kind: "running"; from: string; to: string }
  | { kind: "done"; to: string }
  | { kind: "failed"; message: string };

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
  // The password the pre-update backup is encrypted with, typed twice so a typo
  // in a password that cannot be recovered is caught before the update runs.
  const [backupPw, setBackupPw] = useState("");
  const [backupPw2, setBackupPw2] = useState("");

  // Cluster awareness: on a primary in a cluster the one-click update becomes a
  // rolling cluster update; a follower manages updates from the primary instead.
  const [ha, setHa] = useState<HaStatus | null>(null);
  // `haLoaded` becomes true once the HA status has resolved (success or failure),
  // so we never offer an update action while it is still unknown whether this is
  // a standalone node, a cluster primary or a follower — a click in that window
  // would otherwise start the wrong (single-node) update.
  const [haLoaded, setHaLoaded] = useState(false);
  const [clusterDialog, setClusterDialog] = useState(false);
  const inCluster = (ha?.peers.length ?? 0) > 0;
  const isPrimary = ha?.isPrimary ?? true;
  const clusterMode = inCluster && isPrimary;
  const isFollower = inCluster && !isPrimary;
  const cluster = useClusterUpdate(clusterMode);

  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    api
      .haStatus(controller.signal)
      .then((status) => {
        if (active) setHa(status);
      })
      .catch(() => {
        /* No HA status just means we treat this as a standalone node. */
      })
      .finally(() => {
        if (active) setHaLoaded(true);
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, []);

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

  // Switching the release channel is per node (never synced) and refreshes the
  // available-update view against the newly selected channel.
  const [channelBusy, setChannelBusy] = useState(false);
  async function pickChannel(channel: UpdateChannel) {
    if (channelBusy || info?.channel === channel) return;
    setChannelBusy(true);
    try {
      const next = await api.setUpdateChannel(channel);
      setInfo(next);
      setBackupPath(next.defaultBackupPath);
    } catch {
      /* Leave the previous state; the error field carries the reason. */
    } finally {
      setChannelBusy(false);
    }
  }

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
    setBackupPw("");
    setBackupPw2("");
    setFlow({ kind: "dialog" });
  }, []);

  const pwMismatch = backupPw !== "" && backupPw2 !== "" && backupPw !== backupPw2;
  const pwValid = backupPw !== "" && backupPw === backupPw2;

  async function startUpdate() {
    if (!info?.latestVersion) return;
    if (!pwValid) {
      setDialogError(t("update.passwordMismatch"));
      return;
    }
    setDialogError(null);
    try {
      // No download here: the server writes its own encrypted safety backup to
      // the path shown in the dialog before the sidecar pulls anything, so the
      // update never depends on a browser download. The restore file above is
      // a pure courtesy copy — easy to skip on a phone, where blob downloads
      // are flaky and the file has nowhere sensible to go anyway.
      await api.startUpdate(backupPath, backupPw);
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
    if (!pwValid) {
      setDialogError(t("update.passwordMismatch"));
      return;
    }
    setDialogError(null);
    try {
      await downloadRestore(backupPw);
      setDownloaded(true);
    } catch {
      setDialogError(t("update.downloadFailed"));
    }
  }

  // The cluster path (primary in a cluster): check for a release first, then a
  // password-confirmed rolling update driven by the shared hook.
  async function openClusterDialog() {
    cluster.clearError();
    const next = await cluster.check();
    if (next) {
      setInfo(next);
      setBackupPath(next.defaultBackupPath);
      if (!next.updateAvailable) return; // Card now shows "up to date".
    }
    cluster.resetPassword();
    setClusterDialog(true);
  }
  function closeClusterDialog() {
    setClusterDialog(false);
    cluster.resetPassword();
  }
  async function confirmCluster() {
    if (await cluster.start()) closeClusterDialog();
  }

  // A follower manages updates from the primary; never a local one-click update.
  if (isFollower) {
    return (
      <section className={styles.card} aria-label={t("update.cardTitle")}>
        <div className={styles.head}>
          <h2 className={styles.title}>{t("update.cardTitle")}</h2>
        </div>
        <p className={styles.note}>
          {t("update.manageOnPrimary")}
          {ha?.primaryUrl ? (
            <>
              {" "}
              <a
                className={styles.releaseLink}
                href={ha.primaryUrl}
                target="_blank"
                rel="noreferrer noopener"
              >
                {ha.primaryUrl}
              </a>
            </>
          ) : null}
        </p>
      </section>
    );
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

      {/* Release channel: stable (default) or beta, chosen per node. */}
      <div className={styles.channel}>
        <span className={styles.channelLabel}>{t("update.channelLabel")}</span>
        <div
          className={styles.channelCards}
          role="radiogroup"
          aria-label={t("update.channelLabel")}
        >
          {(["stable", "beta"] as const).map((ch) => {
            const active = info.channel === ch;
            return (
              <button
                key={ch}
                type="button"
                role="radio"
                aria-checked={active}
                className={`${styles.channelCard} ${active ? styles.channelActive : ""}`}
                onClick={() => pickChannel(ch)}
                disabled={channelBusy || flow.kind === "running"}
              >
                <span className={styles.channelName}>
                  {t(ch === "stable" ? "update.channelStable" : "update.channelBeta")}
                  {active ? (
                    <span className={styles.channelBadge}>{t("update.channelActive")}</span>
                  ) : null}
                </span>
                <span className={styles.channelDesc}>
                  {t(
                    ch === "stable"
                      ? "update.channelStableDesc"
                      : "update.channelBetaDesc",
                  )}
                </span>
              </button>
            );
          })}
        </div>
        {info.channel === "beta" ? (
          <p className={styles.channelWarn} role="note">
            {t("update.channelBetaWarn")}
          </p>
        ) : null}
        {info.channel === "beta" && info.updaterPresent && !info.updaterRcCapable ? (
          <p className={styles.channelBlock} role="alert">
            {t("update.updaterTooOld")}
          </p>
        ) : null}
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
            <button
              type="button"
              className={styles.update}
              onClick={clusterMode ? openClusterDialog : openDialog}
              disabled={!haLoaded || (clusterMode && (cluster.checking || cluster.running))}
            >
              {clusterMode
                ? cluster.checking
                  ? t("update.checking")
                  : t("update.updateCluster")
                : t("update.updateButton")}
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

      {clusterMode && cluster.error ? (
        <p className={styles.note} role="alert">{cluster.error}</p>
      ) : null}
      {clusterMode && cluster.downloadWarning ? (
        <p className={styles.note} role="status">{cluster.downloadWarning}</p>
      ) : null}

      {clusterMode && cluster.state && cluster.state.phase !== "idle" ? (
        <>
          <ClusterUpdateProgress state={cluster.state} />
          <Link className={styles.releaseLink} to="/ha">
            {t("update.clusterDetails")}
          </Link>
        </>
      ) : null}

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

            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("update.backupPasswordLabel")}</span>
              <input
                className={styles.input}
                type="password"
                autoComplete="new-password"
                value={backupPw}
                onChange={(event) => setBackupPw(event.target.value)}
                placeholder={t("update.backupPasswordPlaceholder")}
              />
            </label>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("update.backupPasswordConfirmLabel")}</span>
              <input
                className={styles.input}
                type="password"
                autoComplete="new-password"
                value={backupPw2}
                onChange={(event) => setBackupPw2(event.target.value)}
                placeholder={t("update.backupPasswordConfirmPlaceholder")}
              />
              <span className={styles.fieldHint}>
                {pwMismatch
                  ? t("update.passwordMismatch")
                  : t("update.backupPasswordHint")}
              </span>
            </label>

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
                <button
                  type="button"
                  className={styles.secondary}
                  onClick={doDownload}
                  disabled={!pwValid}
                >
                  {downloaded ? t("update.downloadDone") : t("update.downloadRestore")}
                </button>
                {dialogError ? <p className={styles.dialogError}>{dialogError}</p> : null}
                <div className={styles.dialogActions}>
                  <button type="button" className={styles.cancel} onClick={() => setFlow({ kind: "idle" })}>
                    {t("update.cancel")}
                  </button>
                  <button
                    type="button"
                    className={styles.update}
                    onClick={startUpdate}
                    disabled={!pwValid}
                  >
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
                <button
                  type="button"
                  className={styles.secondary}
                  onClick={doDownload}
                  disabled={!pwValid}
                >
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

      {clusterDialog && info.latestVersion ? (
        <ConfirmDialog
          title={t("update.dialogTitle", { version: info.latestVersion })}
          body={t("ha.cluster.updateBody")}
          details={
            <>
              <p className={styles.dialogWarn} role="alert">
                <span className={styles.dialogWarnIcon} aria-hidden="true" />
                {t("update.cluster.warnDialog")}
              </p>
              <label className={styles.field}>
                <span className={styles.fieldLabel}>{t("update.backupPasswordLabel")}</span>
                <input
                  className={styles.input}
                  type="password"
                  autoComplete="new-password"
                  value={cluster.password}
                  onChange={(event) => cluster.setPassword(event.target.value)}
                  placeholder={t("update.backupPasswordPlaceholder")}
                />
              </label>
              <label className={styles.field}>
                <span className={styles.fieldLabel}>{t("update.backupPasswordConfirmLabel")}</span>
                <input
                  className={styles.input}
                  type="password"
                  autoComplete="new-password"
                  value={cluster.confirm}
                  onChange={(event) => cluster.setConfirm(event.target.value)}
                  placeholder={t("update.backupPasswordConfirmPlaceholder")}
                />
                <span className={styles.fieldHint}>
                  {cluster.mismatch
                    ? t("update.passwordMismatch")
                    : t("update.backupPasswordHint")}
                </span>
              </label>
              <button
                type="button"
                className={styles.secondary}
                onClick={() => cluster.downloadFile()}
                disabled={!cluster.valid}
              >
                {cluster.downloaded
                  ? t("update.downloadDone")
                  : t("update.downloadRestore")}
              </button>
              {cluster.downloadWarning && (
                <p className={styles.dialogError} role="status">
                  {cluster.downloadWarning}
                </p>
              )}
              {cluster.error && (
                <p className={styles.dialogError} role="alert">{cluster.error}</p>
              )}
            </>
          }
          confirmLabel={t("update.updateCluster")}
          cancelLabel={t("update.cancel")}
          busy={cluster.busy}
          confirmDisabled={!cluster.valid}
          onConfirm={confirmCluster}
          onCancel={closeClusterDialog}
        />
      ) : null}

    </section>
  );
}
