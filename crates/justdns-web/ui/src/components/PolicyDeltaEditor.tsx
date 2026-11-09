// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { InfoHint } from "./InfoHint";
import { type ListObject, type PolicyDelta, type Service } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./PolicyDeltaEditor.module.css";

/** View the service catalogue as switchable objects keyed by their `svc:<id>`
 *  list id, so per-scope service toggles ride the very same delta mechanism (and
 *  the same inherit/effective maths) as the block lists. */
export function serviceObjects(services: Service[]): ListObject[] {
  return services.map((service) => ({
    id: service.listId,
    label: service.name,
    globalOn: service.globalOn,
  }));
}

/** Where one list sits in a delta: left to the parent scope, forced on, or
 *  forced off. */
export type ListState = "inherit" | "on" | "off";

/** Whether a delta changes nothing — no list tweaks and no ad-hoc rules. */
export function isEmptyDelta(delta: PolicyDelta): boolean {
  return (
    delta.listsAdded.length === 0 &&
    delta.listsDisabled.length === 0 &&
    delta.allow.length === 0 &&
    delta.block.length === 0
  );
}

/** Read a list's state out of a delta. */
export function listStateOf(delta: PolicyDelta, id: string): ListState {
  if (delta.listsAdded.includes(id)) return "on";
  if (delta.listsDisabled.includes(id)) return "off";
  return "inherit";
}

/** Set a list's state in a delta, returning a fresh delta. */
export function withListState(delta: PolicyDelta, id: string, state: ListState): PolicyDelta {
  return {
    ...delta,
    listsAdded:
      state === "on"
        ? [...new Set([...delta.listsAdded, id])]
        : delta.listsAdded.filter((x) => x !== id),
    listsDisabled:
      state === "off"
        ? [...new Set([...delta.listsDisabled, id])]
        : delta.listsDisabled.filter((x) => x !== id),
  };
}

/** Fold a chain of deltas (group then device) over the global defaults into the
 *  set of list ids that are effectively on for a scope. */
export function effectiveLists(lists: ListObject[], deltas: PolicyDelta[]): Set<string> {
  const active = new Set(lists.filter((l) => l.globalOn).map((l) => l.id));
  for (const delta of deltas) {
    for (const id of delta.listsAdded) active.add(id);
    for (const id of delta.listsDisabled) active.delete(id);
  }
  return active;
}

/** Split a free-text domain box (one per line or comma-separated) into a clean,
 *  de-duplicated, lower-cased list. */
export function parseDomains(text: string): string[] {
  const seen = new Set<string>();
  for (const raw of text.split(/[\s,]+/)) {
    const domain = raw.trim().toLowerCase();
    if (domain) seen.add(domain);
  }
  return [...seen];
}

/** A read-only summary of which block lists are effectively on for a scope, so
 *  the user sees the net result of inheritance plus their own tweaks. */
export function EffectiveLists({ lists, active }: { lists: ListObject[]; active: Set<string> }) {
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

/** One inherit/on/off row for a switchable object (a block list or a service),
 *  shared by the lists and services sections so both read and behave alike. */
function PolicyToggleRow({
  object,
  delta,
  inherited,
  onChange,
}: {
  object: ListObject;
  delta: PolicyDelta;
  inherited: Set<string>;
  onChange: (next: PolicyDelta) => void;
}) {
  const { t } = useI18n();
  const states: ListState[] = ["inherit", "on", "off"];
  const state = listStateOf(delta, object.id);
  const inheritedOn = inherited.has(object.id);
  return (
    <div className={styles.policyList}>
      <span className={styles.policyListName}>
        {object.label}
        <span className={styles.policyInherited}>
          {inheritedOn ? t("clients.inheritedOn") : t("clients.inheritedOff")}
        </span>
      </span>
      <div className={styles.listSwitch} role="group" aria-label={object.label}>
        {states.map((s) => (
          <button
            key={s}
            type="button"
            className={styles.listSegment}
            data-active={state === s || undefined}
            aria-pressed={state === s}
            onClick={() => onChange(withListState(delta, object.id, s))}
          >
            {t(`clients.list.${s}` as MessageKey)}
          </button>
        ))}
      </div>
    </div>
  );
}

/** The shared policy-delta editor: the per-list inherit/on/off controls, the
 *  matching per-service controls and the two ad-hoc domain boxes. `inherited` is
 *  the set of list and service ids on in the parent scope, so each row shows what
 *  "inherit" means here. Used by the devices/groups page and the schedules page. */
export function PolicyDeltaEditor({
  lists,
  services = [],
  inherited,
  delta,
  onChange,
}: {
  lists: ListObject[];
  /** The blockable services, as switchable objects keyed by their `svc:<id>`
   *  list id. Rendered as their own section below the block lists. */
  services?: ListObject[];
  inherited: Set<string>;
  delta: PolicyDelta;
  onChange: (next: PolicyDelta) => void;
}) {
  const { t } = useI18n();
  // Only the two built-in objects (ads, malware) are always present; any further
  // entry is a block feed the user enabled. With none, point them at where more
  // lists come from, so an empty-looking editor is not mistaken for a limit.
  const hasFeeds = lists.some((list) => list.id !== "ads" && list.id !== "malware");

  return (
    <div className={styles.policy}>
      <div className={styles.policyLists}>
        <span className={styles.policyLabel}>
          {t("clients.listsLabel")}
          <InfoHint label={t("clients.listsLabel")} text={t("clients.listStateHint")} />
        </span>
        {lists.map((list) => (
          <PolicyToggleRow
            key={list.id}
            object={list}
            delta={delta}
            inherited={inherited}
            onChange={onChange}
          />
        ))}
        {hasFeeds ? null : <p className={styles.listsEmptyHint}>{t("clients.noFeedsHint")}</p>}
      </div>
      {services.length > 0 ? (
        <div className={styles.policyLists}>
          <span className={styles.policyLabel}>
            {t("clients.servicesLabel")}
            <InfoHint label={t("clients.servicesLabel")} text={t("clients.servicesHint")} />
          </span>
          {services.map((service) => (
            <PolicyToggleRow
              key={service.id}
              object={service}
              delta={delta}
              inherited={inherited}
              onChange={onChange}
            />
          ))}
        </div>
      ) : null}
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
