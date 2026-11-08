import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import { ApiError, api, type Alias, type Rewrite, type RewritesView } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Rewrites.module.css";

/** Pick the message key for a mutation's failure, shown in the banner. A 409
 *  (an address object still in use) carries a precise server message, handled by
 *  the caller directly rather than through this map. */
function errorKey(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.status === 400) return "rewrites.badInput";
    if (error.status === 404) return "common.notFound";
  }
  return "common.saveFailed";
}

/** Split a free-text field into separate tokens: entries are separated by commas
 *  or whitespace. Used for the IP fields and the name field, where several names
 *  can share one target. Empty fragments are dropped. */
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

  // The top rewrite form only ever adds; editing happens inline in the row.
  const [name, setName] = useState("");
  const [ips, setIps] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [editing, setEditing] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

  // The address-object (alias) form and its own inline edit / confirm state.
  const [aliasName, setAliasName] = useState("");
  const [aliasIps, setAliasIps] = useState("");
  const [aliasEditing, setAliasEditing] = useState<string | null>(null);
  const [aliasConfirming, setAliasConfirming] = useState<string | null>(null);

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
      // A blocked delete (409) names the referencing rewrites in its message;
      // show that verbatim. Everything else maps to a translated line.
      setError(err instanceof ApiError && err.status === 409 ? err.message : t(errorKey(err)));
      return false;
    } finally {
      mark(key, false);
    }
  }

  function togglePicked(aliasName: string) {
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(aliasName)) next.delete(aliasName);
      else next.add(aliasName);
      return next;
    });
  }

  function submitAdd(event: FormEvent) {
    event.preventDefault();
    const addresses = splitTokens(ips);
    const aliases = [...picked];
    const names = splitTokens(name);
    if (names.length === 0) return;
    // A rewrite needs at least one target: an address or an object.
    if (addresses.length === 0 && aliases.length === 0) return;
    run("add", () => api.setRewrites(names, addresses, aliases)).then((ok) => {
      if (ok) {
        setName("");
        setIps("");
        setPicked(new Set());
      }
    });
  }

  function saveEdit(original: string, newName: string, addresses: string[], aliases: string[]) {
    run(`edit:${original}`, () => api.editRewrite(original, newName, addresses, aliases)).then(
      (ok) => {
        if (ok) setEditing(null);
      },
    );
  }

  function confirmRemove() {
    const target = confirming;
    if (!target) return;
    run(`rm:${target}`, () => api.removeRewrite(target)).then((ok) => {
      if (ok && editing === target) setEditing(null);
      setConfirming(null);
    });
  }

  function submitAliasAdd(event: FormEvent) {
    event.preventDefault();
    const addresses = splitTokens(aliasIps);
    const objectName = aliasName.trim();
    if (!objectName || addresses.length === 0) return;
    run("alias-add", () => api.setAlias(objectName, addresses)).then((ok) => {
      if (ok) {
        setAliasName("");
        setAliasIps("");
      }
    });
  }

  function saveAliasEdit(original: string, newName: string, addresses: string[]) {
    run(`alias-edit:${original}`, () => api.editAlias(original, newName, addresses)).then((ok) => {
      if (ok) setAliasEditing(null);
    });
  }

  function askRemoveAlias(alias: Alias) {
    // Catch an in-use object before any request, so the message can name the
    // referencing rewrites in the viewer's own language.
    const usedBy = (view?.rewrites ?? [])
      .filter((rewrite) => rewrite.aliases.includes(alias.name))
      .map((rewrite) => rewrite.name);
    if (usedBy.length > 0) {
      setError(t("aliases.inUse", { name: alias.name, names: usedBy.join(", ") }));
      return;
    }
    setError(null);
    setAliasConfirming(alias.name);
  }

  function confirmRemoveAlias() {
    const target = aliasConfirming;
    if (!target) return;
    run(`alias-rm:${target}`, () => api.removeAlias(target)).then((ok) => {
      if (ok && aliasEditing === target) setAliasEditing(null);
      setAliasConfirming(null);
    });
  }

  // Current address of each object, for the tooltip on a rewrite's alias chip.
  const aliasIpMap = useMemo(
    () => new Map((view?.aliases ?? []).map((alias) => [alias.name, alias.ips])),
    [view],
  );

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
        <div className={styles.addRow}>
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
        </div>
        <AliasPicker
          available={view.aliases}
          selected={picked}
          onToggle={togglePicked}
          busy={busy}
        />
      </form>
      <p className={styles.hint}>{t("rewrites.multiHint")}</p>
      <p className={styles.hint}>{t("rewrites.targetHint")}</p>

      {view.rewrites.length === 0 ? (
        <p className={styles.empty}>{t("rewrites.empty")}</p>
      ) : (
        <ul className={styles.entries}>
          {view.rewrites.map((rewrite) => (
            <RewriteRow
              key={rewrite.name}
              rewrite={rewrite}
              aliasIps={aliasIpMap}
              available={view.aliases}
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
              onSaveEdit={(newName, addresses, aliases) =>
                saveEdit(rewrite.name, newName, addresses, aliases)
              }
              onToggle={(enabled) =>
                run(`en:${rewrite.name}`, () => api.setRewriteEnabled(rewrite.name, enabled))
              }
              onRemove={() => setConfirming(rewrite.name)}
            />
          ))}
        </ul>
      )}

      <section className={styles.section}>
        <div className={styles.sectionHead}>
          <h2 className={styles.sectionTitle}>
            {t("aliases.title")}
            <InfoHint label={t("aliases.title")} text={t("aliases.hint")} />
          </h2>
          <p className={styles.sectionLead}>{t("aliases.lead")}</p>
        </div>

        <form className={styles.addForm} onSubmit={submitAliasAdd}>
          <div className={styles.addRow}>
            <input
              className={styles.input}
              value={aliasName}
              onChange={(event) => setAliasName(event.target.value)}
              placeholder={t("aliases.namePlaceholderAdd")}
              aria-label={t("aliases.nameAriaAdd")}
              autoComplete="off"
              spellCheck={false}
            />
            <input
              className={styles.input}
              value={aliasIps}
              onChange={(event) => setAliasIps(event.target.value)}
              placeholder={t("aliases.ipPlaceholder")}
              aria-label={t("aliases.ipAria")}
              autoComplete="off"
              spellCheck={false}
            />
            <button type="submit" className={styles.addButton} disabled={busy}>
              {t("aliases.add")}
            </button>
          </div>
        </form>
        <p className={styles.hint}>{t("aliases.multiHint")}</p>

        {view.aliases.length === 0 ? (
          <p className={styles.empty}>{t("aliases.empty")}</p>
        ) : (
          <ul className={styles.aliasEntries}>
            {view.aliases.map((alias) => (
              <AliasRow
                key={alias.name}
                alias={alias}
                editing={aliasEditing === alias.name}
                busy={
                  pending.has(`alias-edit:${alias.name}`) ||
                  pending.has(`alias-rm:${alias.name}`)
                }
                onStartEdit={() => {
                  setError(null);
                  setAliasEditing(alias.name);
                }}
                onCancelEdit={() => setAliasEditing(null)}
                onSaveEdit={(newName, addresses) => saveAliasEdit(alias.name, newName, addresses)}
                onRemove={() => askRemoveAlias(alias)}
              />
            ))}
          </ul>
        )}
      </section>

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

      {aliasConfirming ? (
        <ConfirmDialog
          title={t("aliases.removeConfirmTitle")}
          body={t("aliases.removeConfirmBody", { name: aliasConfirming })}
          confirmLabel={t("aliases.removeConfirm")}
          cancelLabel={t("aliases.cancel")}
          busy={pending.has(`alias-rm:${aliasConfirming}`)}
          onConfirm={confirmRemoveAlias}
          onCancel={() => setAliasConfirming(null)}
        />
      ) : null}
    </div>
  );
}

