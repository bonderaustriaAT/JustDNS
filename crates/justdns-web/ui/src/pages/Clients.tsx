import { useEffect, useRef, useState, type FormEvent } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import {
  ApiError,
  api,
  type ClientRow,
  type ClientsView,
  type DeviceKind,
} from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Clients.module.css";

/** The device kinds, in the order the picker offers them. "other" is last as the
 *  fallback. */
const KINDS: DeviceKind[] = [
  "phone",
  "laptop",
  "desktop",
  "tablet",
  "tv",
  "console",
  "iot",
  "server",
  "printer",
  "router",
  "other",
];

/** A small glyph per device kind, so a row reads at a glance without an icon
 *  dependency. Kept deliberately plain. */
const KIND_GLYPH: Record<DeviceKind, string> = {
  phone: "▯",
  laptop: "▭",
  desktop: "◻",
  tablet: "▢",
  tv: "⬛",
  console: "◉",
  iot: "◈",
  server: "▤",
  printer: "⎙",
  router: "⇄",
  other: "•",
};

function errorKey(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.status === 400) return "clients.badInput";
    if (error.status === 404) return "common.notFound";
  }
  return "common.saveFailed";
}

/** A short "seen 2 min ago" phrase from a last-seen timestamp, in the active
 *  language. Mirrors the overview's relative-time wording. */
function lastSeen(ms: number | undefined, t: ReturnType<typeof useI18n>["t"]): string {
  if (ms == null) return t("clients.neverSeen");
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 5) return t("time.justNow");
  if (seconds < 60) return t("time.secondsAgo", { n: seconds });
  const minutes = Math.round(seconds / 60);
  if (minutes === 1) return t("time.oneMinuteAgo");
  if (minutes < 60) return t("time.minutesAgo", { n: minutes });
  const hours = Math.round(minutes / 60);
  return hours === 1 ? t("time.oneHourAgo") : t("time.hoursAgo", { n: hours });
}

