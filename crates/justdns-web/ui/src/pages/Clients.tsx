import { useEffect, useRef, useState, type FormEvent } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import {
  ApiError,
  api,
  emptyDelta,
  type ClientRow,
  type ClientsView,
  type DeviceKind,
  type DevicePolicy,
  type Group,
  type ListObject,
  type PolicyDelta,
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

/** Where a scope stands on one list relative to what it inherits: leave it as
 *  inherited, force it on (add), or force it off (disable). */
type ListState = "inherit" | "on" | "off";

/** Whether a delta changes nothing — no list tweaks and no ad-hoc rules. */
function isEmptyDelta(delta: PolicyDelta): boolean {
  return (
    delta.listsAdded.length === 0 &&
    delta.listsDisabled.length === 0 &&
    delta.allow.length === 0 &&
    delta.block.length === 0
  );
}

/** Read a list's state out of a delta. */
function listStateOf(delta: PolicyDelta, id: string): ListState {
  if (delta.listsAdded.includes(id)) return "on";
  if (delta.listsDisabled.includes(id)) return "off";
  return "inherit";
}

/** Set a list's state in a delta, returning a fresh delta. */
function withListState(delta: PolicyDelta, id: string, state: ListState): PolicyDelta {
  return {
    ...delta,
    listsAdded: state === "on" ? [...new Set([...delta.listsAdded, id])] : delta.listsAdded.filter((x) => x !== id),
    listsDisabled:
      state === "off" ? [...new Set([...delta.listsDisabled, id])] : delta.listsDisabled.filter((x) => x !== id),
  };
}

/** Fold a chain of deltas (group then device) over the global defaults into the
 *  set of list ids that are effectively on for a scope. */
function effectiveLists(lists: ListObject[], deltas: PolicyDelta[]): Set<string> {
  const active = new Set(lists.filter((l) => l.globalOn).map((l) => l.id));
  for (const delta of deltas) {
    for (const id of delta.listsAdded) active.add(id);
    for (const id of delta.listsDisabled) active.delete(id);
  }
  return active;
}

/** Split a free-text domain box (one per line or comma-separated) into a clean,
 *  de-duplicated, lower-cased list. */
function parseDomains(text: string): string[] {
  const seen = new Set<string>();
  for (const raw of text.split(/[\s,]+/)) {
    const domain = raw.trim().toLowerCase();
    if (domain) seen.add(domain);
  }
  return [...seen];
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
                    lists={view.lists}
                    groups={view.groups}
                    busy={pending.has(`edit:${key}`)}
                    onCancel={() => setEditing(null)}
                    onSave={(nextTarget, nextName, nextKind, nextNote, policy) =>
                      run(`edit:${key}`, () =>
                        api.editClient(key, nextTarget, nextName, nextKind, nextNote, policy),
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
                      {row.known && (row.group || (row.policy && !isEmptyDelta(row.policy))) ? (
                        <span className={styles.policyBadge}>
                          {row.group
                            ? (view.groups.find((g) => g.id === row.group)?.name ??
                              t("clients.groupGone"))
                            : t("clients.ownPolicy")}
                        </span>
                      ) : null}
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

      {view && <GroupsSection view={view} run={run} pending={pending} />}

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

/** The inline editor shown in place of a known device's row: identity, group
 *  and the device's own filtering policy. */
function ClientEditor({
  row,
  lists,
  groups,
  busy,
  onSave,
  onCancel,
}: {
  row: ClientRow;
  lists: ListObject[];
  groups: Group[];
  busy: boolean;
  onSave: (
    target: string,
    name: string,
    kind: DeviceKind,
    note: string | undefined,
    policy: DevicePolicy,
  ) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [target, setTarget] = useState(row.target);
  const [name, setName] = useState(row.name ?? "");
  const [kind, setKind] = useState<DeviceKind>(row.kind);
  const [note, setNote] = useState(row.note ?? "");
  const [group, setGroup] = useState<string | null>(row.group ?? null);
  const [delta, setDelta] = useState<PolicyDelta>(row.policy ?? emptyDelta());

  // A device inherits the global default and, when it has a group, that group's
  // delta on top; the policy editor shows list inheritance against that.
  const groupDelta = groups.find((g) => g.id === group)?.policy;
  const inherited = effectiveLists(lists, groupDelta ? [groupDelta] : []);
  const active = effectiveLists(lists, groupDelta ? [groupDelta, delta] : [delta]);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (target.trim() === "" || name.trim() === "") return;
    onSave(target.trim(), name.trim(), kind, note.trim() || undefined, { group, delta });
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
      <label className={styles.groupField}>
        <span className={styles.policyLabel}>{t("clients.groupLabel")}</span>
        <select
          className={styles.select}
          value={group ?? ""}
          onChange={(event) => setGroup(event.target.value || null)}
          aria-label={t("clients.groupLabel")}
        >
          <option value="">{t("clients.groupNone")}</option>
          {groups.map((g) => (
            <option key={g.id} value={g.id}>
              {g.name}
            </option>
          ))}
        </select>
      </label>
      <PolicyEditor lists={lists} inherited={inherited} delta={delta} onChange={setDelta} />
      <EffectiveLists lists={lists} active={active} />
    </form>
  );
}

/** A read-only summary of which block lists are effectively on for a scope, so
 *  the user sees the net result of inheritance plus their own tweaks. */
function EffectiveLists({ lists, active }: { lists: ListObject[]; active: Set<string> }) {
  const { t } = useI18n();
  const on = lists.filter((l) => active.has(l.id));
  return (
    <p className={styles.effective}>
      <span className={styles.policyLabel}>{t("clients.effectiveLabel")}</span>
      {on.length === 0 ? (
        <span className={styles.effectiveNone}>{t("clients.effectiveNone")}</span>
      ) : (
        on.map((l) => (
          <span key={l.id} className={styles.effectiveChip}>
            {l.label}
          </span>
        ))
      )}
    </p>
  );
}

/** The shared policy editor: the per-list inherit/on/off controls and the two
 *  ad-hoc domain boxes. `inherited` is the set of list ids that are on in the
 *  parent scope, so each list shows what "inherit" means here. */
function PolicyEditor({
  lists,
  inherited,
  delta,
  onChange,
}: {
  lists: ListObject[];
  inherited: Set<string>;
  delta: PolicyDelta;
  onChange: (next: PolicyDelta) => void;
}) {
  const { t } = useI18n();
  const states: ListState[] = ["inherit", "on", "off"];

  return (
    <div className={styles.policy}>
      <div className={styles.policyLists}>
        <span className={styles.policyLabel}>{t("clients.listsLabel")}</span>
        {lists.map((list) => {
          const state = listStateOf(delta, list.id);
          const inheritedOn = inherited.has(list.id);
          return (
            <div key={list.id} className={styles.policyList}>
              <span className={styles.policyListName}>
                {list.label}
                <span className={styles.policyInherited}>
                  {inheritedOn ? t("clients.inheritedOn") : t("clients.inheritedOff")}
                </span>
              </span>
              <div
                className={styles.listSwitch}
                role="group"
                aria-label={list.label}
              >
                {states.map((s) => (
                  <button
                    key={s}
                    type="button"
                    className={styles.listSegment}
                    data-active={state === s || undefined}
                    aria-pressed={state === s}
                    onClick={() => onChange(withListState(delta, list.id, s))}
                  >
                    {t(`clients.list.${s}` as MessageKey)}
                  </button>
                ))}
              </div>
            </div>
          );
        })}
      </div>
      <div className={styles.policyDomains}>
        <label className={styles.policyDomain}>
          <span className={styles.policyLabel}>{t("clients.allowLabel")}</span>
          <textarea
            className={styles.domainBox}
            value={delta.allow.join("\n")}
            onChange={(e) => onChange({ ...delta, allow: parseDomains(e.target.value) })}
            placeholder={t("clients.domainsHint")}
            rows={2}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className={styles.policyDomain}>
          <span className={styles.policyLabel}>{t("clients.blockLabel")}</span>
          <textarea
            className={styles.domainBox}
            value={delta.block.join("\n")}
            onChange={(e) => onChange({ ...delta, block: parseDomains(e.target.value) })}
            placeholder={t("clients.domainsHint")}
            rows={2}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
      </div>
    </div>
  );
}

/** The groups panel: create groups and edit each group's policy. A group's
 *  delta layers on the global default and is inherited by every device in it. */
function GroupsSection({
  view,
  run,
  pending,
}: {
  view: ClientsView;
  run: (key: string, action: () => Promise<ClientsView>) => Promise<boolean>;
  pending: Set<string>;
}) {
  const { t } = useI18n();
  const [newName, setNewName] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<Group | null>(null);

  // Devices per group, so each group shows how many clients it covers.
  const counts = new Map<string, number>();
  for (const row of view.clients) {
    if (row.known && row.group) counts.set(row.group, (counts.get(row.group) ?? 0) + 1);
  }

  function submitAdd(event: FormEvent) {
    event.preventDefault();
    if (newName.trim() === "") return;
    run("group:add", () => api.addGroup(newName.trim(), emptyDelta())).then((ok) => {
      if (ok) setNewName("");
    });
  }

  return (
    <section className={styles.groups}>
      <h2 className={styles.groupsTitle}>
        {t("groups.title")}
        <InfoHint label={t("groups.title")} text={t("groups.hint")} />
      </h2>
      <p className={styles.lead}>{t("groups.lead")}</p>

      <form className={styles.groupAdd} onSubmit={submitAdd}>
        <input
          className={styles.input}
          value={newName}
          onChange={(event) => setNewName(event.target.value)}
          placeholder={t("groups.namePlaceholder")}
          aria-label={t("groups.namePlaceholder")}
          autoComplete="off"
        />
        <button className={styles.addButton} type="submit" disabled={pending.has("group:add")}>
          {t("groups.add")}
        </button>
      </form>

      {view.groups.length === 0 ? (
        <p className={styles.empty}>{t("groups.empty")}</p>
      ) : (
        <ul className={styles.entries}>
          {view.groups.map((group) => {
            const isEditing = editing === group.id;
            return (
              <li key={group.id} className={styles.entry} data-editing={isEditing || undefined}>
                {isEditing ? (
                  <GroupEditor
                    group={group}
                    lists={view.lists}
                    busy={pending.has(`group:edit:${group.id}`)}
                    onCancel={() => setEditing(null)}
                    onSave={(name, delta) =>
                      run(`group:edit:${group.id}`, () =>
                        api.editGroup(group.id, name, delta),
                      ).then((ok) => {
                        if (ok) setEditing(null);
                      })
                    }
                  />
                ) : (
                  <>
                    <span className={styles.glyph} aria-hidden="true">
                      ◇
                    </span>
                    <span className={styles.identity}>
                      <span className={styles.name}>{group.name}</span>
                      <span className={styles.target}>
                        {t("groups.deviceCount", { n: counts.get(group.id) ?? 0 })}
                      </span>
                    </span>
                    <span className={styles.actions}>
                      <button
                        type="button"
                        className={styles.action}
                        onClick={() => setEditing(group.id)}
                      >
                        {t("clients.editTitle")}
                      </button>
                      <button
                        type="button"
                        className={styles.action}
                        onClick={() => setConfirming(group)}
                      >
                        {t("clients.removeTitle")}
                      </button>
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
          title={t("groups.removeConfirmTitle")}
          body={t("groups.removeConfirmBody", { name: confirming.name })}
          confirmLabel={t("clients.removeConfirm")}
          cancelLabel={t("clients.cancel")}
          busy={pending.has(`group:remove:${confirming.id}`)}
          onConfirm={() =>
            run(`group:remove:${confirming.id}`, () => api.removeGroup(confirming.id)).then((ok) => {
              if (ok) setConfirming(null);
            })
          }
          onCancel={() => setConfirming(null)}
        />
      )}
    </section>
  );
}

/** The inline editor for one group: its name and its policy delta, shown against
 *  the global default it inherits. */
function GroupEditor({
  group,
  lists,
  busy,
  onSave,
  onCancel,
}: {
  group: Group;
  lists: ListObject[];
  busy: boolean;
  onSave: (name: string, delta: PolicyDelta) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [name, setName] = useState(group.name);
  const [delta, setDelta] = useState<PolicyDelta>(group.policy);

  const inherited = effectiveLists(lists, []);
  const active = effectiveLists(lists, [delta]);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (name.trim() === "") return;
    onSave(name.trim(), delta);
  }

  return (
    <form className={styles.rowEdit} onSubmit={submit}>
      <div className={styles.rowEditInputs}>
        <input
          className={styles.rowEditInput}
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-label={t("groups.namePlaceholder")}
          autoComplete="off"
          autoFocus
        />
        <button className={styles.rowSave} type="submit" disabled={busy}>
          {t("clients.update")}
        </button>
        <button className={styles.rowCancel} type="button" onClick={onCancel} disabled={busy}>
          {t("clients.cancel")}
        </button>
      </div>
      <PolicyEditor lists={lists} inherited={inherited} delta={delta} onChange={setDelta} />
      <EffectiveLists lists={lists} active={active} />
    </form>
  );
}
