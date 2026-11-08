import { useCallback, useEffect, useState } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { Toggle } from "../components/Toggle";
import { api, type HaPeer, type HaStatus, type HaPrefer } from "../lib/api";
import { useI18n } from "../lib/i18n";
import styles from "./HighAvailability.module.css";

/** How often the status refreshes while the page is open, so a sync that runs on
 *  the background thread shows up here without a reload. */
const REFRESH_MS = 5_000;

/** The most nodes a cluster holds (one node plus up to three peers). */
const MAX_NODES = 4;

/** The health of one peer, derived from the last sync and any error. */
type Health = "ok" | "stale" | "error" | "pending";

function peerHealth(peer: HaPeer, intervalSecs: number): Health {
  if (peer.lastError) return "error";
  if (peer.lastSyncMs == null) return "pending";
  // A healthy peer syncs every interval; allow a few missed cycles before the dot
  // turns amber, so a single slow round does not look like a fault.
  const staleAfter = Math.max(intervalSecs * 3, 60) * 1_000;
  return Date.now() - peer.lastSyncMs < staleAfter ? "ok" : "stale";
}

/** Load the HA status and keep it fresh. What is on screen stays put across a
 *  refetch so the panel never flashes empty. */
function useHaStatus() {
  const { t } = useI18n();
  const [status, setStatus] = useState<HaStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(
    async (signal?: AbortSignal) => {
      try {
        const next = await api.haStatus(signal);
        setStatus(next);
        setError(null);
      } catch (err) {
        if (signal?.aborted) return;
        setError(err instanceof Error ? err.message : t("ha.loadFailed"));
      }
    },
    [t],
  );

  useEffect(() => {
    const controller = new AbortController();
    reload(controller.signal);
    const timer = setInterval(() => reload(controller.signal), REFRESH_MS);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [reload]);

  return { status, error, reload };
}

export function HighAvailability() {
  const { lang } = useI18n();
  const { status, error, reload } = useHaStatus();

  if (error && !status) {
    return (
      <div className={styles.page}>
        <Header nodeId={null} />
        <p className={styles.banner}>{error}</p>
      </div>
    );
  }
  if (!status) {
    return (
      <div className={styles.page}>
        <Header nodeId={null} />
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <Header nodeId={status.nodeId} />
      {status.paired ? (
        <Cluster status={status} lang={lang} reload={reload} />
      ) : (
        <Wizard status={status} reload={reload} />
      )}
    </div>
  );
}

function Header({ nodeId }: { nodeId: string | null }) {
  const { t } = useI18n();
  return (
    <header className={styles.header}>
      <div>
        <h1 className={styles.title}>{t("ha.title")}</h1>
        <p className={styles.lead}>{t("ha.lead")}</p>
      </div>
      {nodeId && (
        <p className={styles.nodeId}>
          {t("ha.nodeId")}
          <code>{nodeId}</code>
        </p>
      )}
    </header>
  );
}

/** One step shell: a numbered marker that becomes a check when done, a title, and
 *  the step's body only while it is the active step (done and upcoming steps stay
 *  collapsed). */
function Step({
  index,
  state,
  title,
  children,
}: {
  index: number;
  state: "done" | "active" | "upcoming";
  title: string;
  children?: React.ReactNode;
}) {
  const { t } = useI18n();
  return (
    <section className={styles.step} data-state={state}>
      <div className={styles.marker} aria-hidden="true">
        {state === "done" ? (
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
            <path d="M5 13l4 4L19 7" />
          </svg>
        ) : (
          index
        )}
      </div>
      <div className={styles.stepBody}>
        <h2 className={styles.stepTitle}>
          {title}
          {state === "done" && <span className={styles.doneTag}>{t("ha.stepDone")}</span>}
        </h2>
        {state === "active" && <div className={styles.stepForm}>{children}</div>}
      </div>
    </section>
  );
}

/** The guided set-up: enable → shared key → own address → join, each step
 *  unlocking the next. A node sits at the join step until it is pointed at another
 *  node (or another node joins it), then the view switches to the cluster list. */
function Wizard({ status, reload }: { status: HaStatus; reload: () => Promise<void> }) {
  const { t } = useI18n();
  // The first unfinished step is the active one.
  const active = !status.enabled ? 1 : !status.keySet ? 2 : !status.selfUrl ? 3 : 4;
  const stepState = (n: number): "done" | "active" | "upcoming" =>
    n < active ? "done" : n === active ? "active" : "upcoming";

  return (
    <div className={styles.steps}>
      <Step index={1} state={stepState(1)} title={t("ha.enable.title")}>
        <EnableStep status={status} reload={reload} />
      </Step>
      <Step index={2} state={stepState(2)} title={t("ha.key.title")}>
        <KeyStep status={status} reload={reload} />
      </Step>
      <Step index={3} state={stepState(3)} title={t("ha.address.title")}>
        <AddressStep status={status} reload={reload} />
      </Step>
      <Step index={4} state={stepState(4)} title={t("ha.connect.title")}>
        <ConnectStep reload={reload} />
      </Step>
    </div>
  );
}

function EnableStep({ status, reload }: { status: HaStatus; reload: () => Promise<void> }) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);

  async function toggle(next: boolean) {
    setBusy(true);
    try {
      await api.haEnable(next);
      await reload();
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <p className={styles.stepLead}>{t("ha.enable.body")}</p>
      <div className={styles.inlineRow}>
        <Toggle
          checked={status.enabled}
          onChange={toggle}
          disabled={busy}
          label={t("ha.enable.toggleAria")}
        />
        <span className={styles.stateLabel}>
          {status.enabled ? t("ha.enable.on") : t("ha.enable.off")}
        </span>
      </div>
    </>
  );
}

function KeyStep({ status, reload }: { status: HaStatus; reload: () => Promise<void> }) {
  const { t } = useI18n();
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (status.keyFromEnv) {
    return <p className={styles.stepLead}>{t("ha.key.fromEnv")}</p>;
  }

  async function save(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.haSetKey(value);
      setValue("");
      setSaved(true);
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("ha.loadFailed"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className={styles.form} onSubmit={save}>
      <p className={styles.stepLead}>{t("ha.key.body")}</p>
      <label className={styles.label} htmlFor="ha-key">
        {t("ha.key.label")}
      </label>
      <input
        id="ha-key"
        className={styles.input}
        type="password"
        autoComplete="off"
        value={value}
        placeholder={t("ha.key.placeholder")}
        onChange={(event) => setValue(event.target.value)}
      />
      {error && <p className={styles.fieldError}>{error}</p>}
      {saved && !error && <p className={styles.fieldOk}>{t("ha.key.saved")}</p>}
      <button className={styles.buttonPrimary} type="submit" disabled={busy || value.trim().length === 0}>
        {t("ha.key.save")}
      </button>
    </form>
  );
}

/** The node's own address: how the rest of the cluster reaches back to it. It is
 *  advertised in the roster, so without it a node can pull but stays invisible to
 *  new joiners. Prefilled from the browser's origin; the admin can correct it. */
function AddressStep({ status, reload }: { status: HaStatus; reload: () => Promise<void> }) {
  const { t } = useI18n();
  const fallback = typeof window !== "undefined" ? window.location.origin : "";
  const [value, setValue] = useState(status.selfUrl ?? fallback);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.haSetAddress(value.trim());
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("ha.loadFailed"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className={styles.form} onSubmit={save}>
      <p className={styles.stepLead}>{t("ha.address.body")}</p>
      <label className={styles.label} htmlFor="ha-self-url">
        {t("ha.address.label")}
      </label>
      <input
        id="ha-self-url"
        className={styles.input}
        type="url"
        inputMode="url"
        value={value}
        placeholder={t("ha.address.placeholder")}
        onChange={(event) => setValue(event.target.value)}
      />
      <p className={styles.hint}>{t("ha.address.hint")}</p>
      {error && <p className={styles.fieldError}>{error}</p>}
      <button className={styles.buttonPrimary} type="submit" disabled={busy || value.trim().length === 0}>
        {t("ha.address.save")}
      </button>
    </form>
  );
}

/** Join the cluster by pointing at any one member (the seed). The rest of the mesh
 *  is discovered and connected automatically. */
function ConnectStep({ reload }: { reload: () => Promise<void> }) {
  const { t } = useI18n();
  const [url, setUrl] = useState("");
  const [prefer, setPrefer] = useState<HaPrefer>("peer");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function connect(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.haConnect(url.trim(), prefer);
      await reload();
    } catch (err) {
      const message = err instanceof Error ? err.message : "";
      setError(t("ha.connect.failed", { error: message }));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className={styles.form} onSubmit={connect}>
      <p className={styles.stepLead}>{t("ha.connect.body")}</p>

      <label className={styles.label} htmlFor="ha-url">
        {t("ha.connect.url")}
      </label>
      <input
        id="ha-url"
        className={styles.input}
        type="url"
        inputMode="url"
        value={url}
        placeholder={t("ha.connect.urlPlaceholder")}
        onChange={(event) => setUrl(event.target.value)}
      />

      <span className={styles.label}>{t("ha.connect.prefer")}</span>
      <div className={styles.segmented} role="radiogroup" aria-label={t("ha.connect.prefer")}>
        <button
          type="button"
          role="radio"
          aria-checked={prefer === "peer"}
          data-active={prefer === "peer"}
          className={styles.segment}
          onClick={() => setPrefer("peer")}
        >
          {t("ha.connect.preferPeer")}
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={prefer === "local"}
          data-active={prefer === "local"}
          className={styles.segment}
          onClick={() => setPrefer("local")}
        >
          {t("ha.connect.preferLocal")}
        </button>
      </div>
      <p className={styles.hint}>{t("ha.connect.preferHint")}</p>

      {error && <p className={styles.fieldError}>{error}</p>}
      <button className={styles.buttonPrimary} type="submit" disabled={busy || url.trim().length === 0}>
        {t("ha.connect.action")}
      </button>
    </form>
  );
}

const HEALTH_KEY: Record<Health, "ha.health.ok" | "ha.health.stale" | "ha.health.error" | "ha.health.pending"> = {
  ok: "ha.health.ok",
  stale: "ha.health.stale",
  error: "ha.health.error",
  pending: "ha.health.pending",
};

/** The cluster view once meshed: this node plus every peer as a node list, a
 *  sync-now action, and a leave-cluster action. */
/** A small copy-to-clipboard button. Falls back silently when the clipboard is
 *  unavailable (e.g. an insecure context) — the value stays on screen to copy. */
function HaCopyButton({ value }: { value: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={styles.copyButton}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        } catch {
          // Clipboard blocked: the value is shown for manual copying.
        }
      }}
    >
      {copied ? t("ha.copied") : t("ha.copy")}
    </button>
  );
}

