// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState } from "react";

import { api, type HaStatus } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useAuth } from "../state/auth";
import styles from "./DemoCluster.module.css";

/** How often to refresh the (simulated) cluster roster. */
const REFRESH_MS = 5_000;

/** Strip the scheme and trailing slash so a node reads as a plain host. */
function host(url: string): string {
  return url.replace(/^https?:\/\//, "").replace(/\/+$/, "");
}

/** A read-only cluster roster, shown on the overview of the public demo node
 *  only. It reads the (server-simulated) three-node status and lists each node
 *  with a health dot, its role and version. Renders nothing off the demo node. */
export function DemoCluster() {
  const { demo } = useAuth();
  const { t } = useI18n();
  const [status, setStatus] = useState<HaStatus | null>(null);

  useEffect(() => {
    if (!demo) return;
    const controller = new AbortController();
    const load = () =>
      api
        .haStatus(controller.signal)
        .then(setStatus)
        .catch(() => {
          /* transient — keep the last roster */
        });
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => {
      controller.abort();
      window.clearInterval(timer);
    };
  }, [demo]);

  if (!demo || !status) return null;

  const nodes = [
    {
      id: status.nodeId,
      label: host(status.selfUrl ?? status.nodeId),
      primary: true,
      version: status.version,
      syncedMs: null as number | null,
    },
    ...status.peers.map((peer) => ({
      id: peer.peerNodeId ?? peer.url,
      label: host(peer.url),
      primary: false,
      version: peer.peerVersion ?? status.version,
      syncedMs: peer.lastSyncMs ?? null,
    })),
  ];

  const syncedAgo = (ms: number | null): string => {
    if (ms == null) return t("demo.cluster.healthy");
    const secs = Math.max(0, Math.round((Date.now() - ms) / 1000));
    return t("demo.cluster.synced", { n: secs });
  };

  return (
    <section className={styles.card}>
      <div className={styles.head}>
        <h2 className={styles.title}>{t("demo.cluster.title")}</h2>
        <span className={styles.subtitle}>{t("demo.cluster.subtitle", { n: nodes.length })}</span>
      </div>
      <ul className={styles.nodes}>
        {nodes.map((node) => (
          <li key={node.id} className={styles.node}>
            <span className={styles.dot} aria-hidden="true" />
            <span className={styles.nodeMain}>
              <span className={styles.nodeLabel} title={node.label}>
                {node.label}
              </span>
              <span className={styles.nodeMeta}>
                {node.primary ? t("ha.cluster.primaryTag") : t("update.cluster.follower")}
                {" · "}
                {node.version}
              </span>
            </span>
            <span className={styles.synced}>{syncedAgo(node.syncedMs)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