/** The toggle-chip picker for a rewrite's address objects. Clicking a chip adds
 *  or removes that object from the rewrite's targets. Shown both in the add form
 *  and in a row's inline editor. */
function AliasPicker({
  available,
  selected,
  onToggle,
  busy,
}: {
  available: Alias[];
  selected: Set<string>;
  onToggle: (name: string) => void;
  busy: boolean;
}) {
  const { t } = useI18n();
  return (
    <div className={styles.aliasPick}>
      <span className={styles.aliasPickLabel}>{t("rewrites.aliasesLabel")}</span>
      {available.length === 0 ? (
        <span className={styles.aliasPickEmpty}>{t("rewrites.aliasesNone")}</span>
      ) : (
        <div className={styles.aliasPickChips}>
          {available.map((alias) => (
            <button
              key={alias.name}
              type="button"
              className={styles.aliasPickChip}
              data-on={selected.has(alias.name) || undefined}
              aria-pressed={selected.has(alias.name)}
              aria-label={t("rewrites.aliasToggleAria", { name: alias.name })}
              title={alias.ips.join(", ")}
              disabled={busy}
              onClick={() => onToggle(alias.name)}
            >
              {alias.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** One rewrite. Normally a row with an on/off toggle, the name, its targets as
 *  chips (addresses, then any address objects) and edit/remove controls. In edit
 *  mode the name, addresses and object picker turn into inputs right here. */
function RewriteRow({
  rewrite,
  aliasIps,
  available,
  editing,
  busy,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onToggle,
  onRemove,
}: {
  rewrite: Rewrite;
  aliasIps: Map<string, string[]>;
  available: Alias[];
  editing: boolean;
  busy: boolean;
  onStartEdit: () => void;
  onCancelEdit: () => void;
  onSaveEdit: (name: string, ips: string[], aliases: string[]) => void;
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
          available={available}
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
              {rewrite.aliases.map((aliasName) => (
                <span
                  key={aliasName}
                  className={styles.aliasRef}
                  title={t("rewrites.aliasChipTitle", {
                    ips: (aliasIps.get(aliasName) ?? []).join(", ") || aliasName,
                  })}
                >
                  {aliasName}
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

/** The inline editor for a rewrite: its name, addresses and object picker become
 *  inputs in place. Seeded from the rewrite on mount; reports the new name,
 *  addresses and selected objects on save. */
function RewriteEditor({
  rewrite,
  available,
  busy,
  onSave,
  onCancel,
}: {
  rewrite: Rewrite;
  available: Alias[];
  busy: boolean;
  onSave: (name: string, ips: string[], aliases: string[]) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [draftName, setDraftName] = useState(rewrite.name);
  const [draftIps, setDraftIps] = useState(rewrite.ips.join(", "));
  const [draftAliases, setDraftAliases] = useState<Set<string>>(new Set(rewrite.aliases));
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    nameRef.current?.focus();
    nameRef.current?.select();
  }, []);

  function toggle(name: string) {
    setDraftAliases((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const addresses = splitTokens(draftIps);
    const aliases = [...draftAliases];
    const target = draftName.trim();
    if (!target) return;
    if (addresses.length === 0 && aliases.length === 0) return;
    onSave(target, addresses, aliases);
  }

  return (
    <form className={styles.rowEdit} onSubmit={submit}>
      <div className={styles.rowEditInputs}>
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
        <button type="button" className={styles.rowCancel} onClick={onCancel} disabled={busy}>
          {t("rewrites.cancel")}
        </button>
      </div>
      <AliasPicker available={available} selected={draftAliases} onToggle={toggle} busy={busy} />
    </form>
  );
}

/** One address object: a row with the name, its addresses as chips, and
 *  edit/remove controls. Edit mode swaps the name and addresses for inputs. */
function AliasRow({
  alias,
  editing,
  busy,
  onStartEdit,
  onCancelEdit,
  onSaveEdit,
  onRemove,
}: {
  alias: Alias;
  editing: boolean;
  busy: boolean;
  onStartEdit: () => void;
  onCancelEdit: () => void;
  onSaveEdit: (name: string, ips: string[]) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  return (
    <li className={styles.aliasEntry} data-editing={editing || undefined}>
      {editing ? (
        <AliasEditor alias={alias} busy={busy} onSave={onSaveEdit} onCancel={onCancelEdit} />
      ) : (
        <>
          <span className={styles.aliasEntryName} title={alias.name}>
            {alias.name}
          </span>
          <div className={styles.entryValue}>
            <span className={styles.arrow} aria-hidden="true">
              →
            </span>
            <span className={styles.ips}>
              {alias.ips.map((ip) => (
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
              aria-label={t("aliases.editAria", { name: alias.name })}
              title={t("aliases.editTitle")}
              disabled={busy}
              onClick={onStartEdit}
            >
              {t("aliases.editTitle")}
            </button>
            <button
              type="button"
              className={styles.remove}
              aria-label={t("aliases.removeAria", { name: alias.name })}
              title={t("aliases.removeTitle")}
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

/** The inline editor for an address object: name and addresses become inputs. */
function AliasEditor({
  alias,
  busy,
  onSave,
  onCancel,
}: {
  alias: Alias;
  busy: boolean;
  onSave: (name: string, ips: string[]) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [draftName, setDraftName] = useState(alias.name);
  const [draftIps, setDraftIps] = useState(alias.ips.join(", "));
  const nameRef = useRef<HTMLInputElement>(null);

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
      <div className={styles.rowEditInputs}>
        <input
          ref={nameRef}
          className={styles.rowEditInput}
          value={draftName}
          onChange={(event) => setDraftName(event.target.value)}
          aria-label={t("aliases.nameAriaEdit")}
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
          aria-label={t("aliases.ipAria")}
          autoComplete="off"
          spellCheck={false}
        />
        <button type="submit" className={styles.rowSave} disabled={busy}>
          {t("aliases.update")}
        </button>
        <button type="button" className={styles.rowCancel} onClick={onCancel} disabled={busy}>
          {t("aliases.cancel")}
        </button>
      </div>
    </form>
  );
}