/** The guided "add another node" panel in the cluster view. Collapsed by default;
 *  when expanded it walks the admin through enabling HA on the new node, setting
 *  the same shared key (revealed and copyable here), and entering one seed address
 *  — the new node then meshes with the rest on its own. Hidden work, surfaced only
 *  when there is still room (up to four nodes). */
function AddNodeGuide({ status }: { status: HaStatus }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [revealed, setRevealed] = useState<string | null>(null);
  const [revealBusy, setRevealBusy] = useState(false);
  const [revealError, setRevealError] = useState(false);
  const nodeCount = status.peers.length + 1;
  const full = nodeCount >= MAX_NODES;

  async function reveal() {
    setRevealBusy(true);
    setRevealError(false);
    try {
      const { key } = await api.haRevealKey();
      if (key) setRevealed(key);
      else setRevealError(true);
    } catch {
      setRevealError(true);
    } finally {
      setRevealBusy(false);
    }
  }

  return (
    <div className={styles.addNode}>
      <button
        type="button"
        className={styles.addNodeToggle}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        <span>{t("ha.addNode.title")}</span>
        <span className={styles.addNodeCount}>
          {t("ha.addNode.count", { n: nodeCount, max: MAX_NODES })}
        </span>
      </button>

      {open && (
        <div className={styles.addNodePanel}>
          {full ? (
            <p className={styles.hint}>{t("ha.addNode.full")}</p>
          ) : (
            <ol className={styles.addNodeSteps}>
              <li>{t("ha.addNode.step1")}</li>
              <li>
                <div className={styles.addNodeStepText}>{t("ha.addNode.step2")}</div>
                <div className={styles.addNodeControl}>
                  {status.keyFromEnv ? (
                    <span className={styles.hint}>{t("ha.addNode.keyEnv")}</span>
                  ) : revealed ? (
                    <>
                      <code className={styles.addNodeValue}>{revealed}</code>
                      <HaCopyButton value={revealed} />
                      <button
                        type="button"
                        className={styles.linkButton}
                        onClick={() => setRevealed(null)}
                      >
                        {t("ha.addNode.hideKey")}
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className={styles.buttonSecondary}
                      onClick={reveal}
                      disabled={revealBusy || !status.keySet}
                    >
                      {t("ha.addNode.showKey")}
                    </button>
                  )}
                  {revealError && (
                    <span className={styles.fieldError}>{t("ha.addNode.keyError")}</span>
                  )}
                </div>
              </li>
              <li>
                <div className={styles.addNodeStepText}>{t("ha.addNode.step3")}</div>
                <div className={styles.addNodeControl}>
                  {status.selfUrl ? (
                    <>
                      <code className={styles.addNodeValue}>{status.selfUrl}</code>
                      <HaCopyButton value={status.selfUrl} />
                    </>
                  ) : (
                    <span className={styles.hint}>{t("ha.addNode.seedUnset")}</span>
                  )}
                </div>
              </li>
            </ol>
          )}
        </div>
      )}
    </div>
  );
}

