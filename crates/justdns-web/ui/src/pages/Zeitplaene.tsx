// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState, type FormEvent } from "react";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import {
  EffectiveLists,
  PolicyDeltaEditor,
  effectiveLists,
  serviceObjects,
} from "../components/PolicyDeltaEditor";
import {
  ApiError,
  api,
  emptyDelta,
  WEEKDAYS,
  type ClientsView,
  type PolicyDelta,
  type Schedule,
  type ScheduleDraft,
  type ScheduleMode,
  type ScheduleScope,
  type SchedulesView,
  type Weekday,
} from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useSettings } from "../state/settings";
import styles from "./Zeitplaene.module.css";

function errorKey(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.status === 400) return "schedules.badInput";
    if (error.status === 404) return "common.notFound";
  }
  return "common.saveFailed";
}

/** A blank schedule to start from: a weeknight pause, all weekdays off so the
 *  user has to choose at least one before saving. */
function blankDraft(): ScheduleDraft {
  return {
    name: "",
    enabled: true,
    days: ["mon", "tue", "wed", "thu", "fri"],
    start: "22:00",
    end: "07:00",
    target: { scope: "global" },
    effect: { mode: "pause", delta: emptyDelta() },
  };
}

/** True when the window runs past midnight (end at or before start). */
function isOvernight(start: string, end: string): boolean {
  return end <= start;
}

