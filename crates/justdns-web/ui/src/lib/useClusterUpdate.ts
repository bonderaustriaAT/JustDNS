// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useState } from "react";

import { api, type ClusterNodePhase, type ClusterUpdatePhase, type ClusterUpdateState, type UpdateInfo } from "./api";
import { downloadRestore } from "./downloadRestore";
import { useI18n } from "./i18n";
import type { MessageKey } from "./messages";

/** Overall cluster-update phase → message key. Shared by the HA page and the
 *  System page so both read the rolling update the same way. */
export const CLUSTER_PHASE_KEY: Record<ClusterUpdatePhase, MessageKey> = {
  idle: "ha.cluster.phase.idle",
  running: "ha.cluster.phase.running",
  updatingPrimary: "ha.cluster.phase.updatingPrimary",
  done: "ha.cluster.phase.done",
  aborted: "ha.cluster.phase.aborted",
};

/** Per-node cluster-update phase → message key. */
export const CLUSTER_NODE_PHASE_KEY: Record<ClusterNodePhase, MessageKey> = {
  pending: "ha.cluster.node.pending",
  updating: "ha.cluster.node.updating",
  restarting: "ha.cluster.node.restarting",
  healthy: "ha.cluster.node.healthy",
  skipped: "ha.cluster.node.skipped",
  failed: "ha.cluster.node.failed",
};

/** How often to poll the cluster-update status while a run is active. */
const POLL_MS = 3_000;

/** The shared cluster-update flow: the live run state, a backup password typed
 *  twice (so a typo in a non-recoverable password is caught), a background
 *  update check, and the start call. Drives the panel on the Hochverfügbarkeit
 *  page and the "Cluster aktualisieren" button on the System page from one place,
 *  so the two never drift apart. */
export interface ClusterUpdate {
  /** The latest run state (or `null` until the first fetch). */
  state: ClusterUpdateState | null;
  /** Whether a run is active right now (so callers can disable the trigger). */
  running: boolean;
  /** The backup password and its confirmation, plus the derived checks. */
  password: string;
  confirm: string;
  setPassword: (value: string) => void;
  setConfirm: (value: string) => void;
  /** Both filled but unequal — show the mismatch hint. */
  mismatch: boolean;
  /** Non-empty and equal — the start is allowed. */
  valid: boolean;
  /** Clear both password fields (e.g. when closing the dialog). */
  resetPassword: () => void;
  /** A background update check is in flight. */
  checking: boolean;
  /** Force a fresh GitHub check; returns the info so the caller can decide
   *  whether to open the password dialog. Returns `null` on failure. */
  check: (signal?: AbortSignal) => Promise<UpdateInfo | null>;
  /** The start call is in flight. */
  busy: boolean;
  /** The last start/error message, if any. */
  error: string | null;
  clearError: () => void;
  /** Whether the optional restore-file download already succeeded (label
   *  feedback in the dialogs). */
  downloaded: boolean;
  /** A non-fatal notice: the restore file could not be downloaded. The update
   *  is unaffected — every node writes its own on-disk safety backup. Null
   *  otherwise. */
  downloadWarning: string | null;
  /** Download the optional restore file with the typed password — a courtesy
   *  copy, never a precondition for the update. Returns whether it succeeded. */
  downloadFile: () => Promise<boolean>;
  /** Start the rolling cluster update with the typed password. No-op (returns
   *  `false`) unless `valid`; returns `true` once the start call succeeds. */
  start: () => Promise<boolean>;
}

/**
 * @param enabled Whether this node is (or might be) a cluster. When `false` the
 *   hook makes no cluster-status request, so a standalone install never hits the
 *   HA endpoint. The caller flips it to `true` once it knows it is in a cluster.
 */
export function useClusterUpdate(enabled = true): ClusterUpdate {
  const { t } = useI18n();
  const [state, setState] = useState<ClusterUpdateState | null>(null);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [checking, setChecking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [downloaded, setDownloaded] = useState(false);
  const [downloadWarning, setDownloadWarning] = useState<string | null>(null);

  // One fetch to pick up a run already in progress (or the last result) — only
  // once the caller says this is a cluster, so a standalone node stays quiet.
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    api
      .haClusterUpdateStatus(controller.signal)
      .then(setState)
      .catch(() => {
        // Transient — keep whatever we have.
      });
    return () => controller.abort();
  }, [enabled]);

  const running = state?.phase === "running" || state?.phase === "updatingPrimary";

  // Poll only while a run is active, so an idle, done or aborted panel does not
  // keep hitting the server. Starting a run flips `running`, which restarts this.
  useEffect(() => {
    if (!running) return;
    const controller = new AbortController();
    const timer = setInterval(() => {
      api
        .haClusterUpdateStatus(controller.signal)
        .then(setState)
        .catch(() => {
          // Transient (e.g. the primary restarting at the end) — keep the last state.
        });
    }, POLL_MS);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [running]);

  const mismatch = password !== "" && confirm !== "" && password !== confirm;
  const valid = password !== "" && password === confirm;

  const resetPassword = useCallback(() => {
    setPassword("");
    setConfirm("");
    setDownloaded(false);
    setDownloadWarning(null);
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const check = useCallback<ClusterUpdate["check"]>(async () => {
    setChecking(true);
    try {
      return await api.checkUpdate();
    } catch {
      return null;
    } finally {
      setChecking(false);
    }
  }, []);

  // The optional courtesy copy. The update itself never waits for it: every
  // node writes its own encrypted on-disk safety backup before it updates, so
  // a flaky browser download (a phone, most of all) must not sit in front of
  // a possibly urgent cluster update.
  const downloadFile = useCallback(async (): Promise<boolean> => {
    if (password === "") return false;
    setDownloadWarning(null);
    try {
      await downloadRestore(password);
      setDownloaded(true);
      return true;
    } catch {
      setDownloadWarning(t("ha.cluster.backupDownloadFailed"));
      return false;
    }
  }, [password, t]);

  const start = useCallback(async (): Promise<boolean> => {
    if (password === "" || password !== confirm) return false;
    setBusy(true);
    setError(null);
    try {
      setState(await api.haClusterUpdate(password));
      setPassword("");
      setConfirm("");
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : t("ha.cluster.updateFailed"));
      return false;
    } finally {
      setBusy(false);
    }
  }, [password, confirm, t]);

  return {
    state,
    running,
    password,
    confirm,
    setPassword,
    setConfirm,
    mismatch,
    valid,
    resetPassword,
    checking,
    check,
    busy,
    error,
    clearError,
    downloaded,
    downloadWarning,
    downloadFile,
    start,
  };
}
