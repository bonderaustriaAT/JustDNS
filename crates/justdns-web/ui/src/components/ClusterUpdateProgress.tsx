// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ClusterNodePhase, ClusterUpdateState } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { CLUSTER_NODE_PHASE_KEY, CLUSTER_PHASE_KEY } from "../lib/useClusterUpdate";
import styles from "./ClusterUpdateProgress.module.css";

/** How full each node's ring reads for a given phase. The rolling update reports
 *  discrete phases, not a byte count, so the ring snaps between these levels as a
 *  node advances — pending is empty, a finished node is full. */
const NODE_FILL: Record<ClusterNodePhase, number> = {
  pending: 0,
  updating: 55,
  restarting: 85,
  healthy: 100,
  skipped: 100,
  failed: 100,
};

/** The colour family a node's ring and pill wear, keyed off its phase. */
type Tone = "wait" | "active" | "done" | "failed";
const NODE_TONE: Record<ClusterNodePhase, Tone> = {
  pending: "wait",
  updating: "active",
  restarting: "active",
  healthy: "done",
  skipped: "done",
  failed: "failed",
};

/** Ring geometry: radius 26 → circumference for the stroke-dash maths. */
const RADIUS = 26;
const CIRC = 2 * Math.PI * RADIUS;

/** The live cluster-update view: an aggregate progress bar with a percentage, a
 *  tile per node with a ring showing its own progress, and — while the rollout
 *  runs — a standing reminder not to change settings until every node is up on
 *  the new version. Fed the shared `ClusterUpdateState`, so the System page and
 *  the Hochverfügbarkeit page can show the same rollout the same way. */
export function ClusterUpdateProgress({ state }: { state: ClusterUpdateState }) {
  const { t } = useI18n();
  const nodes = state.nodes;
  const total = nodes.length;
  const doneCount = nodes.filter(
    (node) => node.phase === "healthy" || node.phase === "skipped",
  ).length;
  // A failed node contributes nothing to the aggregate: its ring still reads full
  // (in red), but counting it as 100% done would make the bar imply success the
  // rollout did not reach.
  const sum = nodes.reduce(
    (acc, node) => acc + (node.phase === "failed" ? 0 : NODE_FILL[node.phase]),
    0,
  );
  const overall = state.phase === "done" ? 100 : total ? Math.round(sum / total) : 0;
  const running = state.phase === "running" || state.phase === "updatingPrimary";

  return (
    <div className={styles.wrap} role="status" aria-live="polite">
      <div className={styles.head}>
        <strong className={styles.phase}>
          {t(CLUSTER_PHASE_KEY[state.phase])}
          {state.target ? ` → ${state.target}` : ""}
        </strong>
        {total ? (
          <span className={styles.count}>
            {t("update.cluster.nodesDone", { done: doneCount, total })}
          </span>
        ) : null}
      </div>

      <div className={styles.barRow}>
        <div
          className={styles.bar}
          role="progressbar"
          aria-valuenow={overall}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <i style={{ width: `${overall}%` }} data-done={state.phase === "done"} />
        </div>
        <span className={styles.pct}>{overall}%</span>
      </div>

      {state.message ? <p className={styles.message}>{state.message}</p> : null}

      <ul className={styles.tiles}>
        {nodes.map((node) => {
          const tone = NODE_TONE[node.phase];
          const fill = NODE_FILL[node.phase];
          const offset = CIRC * (1 - fill / 100);
          return (
            <li key={node.nodeId} className={styles.tile}>
              <div className={styles.ring} data-tone={tone}>
                <svg viewBox="0 0 64 64" aria-hidden="true">
                  <circle className={styles.track} cx="32" cy="32" r={RADIUS} />
                  <circle
                    className={styles.value}
                    cx="32"
                    cy="32"
                    r={RADIUS}
                    strokeDasharray={CIRC}
                    strokeDashoffset={offset}
                  />
                </svg>
                <span className={styles.ringNum} data-tone={tone}>
                  {tone === "failed" ? "!" : `${fill}%`}
                </span>
              </div>
              <span className={styles.tileLabel} title={node.label}>
                {node.label}
              </span>
              <span className={styles.tileRole}>
                {node.isPrimary ? t("ha.cluster.primaryTag") : t("update.cluster.follower")}
              </span>
              <span className={styles.tilePhase} data-tone={tone}>
                {t(CLUSTER_NODE_PHASE_KEY[node.phase])}
              </span>
              {node.error ? <span className={styles.tileError}>{node.error}</span> : null}
            </li>
          );
        })}
      </ul>

      {running ? (
        <p className={styles.warn}>
          <span className={styles.warnIcon} aria-hidden="true" />
          {t("update.cluster.warnRunning")}
        </p>
      ) : null}
    </div>
  );
}