function Cluster({
  status,
  lang,
  reload,
}: {
  status: HaStatus;
  lang: string;
  reload: () => Promise<void>;
}) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [pendingRemove, setPendingRemove] = useState<HaPeer | null>(null);
  const [synced, setSynced] = useState(false);

  // Cluster-wide health is the worst of the peers, so one node in trouble is not
  // hidden behind the others.
  const order: Health[] = ["error", "stale", "pending", "ok"];
  const worst = status.peers
    .map((peer) => peerHealth(peer, status.intervalSecs))
    .sort((a, b) => order.indexOf(a) - order.indexOf(b))[0];
  const clusterHealth: Health = worst ?? "pending";
  const nodeCount = status.peers.length + 1;

  async function syncNow() {
    setBusy(true);
    setSynced(false);
    try {
      await api.haSyncNow();
      setSynced(true);
      await reload();
    } finally {
      setBusy(false);
    }
  }

  async function confirmRemove() {
    const nodeId = pendingRemove?.peerNodeId;
    if (!nodeId) return;
    setBusy(true);
    try {
      await api.haDisconnect(nodeId);
      setPendingRemove(null);
      await reload();
    } finally {
      setBusy(false);
    }
  }

  async function leave() {
    setBusy(true);
    try {
      await api.haDisconnect();
      setLeaving(false);
      await reload();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={styles.summary}>
      <div className={styles.summaryHead}>
        <span className={styles.dot} data-health={clusterHealth} aria-hidden="true" />
        <div>
          <p className={styles.summaryState}>{t(HEALTH_KEY[clusterHealth])}</p>
          <p className={styles.summarySub}>{t("ha.clusterSize", { n: nodeCount })}</p>
        </div>
      </div>

      <ul className={styles.nodeList}>
        <SelfNode status={status} reload={reload} />
        {status.peers.map((peer) => (
          <PeerNode
            key={peer.peerNodeId ?? peer.url}
            peer={peer}
            health={peerHealth(peer, status.intervalSecs)}
            lang={lang}
            busy={busy}
            onRemove={setPendingRemove}
          />
        ))}
      </ul>

      <AddNodeGuide status={status} />

      {synced && <p className={styles.fieldOk}>{t("ha.synced")}</p>}

      <div className={styles.actions}>
        <button className={styles.buttonPrimary} type="button" onClick={syncNow} disabled={busy}>
          {t("ha.syncNow")}
        </button>
        <button className={styles.buttonDanger} type="button" onClick={() => setLeaving(true)} disabled={busy}>
          {t("ha.leave")}
        </button>
      </div>

      {leaving && (
        <ConfirmDialog
          title={t("ha.leave")}
          body={t("ha.leaveConfirm")}
          confirmLabel={t("ha.leave")}
          cancelLabel={t("ha.cancel")}
          busy={busy}
          onConfirm={leave}
          onCancel={() => setLeaving(false)}
        />
      )}

      {pendingRemove && (
        <ConfirmDialog
          title={t("ha.removeNodeTitle")}
          body={t("ha.removeNodeConfirm")}
          details={
            <div className={styles.removeDetails}>
              <div className={styles.removeIdentity}>
                <span className={styles.removeAddress}>{pendingRemove.url}</span>
                {pendingRemove.peerNodeId && (
                  <code className={styles.nodeIdCode}>{pendingRemove.peerNodeId}</code>
                )}
              </div>
              <ul className={styles.removeConsequences}>
                <li>{t("ha.removeNodeConseqSync")}</li>
                <li>{t("ha.removeNodeConseqNotify")}</li>
                <li>{t("ha.removeNodeConseqGhost")}</li>
              </ul>
            </div>
          }
          confirmLabel={t("ha.removeNodeAction")}
          cancelLabel={t("ha.cancel")}
          busy={busy}
          onConfirm={confirmRemove}
          onCancel={() => setPendingRemove(null)}
        />
      )}
    </div>
  );
}