export function Clients() {
  const { t, locale } = useI18n();
  const nf = useRef(new Intl.NumberFormat(locale));
  nf.current = new Intl.NumberFormat(locale);

  const [view, setView] = useState<ClientsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());

  // The add/name form at the top.
  const [target, setTarget] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<DeviceKind>("other");
  const [note, setNote] = useState("");
  const nameRef = useRef<HTMLInputElement>(null);

  const [editing, setEditing] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<ClientRow | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    api
      .clients(controller.signal)
      .then(setView)
      .catch(() => {
        if (controller.signal.aborted) return;
        setError(t("clients.loadFailed"));
      });
    return () => controller.abort();
  }, [t]);

  function mark(key: string, on: boolean) {
    setPending((prev) => {
      const next = new Set(prev);
      if (on) next.add(key);
      else next.delete(key);
      return next;
    });
  }

  async function run(key: string, action: () => Promise<ClientsView>): Promise<boolean> {
    mark(key, true);
    setError(null);
    try {
      setView(await action());
      return true;
    } catch (err) {
      setError(t(errorKey(err)));
      return false;
    } finally {
      mark(key, false);
    }
  }

  function resetForm() {
    setTarget("");
    setName("");
    setKind("other");
    setNote("");
  }

  function submitAdd(event: FormEvent) {
    event.preventDefault();
    if (target.trim() === "" || name.trim() === "") return;
    run("add", () => api.setClient(target.trim(), name.trim(), kind, note.trim() || undefined)).then(
      (ok) => {
        if (ok) resetForm();
      },
    );
  }

  /** Pre-fill the top form from an unnamed client's address and focus the name
   *  field, so naming a seen device is one click plus a word. */
  function startNaming(row: ClientRow) {
    setTarget(row.target);
    setName("");
    setKind("other");
    setNote("");
    nameRef.current?.focus();
    nameRef.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }

  const activeMinutes = view ? Math.max(1, Math.round(view.activeWindowSecs / 60)) : 5;

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>
          {t("clients.title")}
          <InfoHint label={t("clients.title")} text={t("clients.hint")} />
        </h1>
        <p className={styles.lead}>{t("clients.lead")}</p>
      </header>

      {error && <p className={styles.banner}>{error}</p>}

      <form className={styles.addForm} onSubmit={submitAdd}>
        <div className={styles.addRow}>
          <input
            className={styles.input}
            value={target}
            onChange={(event) => setTarget(event.target.value)}
            placeholder={t("clients.targetPlaceholderAdd")}
            aria-label={t("clients.targetAria")}
            autoComplete="off"
            spellCheck={false}
          />
          <input
            ref={nameRef}
            className={styles.input}
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t("clients.namePlaceholder")}
            aria-label={t("clients.nameAria")}
            autoComplete="off"
          />
          <select
            className={styles.select}
            value={kind}
            onChange={(event) => setKind(event.target.value as DeviceKind)}
            aria-label={t("clients.kindAria")}
          >
            {KINDS.map((k) => (
              <option key={k} value={k}>
                {t(`clients.kind.${k}` as MessageKey)}
              </option>
            ))}
          </select>
          <button className={styles.addButton} type="submit" disabled={pending.has("add")}>
            {t("clients.add")}
          </button>
        </div>
        <input
          className={styles.input}
          value={note}
          onChange={(event) => setNote(event.target.value)}
          placeholder={t("clients.notePlaceholder")}
          aria-label={t("clients.noteAria")}
          autoComplete="off"
        />
      </form>

      <p className={styles.windowHint}>
        {t("clients.activeWindowHint", { minutes: activeMinutes })}
      </p>

      {view === null ? (
        <p className={styles.empty}>{t("clients.loadFailed")}</p>
      ) : view.clients.length === 0 ? (
        <p className={styles.empty}>{t("clients.empty")}</p>
      ) : (
        <ul className={styles.entries}>
          {view.clients.map((row) => {
            const key = row.target;
            const isEditing = editing === key;
            return (
              <li key={key} className={styles.entry} data-editing={isEditing || undefined}>
                {isEditing ? (
                  <ClientEditor
                    row={row}
                    busy={pending.has(`edit:${key}`)}
                    onCancel={() => setEditing(null)}
                    onSave={(nextTarget, nextName, nextKind, nextNote) =>
                      run(`edit:${key}`, () =>
                        api.editClient(key, nextTarget, nextName, nextKind, nextNote),
                      ).then((ok) => {
                        if (ok) setEditing(null);
                      })
                    }
                  />
                ) : (
                  <>
                    <span
                      className={styles.dot}
                      data-on={row.active || undefined}
                      title={row.active ? t("clients.active") : undefined}
                      aria-hidden="true"
                    />
                    <span className={styles.glyph} aria-hidden="true">
                      {KIND_GLYPH[row.kind]}
                    </span>
                    <span className={styles.identity}>
                      {row.known ? (
                        <span className={styles.name}>{row.name}</span>
                      ) : (
                        <span className={styles.unnamed}>{t("clients.unnamed")}</span>
                      )}
                      <span className={styles.target}>{row.target}</span>
                      {row.note ? <span className={styles.note}>{row.note}</span> : null}
                    </span>
                    <span className={styles.activity}>
                      <span className={styles.seen}>{lastSeen(row.lastSeenMs, t)}</span>
                      <span className={styles.count}>
                        {t("clients.queries", { n: nf.current.format(row.count) })}
                      </span>
                    </span>
                    <span className={styles.actions}>
                      {row.known ? (
                        <>
                          <button
                            type="button"
                            className={styles.action}
                            onClick={() => setEditing(key)}
                            title={t("clients.editTitle")}
                            aria-label={t("clients.editAria", { name: row.name ?? row.target })}
                          >
                            {t("clients.editTitle")}
                          </button>
                          <button
                            type="button"
                            className={styles.action}
                            onClick={() => setConfirming(row)}
                            title={t("clients.removeTitle")}
                            aria-label={t("clients.removeAria", { name: row.name ?? row.target })}
                          >
                            {t("clients.removeTitle")}
                          </button>
                        </>
                      ) : (
                        <button
                          type="button"
                          className={styles.nameButton}
                          onClick={() => startNaming(row)}
                        >
                          {t("clients.nameThis")}
                        </button>
                      )}
                    </span>
                  </>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {confirming && (
        <ConfirmDialog
          title={t("clients.removeConfirmTitle")}
          body={t("clients.removeConfirmBody", { name: confirming.name ?? confirming.target })}
          confirmLabel={t("clients.removeConfirm")}
          cancelLabel={t("clients.cancel")}
          busy={pending.has(`remove:${confirming.target}`)}
          onConfirm={() =>
            run(`remove:${confirming.target}`, () => api.removeClient(confirming.target)).then(
              (ok) => {
                if (ok) setConfirming(null);
              },
            )
          }
          onCancel={() => setConfirming(null)}
        />
      )}
    </div>
  );
}

/** The inline editor shown in place of a known device's row. */
function ClientEditor({
  row,
  busy,
  onSave,
  onCancel,
}: {
  row: ClientRow;
  busy: boolean;
  onSave: (target: string, name: string, kind: DeviceKind, note: string | undefined) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [target, setTarget] = useState(row.target);
  const [name, setName] = useState(row.name ?? "");
  const [kind, setKind] = useState<DeviceKind>(row.kind);
  const [note, setNote] = useState(row.note ?? "");

  function submit(event: FormEvent) {
    event.preventDefault();
    if (target.trim() === "" || name.trim() === "") return;
    onSave(target.trim(), name.trim(), kind, note.trim() || undefined);
  }

  return (
    <form className={styles.rowEdit} onSubmit={submit}>
      <div className={styles.rowEditInputs}>
        <input
          className={styles.rowEditInput}
          value={target}
          onChange={(event) => setTarget(event.target.value)}
          aria-label={t("clients.targetAria")}
          autoComplete="off"
          spellCheck={false}
        />
        <input
          className={styles.rowEditInput}
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-label={t("clients.nameAria")}
          autoComplete="off"
          autoFocus
        />
        <select
          className={styles.select}
          value={kind}
          onChange={(event) => setKind(event.target.value as DeviceKind)}
          aria-label={t("clients.kindAria")}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`clients.kind.${k}` as MessageKey)}
            </option>
          ))}
        </select>
        <button className={styles.rowSave} type="submit" disabled={busy}>
          {t("clients.update")}
        </button>
        <button className={styles.rowCancel} type="button" onClick={onCancel} disabled={busy}>
          {t("clients.cancel")}
        </button>
      </div>
      <input
        className={styles.rowEditInput}
        value={note}
        onChange={(event) => setNote(event.target.value)}
        placeholder={t("clients.notePlaceholder")}
        aria-label={t("clients.noteAria")}
        autoComplete="off"
      />
    </form>
  );
}
