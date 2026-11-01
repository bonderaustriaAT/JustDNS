import { useEffect, useRef, useState, type FormEvent } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import { ApiError, api, type Rewrite, type RewritesView } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Rewrites.module.css";

/** Pick the message key for a mutation's failure, shown in the banner. */
function errorKey(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.status === 400) return "rewrites.badInput";
    if (error.status === 404) return "common.notFound";
  }
  return "common.saveFailed";
}

/** Split a free-text field into separate tokens: entries are separated by commas
 *  or whitespace. Used for both the IP field ("192.168.20.5, …") and the name
 *  field, where several names can share one address. Empty fragments are
 *  dropped. */
function splitTokens(raw: string): string[] {
  return raw
    .split(/[\s,]+/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

export function Rewrites() {
  const { t } = useI18n();
  const [view, setView] = useState<RewritesView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());

  // The top form only ever adds; editing happens inline in the row.
  const [name, setName] = useState("");
  const [ips, setIps] = useState("");
  // The name of the row being edited inline, or null when none is.
  const [editing, setEditing] = useState<string | null>(null);
  // The name of the row awaiting a delete confirmation, or null.
  const [confirming, setConfirming] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    api
      .rewrites(controller.signal)
      .then(setView)
      .catch(() => {
        if (controller.signal.aborted) return;
        setError(t("rewrites.loadFailed"));
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

  async function run(key: string, action: () => Promise<RewritesView>): Promise<boolean> {
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

  function submitAdd(event: FormEvent) {
    event.preventDefault();
    const addresses = splitTokens(ips);
    if (addresses.length === 0) return;
    const names = splitTokens(name);
    if (names.length === 0) return;
    run("add", () => api.setRewrites(names, addresses)).then((ok) => {
      if (ok) {
        setName("");
        setIps("");
      }
    });
  }

  function saveEdit(original: string, newName: string, addresses: string[]) {
    run(`edit:${original}`, () => api.editRewrite(original, newName, addresses)).then((ok) => {
      if (ok) setEditing(null);
    });
  }

  function confirmRemove() {
    const target = confirming;
    if (!target) return;
    run(`rm:${target}`, () => api.removeRewrite(target)).then((ok) => {
      if (ok && editing === target) setEditing(null);
      setConfirming(null);
    });
  }

  if (!view) {
    return (
      <div className={styles.page}>
        <h1 className={styles.title}>{t("nav.rewrites")}</h1>
        <p className={styles.lead}>{error ?? t("common.loading")}</p>
      </div>
    );
  }

  const busy = pending.size > 0;

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <div>
          <h1 className={styles.title}>
            {t("nav.rewrites")}
            <InfoHint label={t("nav.rewrites")} text={t("rewrites.hint")} />
          </h1>
          <p className={styles.lead}>{t("rewrites.lead")}</p>
        </div>
      </div>

      {error ? (
        <p className={styles.banner} role="alert">
          {error}
        </p>
      ) : null}

      <form className={styles.addForm} onSubmit={submitAdd}>
        <input
          className={styles.input}
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder={t("rewrites.namePlaceholderAdd")}
          aria-label={t("rewrites.nameAriaAdd")}
          autoComplete="off"
          spellCheck={false}
        />
        <input
          className={styles.input}
          value={ips}
          onChange={(event) => setIps(event.target.value)}
          placeholder={t("rewrites.ipPlaceholder")}
          aria-label={t("rewrites.ipAria")}
          autoComplete="off"
          spellCheck={false}
        />
        <button type="submit" className={styles.addButton} disabled={busy}>
          {t("rewrites.save")}
        </button>
      </form>
      <p className={styles.hint}>{t("rewrites.multiHint")}</p>

      {view.rewrites.length === 0 ? (
        <p className={styles.empty}>{t("rewrites.empty")}</p>
      ) : (
        <ul className={styles.entries}>
          {view.rewrites.map((rewrite) => (
            <RewriteRow
              key={rewrite.name}
              rewrite={rewrite}
              editing={editing === rewrite.name}
              busy={
                pending.has(`edit:${rewrite.name}`) ||
                pending.has(`en:${rewrite.name}`) ||
                pending.has(`rm:${rewrite.name}`)
              }
              onStartEdit={() => {
                setError(null);
                setEditing(rewrite.name);
              }}
              onCancelEdit={() => setEditing(null)}
              onSaveEdit={(newName, addresses) => saveEdit(rewrite.name, newName, addresses)}
              onToggle={(enabled) =>
                run(`en:${rewrite.name}`, () => api.setRewriteEnabled(rewrite.name, enabled))
              }
              onRemove={() => setConfirming(rewrite.name)}
            />
          ))}
        </ul>
      )}

      {confirming ? (
        <ConfirmDialog
          title={t("rewrites.removeConfirmTitle")}
          body={t("rewrites.removeConfirmBody", { name: confirming })}
          confirmLabel={t("rewrites.removeConfirm")}
          cancelLabel={t("rewrites.cancel")}
          busy={pending.has(`rm:${confirming}`)}
          onConfirm={confirmRemove}
          onCancel={() => setConfirming(null)}
        />
      ) : null}
    </div>
  );
}

/** One rewrite. Normally a row with an on/off toggle, the name, its addresses as
 *  chips, and edit/remove controls. In edit mode the name and addresses turn
 *  into inputs right here in the row — no jumping back up to the top form. */
function RewriteRow({
  rewrite,
  editing,
  busy,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onToggle,
  onRemove,
}: {
  rewrite: Rewrite;
  editing: boolean;
  busy: boolean;
  onStartEdit: () => void;
  onCancelEdit: () => void;
  onSaveEdit: (name: string, ips: string[]) => void;
  onToggle: (enabled: boolean) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  return (
    <li
      className={styles.entry}
      data-editing={editing || undefined}
      data-off={!rewrite.enabled || undefined}
    >
      <div className={styles.entryToggle}>
        <Toggle
          label={t(rewrite.enabled ? "rewrites.disableAria" : "rewrites.enableAria", {
            name: rewrite.name,
          })}
          checked={rewrite.enabled}
          disabled={busy || editing}
          onChange={onToggle}
        />
      </div>

      {editing ? (
        <RewriteEditor
          rewrite={rewrite}
          busy={busy}
          onSave={onSaveEdit}
          onCancel={onCancelEdit}
        />
      ) : (
        <>
          <span className={styles.entryName} title={rewrite.name}>
            {rewrite.name}
          </span>
          <div className={styles.entryValue}>
            <span className={styles.arrow} aria-hidden="true">
              →
            </span>
            <span className={styles.ips}>
              {rewrite.ips.map((ip) => (
                <span key={ip} className={styles.ip}>
                  {ip}
                </span>
              ))}
            </span>
          </div>
          <div className={styles.entryActions}>
            <button
              type="button"
              className={styles.action}
              aria-label={t("rewrites.editAria", { name: rewrite.name })}
              title={t("rewrites.editTitle")}
              disabled={busy}
              onClick={onStartEdit}
            >
              {t("rewrites.editTitle")}
            </button>
            <button
              type="button"
              className={styles.remove}
              aria-label={t("rewrites.removeAria", { name: rewrite.name })}
              title={t("rewrites.removeTitle")}
              disabled={busy}
              onClick={onRemove}
            >
              ✕
            </button>
          </div>
        </>
      )}
    </li>
  );
}

/** The inline editor shown in place of a row's name/addresses while it is being
 *  edited. It keeps its own draft, seeded from the rewrite when it mounts, and
 *  reports the new name and addresses on save. */
function RewriteEditor({
  rewrite,
  busy,
  onSave,
  onCancel,
}: {
  rewrite: Rewrite;
  busy: boolean;
  onSave: (name: string, ips: string[]) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [draftName, setDraftName] = useState(rewrite.name);
  const [draftIps, setDraftIps] = useState(rewrite.ips.join(", "));
  const nameRef = useRef<HTMLInputElement>(null);

  // Put the cursor in the name field as editing opens.
  useEffect(() => {
    nameRef.current?.focus();
    nameRef.current?.select();
  }, []);

  function submit(event: FormEvent) {
    event.preventDefault();
    const addresses = splitTokens(draftIps);
    const target = draftName.trim();
    if (!target || addresses.length === 0) return;
    onSave(target, addresses);
  }

  return (
    <form className={styles.rowEdit} onSubmit={submit}>
      <input
        ref={nameRef}
        className={styles.rowEditInput}
        value={draftName}
        onChange={(event) => setDraftName(event.target.value)}
        aria-label={t("rewrites.nameAriaEdit")}
        autoComplete="off"
        spellCheck={false}
      />
      <span className={styles.arrow} aria-hidden="true">
        →
      </span>
      <input
        className={styles.rowEditInput}
        value={draftIps}
        onChange={(event) => setDraftIps(event.target.value)}
        aria-label={t("rewrites.ipAria")}
        autoComplete="off"
        spellCheck={false}
      />
      <button type="submit" className={styles.rowSave} disabled={busy}>
        {t("rewrites.update")}
      </button>
      <button
        type="button"
        className={styles.rowCancel}
        onClick={onCancel}
        disabled={busy}
      >
        {t("rewrites.cancel")}
      </button>
    </form>
  );
}