/** This node's own card in the cluster list, with an editable advertised address. */
function SelfNode({ status, reload }: { status: HaStatus; reload: () => Promise<void> }) {
  const { t } = useI18n();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(status.selfUrl ?? "");
  const [busy, setBusy] = useState(false);

  async function save(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    try {
      await api.haSetAddress(value.trim());
      setEditing(false);
      await reload();
    } finally {
      setBusy(false);
    }
  }

  return (
    <li className={`${styles.nodeCard} ${styles.nodeCardSelf}`}>
      <div className={styles.nodeHead}>
        <span className={styles.nodeBadge}>{t("ha.thisNode")}</span>
        <code className={styles.nodeIdCode}>{status.nodeId}</code>
      </div>
      {editing ? (
        <form className={styles.inlineEdit} onSubmit={save}>
          <input
            className={styles.input}
            type="url"
            inputMode="url"
            value={value}
            aria-label={t("ha.address.label")}
            placeholder={t("ha.address.placeholder")}
            onChange={(event) => setValue(event.target.value)}
          />
          <button className={styles.copyButton} type="submit" disabled={busy}>
            {t("ha.address.save")}
          </button>
        </form>
      ) : (
        <dl className={styles.nodeMeta}>
          <div>
            <dt>{t("ha.selfAddress")}</dt>
            <dd>
              {status.selfUrl ?? t("ha.selfAddressUnset")}{" "}
              <button type="button" className={styles.linkButton} onClick={() => setEditing(true)}>
                {t("ha.editAddress")}
              </button>
            </dd>
          </div>
          <div>
            <dt>{t("ha.peerVersion")}</dt>
            <dd>{status.version}</dd>
          </div>
          <div>
            <dt>{t("ha.tracked", { n: status.tracked })}</dt>
            <dd>{status.tracked}</dd>
          </div>
        </dl>
      )}
    </li>
  );
}

