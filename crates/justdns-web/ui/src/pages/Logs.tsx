import { useEffect, useMemo, useState } from "react";

import { Toggle } from "../components/Toggle";
import {
  api,
  type ClientRow,
  type Group,
  type HistoryFilter,
  type NodeInfo,
  type QuerySource,
  type ResolverStat,
} from "../lib/api";
import { RECORD_TYPES, RESPONSE_CODES, recordTypeLabel, responseCodeLabel, sourceLabel } from "../lib/dns";
import { useI18n } from "../lib/i18n";
import { LOG_LIMIT, useHistory, useLogs, type LogRow } from "../lib/useLogs";
import { LogDetail } from "./LogDetail";
import styles from "./Logs.module.css";

/** Wait this long after the last keystroke before filtering, so typing a word
 *  does not fire a request per character. */
const SEARCH_DEBOUNCE_MS = 250;

/** The source facets, in the order they read in the sidebar: the signal worth
 *  seeing first, then the ordinary outcomes. */
const SOURCE_FACETS: QuerySource[] = ["blocked", "cache", "upstream", "local", "rewrite"];

/** Record-type and response-code options for the selects, by ascending wire
 *  value so the common ones (A, then AAAA; NOERROR, then the errors) lead. */
const RECORD_TYPE_OPTIONS = Object.keys(RECORD_TYPES)
  .map(Number)
  .sort((a, b) => a - b);
const RESPONSE_CODE_OPTIONS = Object.keys(RESPONSE_CODES)
  .map(Number)
  .sort((a, b) => a - b);

/** A `datetime-local` value is local wall-clock text; turn it into epoch ms, or
 *  `null` when empty or unparseable. */
function toMs(local: string): number | null {
  if (!local) return null;
  const ms = new Date(local).getTime();
  return Number.isNaN(ms) ? null : ms;
}

/** Load the cluster roster so a row's origin can read as a server hostname
 *  instead of a node-id prefix. A single-node instance gets just itself, and
 *  then `showNodes` is false so the badges stay hidden. */
function useNodeLabels() {
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  useEffect(() => {
    const controller = new AbortController();
    api
      .nodes(controller.signal)
      .then((res) => setNodes(res.nodes))
      .catch(() => {
        /* no roster just means the node badges stay hidden */
      });
    return () => controller.abort();
  }, []);

  const nodeLabels = useMemo(() => {
    const map = new Map<string, string>();
    for (const node of nodes) map.set(node.id, node.label);
    return map;
  }, [nodes]);
  const selfLabel = useMemo(() => nodes.find((node) => node.self)?.label ?? "", [nodes]);
  // Only worth showing which server answered once there is more than one.
  return { nodeLabels, selfLabel, showNodes: nodes.length > 1 };
}

