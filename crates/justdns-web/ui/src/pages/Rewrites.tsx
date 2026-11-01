import { useEffect, useState, type FormEvent } from "react";

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

  const [name, setName] = useState("");
  const [ips, setIps] = useState("");
  // The original name of the entry being edited, or null when adding.
  const [editing, setEditing] = useState<string | null>(null);

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

  function resetForm() {
    setName("");
    setIps("");
    setEditing(null);
  }

  function startEdit(rewrite: Rewrite) {
    setEditing(rewrite.name);
    setName(rewrite.name);
    setIps(rewrite.ips.join(", "));
    setError(null);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const addresses = splitTokens(ips);
    if (addresses.length === 0) return;

    if (editing) {
      const target = name.trim();
      if (!target) return;
      run(`edit:${editing}`, () => api.editRewrite(editing, target, addresses)).then((ok) => {
        if (ok) resetForm();
      });
    } else {
      const names = splitTokens(name);
      if (names.length === 0) return;
      run("add", () => api.setRewrites(names, addresses)).then((ok) => {
        if (ok) resetForm();
      });
    }
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

      <form className={styles.addForm} onSubmit={submit}>
        <input
          className={styles.input}
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder={editing ? t("rewrites.namePlaceholderEdit") : t("rewrites.namePlaceholderAdd")}
          aria-label={editing ? t("rewrites.nameAriaEdit") : t("rewrites.nameAriaAdd")}
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
          {editing ? t("rewrites.update") : t("rewrites.save")}
        </button>
        {editing ? (
          <button
            type="button"
            className={styles.cancelButton}
            onClick={resetForm}
            disabled={busy}
          >
            {t("rewrites.cancel")}
          </button>
        ) : null}
      </form>
      {!editing ? <p className={styles.hint}>{t("rewrites.multiHint")}</p> : null}

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
              onEdit={() => startEdit(rewrite)}
              onToggle={(enabled) =>
                run(`en:${rewrite.name}`, () => api.setRewriteEnabled(rewrite.name, enabled))
              }
              onRemove={() => {
                run(`rm:${rewrite.name}`, () => api.removeRewrite(rewrite.name)).then((ok) => {
                  if (ok && editing === rewrite.name) resetForm();
                });
              }}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

/** One rewrite: an on/off toggle, the name, its addresses as chips, and edit and
 *  remove controls. A disabled rewrite is dimmed to show it is not answered. */
function RewriteRow({
  rewrite,
  editing,
  busy,
  onEdit,
  onToggle,
  onRemove,
}: {
  rewrite: Rewrite;
  editing: boolean;
  busy: boolean;
  onEdit: () => void;
  onToggle: (enabled: boolean) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  return (
    <li className={styles.entry} data-editing={editing || undefined} data-off={!rewrite.enabled || undefined}>
      <div className={styles.entryToggle}>
        <Toggle
          label={t(rewrite.enabled ? "rewrites.disableAria" : "rewrites.enableAria", {
            name: rewrite.name,
          })}
          checked={rewrite.enabled}
          disabled={busy}
          onChange={onToggle}
        />
      </div>
      <div className={styles.entryMain}>
        <span className={styles.entryName}>{rewrite.name}</span>
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
          onClick={onEdit}
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
    </li>
  );
}