/** One peer's card: a health dot, its identity and address, last sync, any error,
 *  and a remove action. */
function PeerNode({
  peer,
  health,
  lang,
  busy,
  onRemove,
}: {
  peer: HaPeer;
  health: Health;
  lang: string;
  busy: boolean;
  onRemove: (peer: HaPeer) => void;
}) {
  const { t } = useI18n();
  const formatTime = (ms?: number) =>
    ms == null ? t("ha.lastSyncNever") : new Date(ms).toLocaleString(lang);

  return (
    <li className={styles.nodeCard}>
      <div className={styles.nodeHead}>
        <span className={styles.dot} data-health={health} aria-hidden="true" />
        <span className={styles.nodeState}>{t(HEALTH_KEY[health])}</span>
        {peer.peerNodeId && <code className={styles.nodeIdCode}>{peer.peerNodeId}</code>}
        {peer.peerNodeId && (
          <button
            type="button"
            className={styles.removeButton}
            onClick={() => onRemove(peer)}
            disabled={busy}
            aria-label={t("ha.removeNode")}
            title={t("ha.removeNode")}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        )}
      </div>
      <dl className={styles.nodeMeta}>
        <div>
          <dt>{t("ha.peerUrl")}</dt>
          <dd>{peer.url}</dd>
        </div>
        {peer.peerVersion && (
          <div>
            <dt>{t("ha.peerVersion")}</dt>
            <dd>{peer.peerVersion}</dd>
          </div>
        )}
        <div>
          <dt>{t("ha.lastSync")}</dt>
          <dd>{formatTime(peer.lastSyncMs)}</dd>
        </div>
      </dl>
      {peer.lastError && <p className={styles.fieldError}>{t("ha.lastError")}: {peer.lastError}</p>}
    </li>
  );
}