export function Logs() {
  const { t, lang } = useI18n();
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [live, setLive] = useState(true);
  const [openKey, setOpenKey] = useState<string | null>(null);

  // Facet filters. Any one of them switches the page into history mode.
  const [sources, setSources] = useState<QuerySource[]>([]);
  const [recordType, setRecordType] = useState<number | null>(null);
  const [responseCode, setResponseCode] = useState<number | null>(null);
  const [resolver, setResolver] = useState<string | null>(null);
  const [device, setDevice] = useState<string | null>(null);
  const [group, setGroup] = useState<string | null>(null);
  const [startStr, setStartStr] = useState("");
  const [endStr, setEndStr] = useState("");
  const [resolverOptions, setResolverOptions] = useState<ResolverStat[]>([]);
  const [deviceOptions, setDeviceOptions] = useState<ClientRow[]>([]);
  const [groupOptions, setGroupOptions] = useState<Group[]>([]);

  useEffect(() => {
    const id = setTimeout(() => setQuery(input), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [input]);

  // The resolver options for the select come from the same per-day stats the
  // response-time view uses; a cache-only instance simply gets none.
  useEffect(() => {
    const controller = new AbortController();
    api
      .resolverStats(controller.signal)
      .then(setResolverOptions)
      .catch(() => {
        /* a missing resolver list just means an empty select */
      });
    return () => controller.abort();
  }, []);

  // The device and group options for the filter come from the devices view. Only
  // named devices can be filtered on (an unnamed client has no device to belong
  // to); a fresh instance with neither simply gets an empty, disabled select.
  useEffect(() => {
    const controller = new AbortController();
    api
      .clients(controller.signal)
      .then((view) => {
        setDeviceOptions(view.clients.filter((row) => row.known && !!row.name));
        setGroupOptions(view.groups);
      })
      .catch(() => {
        /* a missing devices view just means empty selects */
      });
    return () => controller.abort();
  }, []);

  const startMs = useMemo(() => toMs(startStr), [startStr]);
  const endMs = useMemo(() => toMs(endStr), [endStr]);

  const facetsActive =
    sources.length > 0 ||
    recordType !== null ||
    responseCode !== null ||
    resolver !== null ||
    device !== null ||
    group !== null ||
    startMs !== null ||
    endMs !== null;

  // Memoised so its identity is stable between renders: the history hook keys
  // its first-page load on this object.
  const historyFilter = useMemo<HistoryFilter>(
    () => ({ sources, recordType, responseCode, resolver, device, group, startMs, endMs, q: query }),
    [sources, recordType, responseCode, resolver, device, group, startMs, endMs, query],
  );

  const liveState = useLogs(query, live, !facetsActive);
  const history = useHistory(historyFilter, facetsActive);

  function toggleSource(source: QuerySource) {
    setSources((prev) =>
      prev.includes(source) ? prev.filter((value) => value !== source) : [...prev, source],
    );
  }

  function resetFilters() {
    setSources([]);
    setRecordType(null);
    setResponseCode(null);
    setResolver(null);
    setDevice(null);
    setGroup(null);
    setStartStr("");
    setEndStr("");
  }

  const term = query.trim();
  const searching = term.length > 0;

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <div className={styles.headline}>
          <h1 className={styles.title}>{t("logs.title")}</h1>
          <label className={styles.live}>
            <span className={styles.liveLabel} data-on={live && !facetsActive}>
              <span className={styles.liveDot} aria-hidden="true" />
              {facetsActive ? t("logs.paused") : live ? t("logs.live") : t("logs.paused")}
            </span>
            <Toggle
              checked={live && !facetsActive}
              onChange={setLive}
              label={t("logs.liveToggle")}
              disabled={facetsActive}
            />
          </label>
        </div>
        <p className={styles.scopeNote}>{t("logs.scopeNote")}</p>
        <input
          type="search"
          className={styles.input}
          placeholder={t("logs.searchPlaceholder")}
          value={input}
          onChange={(event) => setInput(event.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
      </header>

      <div className={styles.explorer}>
        <aside className={styles.filters} aria-label={t("logs.filters")}>
          <div className={styles.filtersHead}>
            <h2 className={styles.filtersTitle}>{t("logs.filters")}</h2>
            {facetsActive && (
              <button type="button" className={styles.reset} onClick={resetFilters}>
                {t("logs.filterReset")}
              </button>
            )}
          </div>

          <fieldset className={styles.facet}>
            <legend className={styles.facetLabel}>{t("logs.filterSource")}</legend>
            {SOURCE_FACETS.map((source) => (
              <label key={source} className={styles.check}>
                <input
                  type="checkbox"
                  checked={sources.includes(source)}
                  onChange={() => toggleSource(source)}
                />
                <span className={styles.source} data-source={source}>
                  {sourceLabel(source, lang)}
                </span>
              </label>
            ))}
          </fieldset>

          <label className={styles.facet}>
            <span className={styles.facetLabel}>{t("logs.filterType")}</span>
            <select
              className={styles.select}
              value={recordType ?? ""}
              onChange={(event) =>
                setRecordType(event.target.value === "" ? null : Number(event.target.value))
              }
            >
              <option value="">{t("logs.filterAny")}</option>
              {RECORD_TYPE_OPTIONS.map((code) => (
                <option key={code} value={code}>
                  {recordTypeLabel(code)}
                </option>
              ))}
            </select>
          </label>

          <label className={styles.facet}>
            <span className={styles.facetLabel}>{t("logs.filterCode")}</span>
            <select
              className={styles.select}
              value={responseCode ?? ""}
              onChange={(event) =>
                setResponseCode(event.target.value === "" ? null : Number(event.target.value))
              }
            >
              <option value="">{t("logs.filterAny")}</option>
              {RESPONSE_CODE_OPTIONS.map((code) => (
                <option key={code} value={code}>
                  {responseCodeLabel(code)}
                </option>
              ))}
            </select>
          </label>

          <label className={styles.facet}>
            <span className={styles.facetLabel}>{t("logs.filterResolver")}</span>
            <select
              className={styles.select}
              value={resolver ?? ""}
              onChange={(event) => setResolver(event.target.value === "" ? null : event.target.value)}
              disabled={resolverOptions.length === 0}
            >
              <option value="">{t("logs.filterAny")}</option>
              {resolverOptions.map((option) => (
                <option key={option.resolver} value={option.resolver}>
                  {option.resolver}
                </option>
              ))}
            </select>
          </label>

          <label className={styles.facet}>
            <span className={styles.facetLabel}>{t("logs.filterDeviceGroup")}</span>
            <select
              className={styles.select}
              value={device != null ? `d:${device}` : group != null ? `g:${group}` : ""}
              onChange={(event) => {
                const value = event.target.value;
                if (value.startsWith("g:")) {
                  setGroup(value.slice(2));
                  setDevice(null);
                } else if (value.startsWith("d:")) {
                  setDevice(value.slice(2));
                  setGroup(null);
                } else {
                  setDevice(null);
                  setGroup(null);
                }
              }}
              disabled={deviceOptions.length === 0 && groupOptions.length === 0}
            >
              <option value="">{t("logs.filterAny")}</option>
              {groupOptions.length > 0 && (
                <optgroup label={t("logs.filterGroups")}>
                  {groupOptions.map((option) => (
                    <option key={option.id} value={`g:${option.id}`}>
                      {option.name}
                    </option>
                  ))}
                </optgroup>
              )}
              {deviceOptions.length > 0 && (
                <optgroup label={t("logs.filterDevices")}>
                  {deviceOptions.map((option) => (
                    <option key={option.target} value={`d:${option.target}`}>
                      {option.name}
                    </option>
                  ))}
                </optgroup>
              )}
            </select>
          </label>

          <label className={styles.facet}>
            <span className={styles.facetLabel}>{t("logs.filterFrom")}</span>
            <input
              type="datetime-local"
              className={styles.select}
              value={startStr}
              onChange={(event) => setStartStr(event.target.value)}
            />
          </label>

          <label className={styles.facet}>
            <span className={styles.facetLabel}>{t("logs.filterTo")}</span>
            <input
              type="datetime-local"
              className={styles.select}
              value={endStr}
              onChange={(event) => setEndStr(event.target.value)}
            />
          </label>
        </aside>

        <section className={styles.results}>
          {facetsActive ? (
            <HistoryResults
              state={history}
              openKey={openKey}
              setOpenKey={setOpenKey}
            />
          ) : (
            <LiveResults
              state={liveState}
              openKey={openKey}
              setOpenKey={setOpenKey}
              searching={searching}
              term={term}
            />
          )}
        </section>
      </div>
    </div>
  );
}

/** The ring-backed live view: the newest entries, polled while live is on. */
function LiveResults({
  state,
  openKey,
  setOpenKey,
  searching,
  term,
}: {
  state: ReturnType<typeof useLogs>;
  openKey: string | null;
  setOpenKey: (key: string | null) => void;
  searching: boolean;
  term: string;
}) {
  const { t } = useI18n();
  const { rows, error, loading } = state;

  if (rows === null) {
    return (
      <p className={styles.note}>
        {error ? t("logs.unreachable", { error }) : loading ? t("logs.loading") : t("common.noData")}
      </p>
    );
  }
  if (rows.length === 0) {
    return (
      <p className={styles.note}>
        {searching ? t("logs.noMatches", { term }) : t("logs.noneRecorded")}
      </p>
    );
  }
  return (
    <>
      <p className={styles.count}>
        {searching
          ? rows.length === LOG_LIMIT
            ? t("logs.matchesPlus", { n: rows.length })
            : t("logs.matches", { n: rows.length })
          : t("logs.recent", { n: rows.length })}
        {error ? t("logs.updatesInterrupted") : ""}
      </p>
      <LogList rows={rows} openKey={openKey} setOpenKey={setOpenKey} />
    </>
  );
}

/** The DuckDB-backed history view: filtered results with "load older" paging. */
function HistoryResults({
  state,
  openKey,
  setOpenKey,
}: {
  state: ReturnType<typeof useHistory>;
  openKey: string | null;
  setOpenKey: (key: string | null) => void;
}) {
  const { t } = useI18n();
  const { rows, error, loading, loadingMore, hasMore, loadMore } = state;

  if (rows === null) {
    return (
      <p className={styles.note}>
        {error ? t("logs.historyError", { error }) : t("logs.loading")}
      </p>
    );
  }
  if (rows.length === 0) {
    return (
      <p className={styles.note}>{error ? t("logs.historyError", { error }) : t("logs.historyEmpty")}</p>
    );
  }
  return (
    <>
      <p className={styles.hint}>{t("logs.historyHint")}</p>
      <p className={styles.count}>
        {t("logs.historyShown", { n: rows.length })}
        {error ? t("logs.historyError", { error }) : ""}
      </p>
      <LogList rows={rows} openKey={openKey} setOpenKey={setOpenKey} showDate />
      {hasMore ? (
        <button
          type="button"
          className={styles.loadMore}
          onClick={loadMore}
          disabled={loadingMore || loading}
        >
          {loadingMore ? t("logs.loadingOlder") : t("logs.loadOlder")}
        </button>
      ) : (
        <p className={styles.endNote}>{t("logs.historyEnd")}</p>
      )}
    </>
  );
}

/** The shared results list: one grid, one expandable row per entry. In the
 *  history view the rows span several days, so the Zeit cell also shows the
 *  date; the live view is always "today", so it stays time-only. */
function LogList({
  rows,
  openKey,
  setOpenKey,
  showDate = false,
}: {
  rows: LogRow[];
  openKey: string | null;
  setOpenKey: (key: string | null) => void;
  showDate?: boolean;
}) {
  const { t } = useI18n();
  const { nodeLabels, selfLabel, showNodes } = useNodeLabels();
  return (
    <ol className={styles.list}>
      <li className={styles.columns} aria-hidden="true">
        <span>{t("logs.colTime")}</span>
        <span>{t("logs.colClient")}</span>
        <span>{t("logs.colDomain")}</span>
        <span className={styles.detailsHead}>{t("logs.colDetails")}</span>
      </li>
      {rows.map((row, index) => {
        const key = `${row.at_ms}-${index}`;
        const open = openKey === key;
        return (
          <li key={key} className={styles.item} data-blocked={row.isBlocked || undefined}>
            <button
              type="button"
              className={styles.row}
              aria-expanded={open}
              onClick={() => setOpenKey(open ? null : key)}
            >
              <span className={styles.time}>
                {showDate && <span className={styles.date}>{row.date}</span>}
                {row.time}
              </span>
              <span className={styles.client} title={row.deviceName ? row.client : undefined}>
                {row.deviceName ?? row.client}
              </span>
              <span className={styles.domainCell}>
                <span className={styles.domain} title={row.name}>
                  {row.name}
                </span>
                {row.addresses.length > 0 ? (
                  <span className={styles.resolved} title={row.addresses.join(", ")}>
                    <span className={styles.resolvedArrow} aria-hidden="true">
                      →
                    </span>{" "}
                    {row.addresses.join(", ")}
                  </span>
                ) : null}
              </span>
              <span className={styles.meta}>
                {showNodes &&
                  (() => {
                    const isSelf = !row.origin;
                    const label = isSelf
                      ? selfLabel
                      : (nodeLabels.get(row.origin!) ?? row.origin!.slice(0, 8));
                    if (!label) return null;
                    return (
                      <span
                        className={styles.node}
                        data-self={isSelf || undefined}
                        title={`${t("logs.nodeBadge")}: ${label}${
                          isSelf ? ` (${t("logs.thisNode")})` : ""
                        }`}
                      >
                        {label}
                      </span>
                    );
                  })()}
                <span className={styles.type}>{row.type}</span>
                <span className={styles.source} data-source={row.source}>
                  {row.sourceText}
                </span>
                {row.rcodeTone !== "none" && (
                  <span className={styles.rcode} data-tone={row.rcodeTone}>
                    {row.rcode}
                  </span>
                )}
                <span className={styles.duration}>{row.durationMs}&thinsp;ms</span>
                <span className={styles.chevron} data-open={open || undefined} aria-hidden="true">
                  ›
                </span>
              </span>
            </button>
            {open && <LogDetail row={row} />}
          </li>
        );
      })}
    </ol>
  );
}