export function Zeitplaene() {
  const { t } = useI18n();
  const [view, setView] = useState<SchedulesView | null>(null);
  const [clients, setClients] = useState<ClientsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());
  // The id of the schedule being edited, "new" while the add form is open, or
  // null when nothing is open.
  const [editing, setEditing] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<Schedule | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    Promise.all([api.schedules(controller.signal), api.clients(controller.signal)])
      .then(([schedules, clientsView]) => {
        setView(schedules);
        setClients(clientsView);
      })
      .catch(() => {
        if (controller.signal.aborted) return;
        setError(t("schedules.loadFailed"));
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

  async function run(key: string, action: () => Promise<SchedulesView>): Promise<boolean> {
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

  const schedules = view?.schedules ?? [];

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>
          {t("schedules.title")}
          <InfoHint label={t("schedules.title")} text={t("schedules.hint")} />
        </h1>
        <p className={styles.lead}>{t("schedules.lead")}</p>
      </header>

      {error && <p className={styles.banner}>{error}</p>}

      <div className={styles.toolbar}>
        <button
          type="button"
          className={styles.newButton}
          onClick={() => setEditing(editing === "new" ? null : "new")}
          aria-expanded={editing === "new"}
        >
          {t("schedules.new")}
        </button>
      </div>

      {editing === "new" && clients && (
        <ScheduleEditor
          key="new"
          draft={blankDraft()}
          clients={clients}
          busy={pending.has("add")}
          submitLabel={t("schedules.add")}
          onCancel={() => setEditing(null)}
          onSave={(draft) =>
            run("add", () => api.addSchedule(draft)).then((ok) => {
              if (ok) setEditing(null);
            })
          }
        />
      )}

      {view === null ? (
        <p className={styles.empty}>{t("schedules.loadFailed")}</p>
      ) : schedules.length === 0 ? (
        <p className={styles.empty}>{t("schedules.empty")}</p>
      ) : (
        <ul className={styles.entries}>
          {schedules.map((schedule) => {
            const isEditing = editing === schedule.id;
            return (
              <li key={schedule.id} className={styles.entry} data-editing={isEditing || undefined}>
                {isEditing && clients ? (
                  <ScheduleEditor
                    draft={schedule}
                    clients={clients}
                    busy={pending.has(`edit:${schedule.id}`)}
                    submitLabel={t("schedules.update")}
                    onCancel={() => setEditing(null)}
                    onSave={(draft) =>
                      run(`edit:${schedule.id}`, () =>
                        api.editSchedule({ ...draft, id: schedule.id }),
                      ).then((ok) => {
                        if (ok) setEditing(null);
                      })
                    }
                  />
                ) : (
                  <ScheduleRow
                    schedule={schedule}
                    clients={clients}
                    busy={pending.has(`toggle:${schedule.id}`)}
                    onToggle={(enabled) =>
                      run(`toggle:${schedule.id}`, () =>
                        api.editSchedule({ ...schedule, enabled }),
                      )
                    }
                    onEdit={() => setEditing(schedule.id)}
                    onDelete={() => setConfirming(schedule)}
                  />
                )}
              </li>
            );
          })}
        </ul>
      )}

      {confirming && (
        <ConfirmDialog
          title={t("schedules.confirmTitle")}
          body={t("schedules.confirmBody", { name: confirming.name })}
          confirmLabel={t("schedules.delete")}
          cancelLabel={t("schedules.cancel")}
          busy={pending.has(`remove:${confirming.id}`)}
          onCancel={() => setConfirming(null)}
          onConfirm={() =>
            run(`remove:${confirming.id}`, () => api.removeSchedule(confirming.id)).then((ok) => {
              if (ok) setConfirming(null);
            })
          }
        />
      )}
    </div>
  );
}

/** The display name of a schedule's target, for the summary line. */
function targetName(schedule: Schedule, clients: ClientsView | null): string {
  if (schedule.target.scope === "global") return "";
  const id = schedule.target.id ?? "";
  if (schedule.target.scope === "group") {
    return clients?.groups.find((g) => g.id === id)?.name ?? id;
  }
  return clients?.clients.find((c) => c.target === id)?.name ?? id;
}

/** One schedule, collapsed: its name and state, the window it covers, its target
 *  and effect, with an enable toggle and edit/delete actions. */
function ScheduleRow({
  schedule,
  clients,
  busy,
  onToggle,
  onEdit,
  onDelete,
}: {
  schedule: Schedule;
  clients: ClientsView | null;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const overnight = isOvernight(schedule.start, schedule.end);
  const scopeName = targetName(schedule, clients);

  return (
    <div className={styles.row} data-off={!schedule.enabled || undefined}>
      <div className={styles.rowMain}>
        <div className={styles.rowHead}>
          <span className={styles.rowName}>{schedule.name}</span>
          <span
            className={styles.effectChip}
            data-mode={schedule.effect.mode}
          >
            {t(`schedules.effect.${schedule.effect.mode}` as MessageKey)}
          </span>
        </div>
        <div className={styles.rowMeta}>
          <span className={styles.window}>
            {schedule.start}&thinsp;–&thinsp;{schedule.end}
            {overnight && <span className={styles.overnight}>{t("schedules.overnightTag")}</span>}
          </span>
          <span className={styles.days} aria-hidden="true">
            {WEEKDAYS.map((day) => (
              <span
                key={day}
                className={styles.dayPip}
                data-on={schedule.days.includes(day) || undefined}
              >
                {t(`schedules.dayMin.${day}` as MessageKey)}
              </span>
            ))}
          </span>
          <span className={styles.visuallyHidden}>
            {schedule.days.map((d) => t(`schedules.day.${d}` as MessageKey)).join(", ")}
          </span>
          <span className={styles.target}>
            {schedule.target.scope === "global"
              ? t("schedules.targetGlobalSummary")
              : t(
                  schedule.target.scope === "group"
                    ? "schedules.targetGroupSummary"
                    : "schedules.targetDeviceSummary",
                  { name: scopeName },
                )}
          </span>
        </div>
      </div>
      <div className={styles.rowActions}>
        <Toggle
          label={t("schedules.enabledAria", { name: schedule.name })}
          checked={schedule.enabled}
          onChange={onToggle}
          disabled={busy}
        />
        <button type="button" className={styles.rowButton} onClick={onEdit}>
          {t("schedules.edit")}
        </button>
        <button type="button" className={styles.rowDelete} onClick={onDelete}>
          {t("schedules.delete")}
        </button>
      </div>
    </div>
  );
}

/** The add/edit form for one schedule: name, on/off, weekdays, the time window,
 *  the target scope and, for a block, the filtering delta. */
function ScheduleEditor({
  draft,
  clients,
  busy,
  submitLabel,
  onCancel,
  onSave,
}: {
  draft: ScheduleDraft;
  clients: ClientsView;
  busy: boolean;
  submitLabel: string;
  onCancel: () => void;
  onSave: (draft: ScheduleDraft) => void;
}) {
  const { t } = useI18n();
  const { settings } = useSettings();
  const [name, setName] = useState(draft.name);
  const [enabled, setEnabled] = useState(draft.enabled);
  const [days, setDays] = useState<Weekday[]>(draft.days);
  const [start, setStart] = useState(draft.start);
  const [end, setEnd] = useState(draft.end);
  const [scope, setScope] = useState<ScheduleScope>(draft.target.scope);
  const [targetId, setTargetId] = useState<string>(draft.target.id ?? "");
  const [mode, setMode] = useState<ScheduleMode>(draft.effect.mode);
  const [delta, setDelta] = useState<PolicyDelta>(draft.effect.delta ?? emptyDelta());

  const lists = clients.lists;
  const services = serviceObjects(clients.services);
  const catalogue = [...lists, ...services];
  const groups = clients.groups;
  const devices = clients.clients.filter((c) => c.known);

  // What is already on for the chosen target, so the delta editor shows what
  // "inherit" means and the summary reflects the net result while the window is
  // open. The schedule's own delta layers on top of this.
  function baseDeltas(): PolicyDelta[] {
    if (scope === "group") {
      const group = groups.find((g) => g.id === targetId);
      return group?.policy ? [group.policy] : [];
    }
    if (scope === "device") {
      const device = devices.find((d) => d.target === targetId);
      const groupDelta = groups.find((g) => g.id === device?.group)?.policy;
      const chain: PolicyDelta[] = [];
      if (groupDelta) chain.push(groupDelta);
      if (device?.policy) chain.push(device.policy);
      return chain;
    }
    return [];
  }

  const base = baseDeltas();
  const inherited = effectiveLists(catalogue, base);
  const active = effectiveLists(catalogue, [...base, delta]);
  const overnight = isOvernight(start, end);

  function toggleDay(day: Weekday) {
    setDays((prev) =>
      prev.includes(day) ? prev.filter((d) => d !== day) : [...prev, day],
    );
  }

  const canSave = name.trim() !== "" && days.length > 0 && (scope === "global" || targetId !== "");

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!canSave) return;
    // Keep weekdays in canonical order regardless of click order.
    const orderedDays = WEEKDAYS.filter((d) => days.includes(d));
    onSave({
      name: name.trim(),
      enabled,
      days: orderedDays,
      start,
      end,
      target: scope === "global" ? { scope } : { scope, id: targetId },
      effect:
        mode === "pause"
          ? { mode: "pause", delta: emptyDelta() }
          : { mode: "block", delta },
    });
  }

  return (
    <form className={styles.editor} onSubmit={submit}>
      <div className={styles.editorTop}>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("schedules.nameLabel")}</span>
          <input
            className={styles.input}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t("schedules.namePlaceholder")}
            aria-label={t("schedules.nameLabel")}
            autoComplete="off"
            autoFocus
          />
        </label>
        <label className={styles.enabledField}>
          <span className={styles.fieldLabel}>{t("schedules.enabledLabel")}</span>
          <Toggle
            label={t("schedules.enabledLabel")}
            checked={enabled}
            onChange={setEnabled}
          />
        </label>
      </div>

      <fieldset className={styles.fieldset}>
        <legend className={styles.fieldLabel}>{t("schedules.daysLabel")}</legend>
        <div className={styles.dayPicker} role="group" aria-label={t("schedules.daysLabel")}>
          {WEEKDAYS.map((day) => (
            <button
              key={day}
              type="button"
              className={styles.dayToggle}
              data-active={days.includes(day) || undefined}
              aria-pressed={days.includes(day)}
              onClick={() => toggleDay(day)}
            >
              {t(`schedules.dayShort.${day}` as MessageKey)}
            </button>
          ))}
        </div>
      </fieldset>

      <div className={styles.times}>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("schedules.startLabel")}</span>
          <input
            className={styles.timeInput}
            type="time"
            value={start}
            onChange={(e) => setStart(e.target.value)}
            aria-label={t("schedules.startLabel")}
            required
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("schedules.endLabel")}</span>
          <input
            className={styles.timeInput}
            type="time"
            value={end}
            onChange={(e) => setEnd(e.target.value)}
            aria-label={t("schedules.endLabel")}
            required
          />
        </label>
        <p className={styles.timeHint}>
          {overnight ? t("schedules.overnightHint") : t("schedules.sameDayHint")}
          <span className={styles.tzNote}>
            {t("schedules.tzNote", { tz: settings.timezone })}
          </span>
        </p>
      </div>

      <div className={styles.targetRow}>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("schedules.targetLabel")}</span>
          <select
            className={styles.select}
            value={scope}
            onChange={(e) => {
              setScope(e.target.value as ScheduleScope);
              setTargetId("");
            }}
            aria-label={t("schedules.targetLabel")}
          >
            <option value="global">{t("schedules.scope.global")}</option>
            <option value="group">{t("schedules.scope.group")}</option>
            <option value="device">{t("schedules.scope.device")}</option>
          </select>
        </label>
        {scope === "group" && (
          <label className={styles.field}>
            <span className={styles.fieldLabel}>{t("schedules.groupLabel")}</span>
            {groups.length === 0 ? (
              <span className={styles.pickerEmpty}>{t("schedules.noGroups")}</span>
            ) : (
              <select
                className={styles.select}
                value={targetId}
                onChange={(e) => setTargetId(e.target.value)}
                aria-label={t("schedules.groupLabel")}
              >
                <option value="">{t("schedules.pickOne")}</option>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name}
                  </option>
                ))}
              </select>
            )}
          </label>
        )}
        {scope === "device" && (
          <label className={styles.field}>
            <span className={styles.fieldLabel}>{t("schedules.deviceLabel")}</span>
            {devices.length === 0 ? (
              <span className={styles.pickerEmpty}>{t("schedules.noDevices")}</span>
            ) : (
              <select
                className={styles.select}
                value={targetId}
                onChange={(e) => setTargetId(e.target.value)}
                aria-label={t("schedules.deviceLabel")}
              >
                <option value="">{t("schedules.pickOne")}</option>
                {devices.map((d) => (
                  <option key={d.target} value={d.target}>
                    {d.name ?? d.target} ({d.target})
                  </option>
                ))}
              </select>
            )}
          </label>
        )}
      </div>

      <fieldset className={styles.fieldset}>
        <legend className={styles.fieldLabel}>
          {t("schedules.effectLabel")}
          <InfoHint label={t("schedules.effectLabel")} text={t("schedules.effectHint")} />
        </legend>
        <div className={styles.effectSwitch} role="group" aria-label={t("schedules.effectLabel")}>
          {(["pause", "block"] as ScheduleMode[]).map((m) => (
            <button
              key={m}
              type="button"
              className={styles.effectSegment}
              data-active={mode === m || undefined}
              aria-pressed={mode === m}
              onClick={() => setMode(m)}
            >
              {t(`schedules.effect.${m}` as MessageKey)}
            </button>
          ))}
        </div>
        <p className={styles.effectDesc}>
          {t(mode === "pause" ? "schedules.effectPauseDesc" : "schedules.effectBlockDesc")}
        </p>
      </fieldset>

      {mode === "block" && (
        <>
          <PolicyDeltaEditor
            lists={lists}
            services={services}
            inherited={inherited}
            delta={delta}
            onChange={setDelta}
          />
          <EffectiveLists lists={catalogue} active={active} />
        </>
      )}

      <div className={styles.editorActions}>
        <button className={styles.save} type="submit" disabled={busy || !canSave}>
          {submitLabel}
        </button>
        <button className={styles.cancel} type="button" onClick={onCancel} disabled={busy}>
          {t("schedules.cancel")}
        </button>
      </div>
    </form>
  );
}
