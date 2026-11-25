import { useEffect, useState } from "react";
import { Link } from "react-router-dom";

import { api, type ClassifyResult, type QueryTrace } from "../lib/api";
import { blockListLabel } from "../lib/dns";
import { useI18n } from "../lib/i18n";
import { OUTCOME_KEY, STAGE_KEY, STAGE_LINK } from "../lib/trace";
import type { LogRow } from "../lib/useLogs";
import styles from "./Logs.module.css";

/** The glyph that leads a trace step, by outcome: a block hit, a warning, a
 *  passed gate, or a neutral note. */
const OUTCOME_MARK: Record<QueryTrace["steps"][number]["outcome"], string> = {
  hit: "✕",
  warn: "!",
  pass: "✓",
  info: "·",
};

/** The decision trace for one entry: the ordered stages the resolver ran, read
 *  from the durable log on demand. The decisive stage (a block hit or a warn)
 *  and the neutral notes (source, answer) show at once; the gates that merely
 *  passed fold behind a toggle, so a plain answer stays quiet. */
function TraceView({ row }: { row: LogRow }) {
  const { t } = useI18n();
  const [trace, setTrace] = useState<QueryTrace | null | undefined>(undefined);
  const [showPassed, setShowPassed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    api
      .queryTrace(
        {
          at_ms: row.at_ms,
          client: row.client,
          name: row.rawName,
          record_type: row.recordTypeNum,
        },
        controller.signal,
      )
      .then((result) => setTrace(result))
      .catch(() => setTrace(null));
    return () => controller.abort();
  }, [row.at_ms, row.client, row.rawName, row.recordTypeNum]);

  if (trace === undefined) {
    return <p className={styles.reasonLoading}>{t("trace.loading")}</p>;
  }
  if (trace === null || trace.steps.length === 0) {
    return <p className={styles.reasonLoading}>{t("trace.none")}</p>;
  }

  const passed = trace.steps.filter((step) => step.outcome === "pass");
  const visible = showPassed
    ? trace.steps
    : trace.steps.filter((step) => step.outcome !== "pass");

  return (
    <div className={styles.trace}>
      <h3 className={styles.traceTitle}>{t("trace.title")}</h3>
      {trace.paused ? (
        <p className={styles.tracePause} role="status">
          {t("trace.paused")}
        </p>
      ) : null}
      <ol className={styles.traceSteps}>
        {visible.map((step, index) => {
          const decisive = step.outcome === "hit" || step.outcome === "warn";
          const link = decisive ? STAGE_LINK[step.stage] : undefined;
          return (
            <li key={index} className={styles.traceStep} data-outcome={step.outcome}>
              <span className={styles.traceMark} aria-hidden="true">
                {OUTCOME_MARK[step.outcome]}
              </span>
              <span className={styles.traceStage}>{t(STAGE_KEY[step.stage])}</span>
              {step.detail ? <span className={styles.traceDetail}>{step.detail}</span> : null}
              {link ? (
                <Link className={styles.traceLink} to={link.to}>
                  {t(link.label)}
                </Link>
              ) : (
                <span className={styles.traceOutcome}>{t(OUTCOME_KEY[step.outcome])}</span>
              )}
            </li>
          );
        })}
      </ol>
      {passed.length > 0 ? (
        <button
          type="button"
          className={styles.traceToggle}
          onClick={() => setShowPassed((value) => !value)}
        >
          {showPassed ? t("trace.hidePassed") : t("trace.showPassed")}
        </button>
      ) : null}
      {row.origin ? (
        <p className={styles.traceNode}>
          {t("trace.node")}: {row.origin.slice(0, 8)}
        </p>
      ) : null}
    </div>
  );
}

type ActionState = "idle" | "pending" | "done" | "error";

/** Put the exact queried domain on the allow list (for a blocked entry) or the
 *  user's own block list (for one that got through). The whole point is the
 *  one-click fix straight from the log, on the exact name that was asked. */
function ListAction({ name, blocked }: { name: string; blocked: boolean }) {
  const { t } = useI18n();
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

  const doneText = blocked ? t("logDetail.allowDone") : t("logDetail.blockDone");

  return (
    <div className={styles.actions}>
      <p className={styles.actionNote}>
        {blocked ? t("logDetail.blockedActionNote") : t("logDetail.allowActionNote")}
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
              ? t("logDetail.saving")
              : blocked
                ? t("logDetail.setAllow")
                : t("logDetail.addBlock")}
          </button>
          {state === "error" ? (
            <span className={styles.actionError} role="alert">
              {t("logDetail.saveError")}
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
  const { t, lang } = useI18n();
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
    return <p className={styles.reasonLoading}>{t("logDetail.reasonLoading")}</p>;
  }
  if (result === null) {
    // Logged as blocked, but on no current list (e.g. the list changed since).
    return <p className={styles.reasonLoading}>{t("logDetail.notOnList")}</p>;
  }
  return (
    <dl className={styles.reasonGrid}>
      <Field label={t("logDetail.list")} value={result.name ?? blockListLabel(result.list, lang)} />
      <Field label={t("logDetail.rule")} value={result.rule} hint={t("logDetail.ruleHint")} />
    </dl>
  );
}

/** The expanded detail for one log entry: the full record, with the block
 *  status — the thing worth seeing at a glance — called out at the top. */
export function LogDetail({ row }: { row: LogRow }) {
  const { t } = useI18n();
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
          {row.isBlocked ? t("logDetail.blocked") : t("logDetail.notBlocked")}
        </span>
      </div>

      {row.isBlocked ? <BlockReason name={row.name} /> : null}

      <dl className={styles.detailGrid}>
        <Field label={t("logDetail.domain")} value={row.name} />
        <Field label={t("logDetail.client")} value={row.client} />
        <Field label={t("logDetail.type")} value={row.type} hint={row.typeDesc} />
        <Field label={t("logDetail.response")} value={row.rcode} hint={row.rcodeDesc} />
        <Field label={t("logDetail.source")} value={row.sourceText} />
        {row.phishing
          ? (() => {
              const kindLabel =
                row.phishing.kind === "lookalike"
                  ? t("phishing.kindLookalike")
                  : row.phishing.kind === "punycode"
                    ? t("phishing.kindPunycode")
                    : t("phishing.kindNrd");
              const verdict = row.phishing.blocked
                ? t("phishing.badgeBlock")
                : t("phishing.badgeWarn");
              const kind = row.phishing.brand ? `${kindLabel} · ${row.phishing.brand}` : kindLabel;
              return (
                <Field
                  label={t("phishing.colVerdict")}
                  value={`${kind} — ${verdict}`}
                  hint={t("phishing.title")}
                />
              );
            })()
          : null}
        <Field label={t("logDetail.answers")} value={String(row.answers)} />
        {row.addresses.length > 0 ? (
          <Field label={t("logDetail.resolvedTo")} value={row.addresses.join(", ")} />
        ) : null}
        <Field label={t("logDetail.duration")} value={`${row.durationMs} ms`} />
        <Field label={t("logDetail.time")} value={row.fullTime} />
      </dl>

      <TraceView row={row} />

      <ListAction name={row.name} blocked={row.isBlocked} />
    </div>
  );
}
