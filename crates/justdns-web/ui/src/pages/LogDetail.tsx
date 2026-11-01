import { useEffect, useState } from "react";

import { api, type ClassifyResult } from "../lib/api";
import { blockListLabel } from "../lib/dns";
import type { LogRow } from "../lib/useLogs";
import styles from "./Logs.module.css";

type ActionState = "idle" | "pending" | "done" | "error";

/** Put the exact queried domain on the allow list (for a blocked entry) or the
 *  user's own block list (for one that got through). The whole point is the
 *  one-click fix straight from the log, on the exact name that was asked. */
function ListAction({ name, blocked }: { name: string; blocked: boolean }) {
  const [state, setState] = useState<ActionState>("idle");

  async function act() {
    setState("pending");
    try {
      if (blocked) await api.addAllow(name);
      else await api.addBlock(name);
      setState("done");
    } catch {
      setState("error");
    }
  }

  const doneText = blocked
    ? "Freigegeben — diese Domain wird nicht mehr geblockt."
    : "Zur eigenen Blockliste hinzugefügt.";

  return (
    <div className={styles.actions}>
      <p className={styles.actionNote}>
        {blocked
          ? "Fälschlich geblockt? Nimm genau diese Domain dauerhaft von der Sperre aus."
          : "Diese Domain stört? Sperre genau sie für das ganze Netzwerk."}
      </p>
      {state === "done" ? (
        <span className={styles.actionDone} role="status">
          {doneText}
        </span>
      ) : (
        <div className={styles.actionRow}>
          <button
            type="button"
            className={styles.actionButton}
            data-variant={blocked ? "allow" : "block"}
            disabled={state === "pending"}
            onClick={act}
          >
            {state === "pending"
              ? "Wird gespeichert …"
              : blocked
                ? "Auf Freigabeliste setzen"
                : "Zur Blockliste hinzufügen"}
          </button>
          {state === "error" ? (
            <span className={styles.actionError} role="alert">
              Konnte nicht gespeichert werden.
            </span>
          ) : null}
        </div>
      )}
    </div>
  );
}

/** One labelled field in the detail grid. */
function Field({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className={styles.field}>
      <dt className={styles.fieldLabel}>{label}</dt>
      <dd className={styles.fieldValue}>
        {value}
        {hint ? <span className={styles.fieldHint}>{hint}</span> : null}
      </dd>
    </div>
  );
}

/** Ask the server which list blocked `name` and why. Kept separate so the
 *  request only fires when a blocked row is actually opened. */
function BlockReason({ name }: { name: string }) {
  const [result, setResult] = useState<ClassifyResult | null | undefined>(undefined);

  useEffect(() => {
    const controller = new AbortController();
    api
      .classify(name, controller.signal)
      .then((r) => setResult(r))
      .catch(() => setResult(null));
    return () => controller.abort();
  }, [name]);

  if (result === undefined) {
    return <p className={styles.reasonLoading}>Grund wird ermittelt …</p>;
  }
  if (result === null) {
    // Logged as blocked, but on no current list (e.g. the list changed since).
    return <p className={styles.reasonLoading}>Auf keiner aktuellen Liste gefunden.</p>;
  }
  return (
    <dl className={styles.reasonGrid}>
      <Field label="Liste" value={blockListLabel(result.list)} />
      <Field label="Regel" value={result.rule} hint="hat den Treffer ausgelöst" />
    </dl>
  );
}

/** The expanded detail for one log entry: the full record, with the block
 *  status — the thing worth seeing at a glance — called out at the top. */
export function LogDetail({ row }: { row: LogRow }) {
  return (
    <div className={styles.detail}>
      <div
        className={styles.verdict}
        data-blocked={row.isBlocked || undefined}
        role="status"
      >
        <span className={styles.verdictMark} aria-hidden="true">
          {row.isBlocked ? "✕" : "✓"}
        </span>
        <span className={styles.verdictText}>
          {row.isBlocked ? "Geblockt" : "Nicht geblockt"}
        </span>
      </div>

      {row.isBlocked ? <BlockReason name={row.name} /> : null}

      <dl className={styles.detailGrid}>
        <Field label="Domain" value={row.name} />
        <Field label="Client" value={row.client} />
        <Field label="Typ" value={row.type} hint={row.typeDesc} />
        <Field label="Antwort" value={row.rcode} hint={row.rcodeDesc} />
        <Field label="Quelle" value={row.sourceText} />
        <Field label="Antworten" value={String(row.answers)} />
        <Field label="Dauer" value={`${row.durationMs} ms`} />
        <Field label="Zeit" value={row.fullTime} />
      </dl>

      <ListAction name={row.name} blocked={row.isBlocked} />
    </div>
  );
}
