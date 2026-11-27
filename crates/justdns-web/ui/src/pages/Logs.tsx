import { useEffect, useMemo, useRef, useState } from "react";

import { Toggle } from "../components/Toggle";
import {
  api,
  type ClientRow,
  type Group,
  type HistoryFilter,
  type NodeInfo,
  type PhishingQuery,
  type QuerySource,
  type ResolverStat,
} from "../lib/api";
import { RECORD_TYPES, RESPONSE_CODES, recordTypeLabel, responseCodeLabel, sourceLabel } from "../lib/dns";
import { countryName, describeGeo } from "../lib/geo";
import { useI18n } from "../lib/i18n";
import { REASON_KEY } from "../lib/trace";
import { LOG_LIMIT, useHistory, useLogs, type LogRow } from "../lib/useLogs";
import { LogDetail } from "./LogDetail";
import styles from "./Logs.module.css";

/** Wait this long after the last keystroke before filtering, so typing a word
 *  does not fire a request per character. */
const SEARCH_DEBOUNCE_MS = 250;

/** The source facets, in the order they read as pills: the signal worth seeing
 *  first, then the ordinary outcomes. */
const SOURCE_FACETS: QuerySource[] = ["blocked", "cache", "upstream", "local", "rewrite"];

/** The phishing filter options, in the order the select lists them. `null` is
 *  "no phishing filter". */
const PHISHING_OPTIONS: (PhishingQuery | null)[] = [null, "any", "warn", "block"];

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

/** The phishing filter: a pill trigger with a themed popover, so the choice list
 *  matches the dark UI instead of the browser's native (OS-styled) select. Behaves
 *  like a listbox — open on click or arrow key, pick with click or Enter, close on
 *  Escape or an outside click, and the active row carries a check. */
function PhishingFilter({
  value,
  onChange,
}: {
  value: PhishingQuery | null;
  onChange: (next: PhishingQuery | null) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  // While open, dismiss on an outside click or Escape. The listeners live only for
  // the open window so a closed menu costs nothing.
  useEffect(() => {
    if (!open) return;
    const onPointer = (event: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  // On open, land focus on the current choice so keyboard users start from where
  // they are, not the top of the list.
  useEffect(() => {
    if (!open) return;
    wrapRef.current
      ?.querySelector<HTMLButtonElement>("[role='option'][data-selected]")
      ?.focus();
  }, [open]);

  const optionLabel = (option: PhishingQuery | null) =>
    option === null
      ? t("logs.phishingOff")
      : option === "any"
        ? t("logs.phishingAny")
        : option === "warn"
          ? t("logs.phishingWarn")
          : t("logs.phishingBlock");

  const triggerLabel = value === null ? t("logs.filterPhishing") : optionLabel(value);

  // Roving focus across the option rows for the arrow keys.
  const moveFocus = (delta: number) => {
    const items = wrapRef.current?.querySelectorAll<HTMLButtonElement>("[role='option']");
    if (!items || items.length === 0) return;
    const list = Array.from(items);
    const current = list.findIndex((el) => el === document.activeElement);
    const next = (current + delta + list.length) % list.length;
    list[next]?.focus();
  };

  return (
    <div className={styles.phishingFilter} ref={wrapRef}>
      <button
        type="button"
        className={styles.pillSelect}
        data-on={value !== null || undefined}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls="phishing-filter-listbox"
        aria-label={t("logs.filterPhishing")}
        onClick={() => setOpen((prev) => !prev)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span
          className={styles.pillDot}
          data-phishing
          data-mode={value ?? "off"}
          aria-hidden="true"
        />
        <span className={styles.pillSelectLabel}>{triggerLabel}</span>
        <span className={styles.pillCaret} data-open={open || undefined} aria-hidden="true">
          ›
        </span>
      </button>
      {open && (
        <ul
          id="phishing-filter-listbox"
          className={styles.phishingPopover}
          role="listbox"
          aria-label={t("logs.filterPhishing")}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              moveFocus(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              moveFocus(-1);
            }
          }}
        >
          {PHISHING_OPTIONS.map((option) => {
            const selected = option === value;
            return (
              <li key={option ?? "off"} className={styles.phishingItem}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={styles.phishingOption}
                  data-selected={selected || undefined}
                  onClick={() => {
                    onChange(option);
                    setOpen(false);
                  }}
                >
                  <span
                    className={styles.phishingOptionDot}
                    data-mode={option ?? "off"}
                    aria-hidden="true"
                  />
                  <span className={styles.phishingOptionLabel}>{optionLabel(option)}</span>
                  {selected && (
                    <span className={styles.phishingOptionCheck} aria-hidden="true">
                      ✓
                    </span>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

export function Logs() {
  const { t, lang } = useI18n();
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [live, setLive] = useState(true);
  const [openKey, setOpenKey] = useState<string | null>(null);
  // The secondary filters (type, code, resolver, device, geo, time) fold behind
  // a disclosure so the bar stays a search plus a row of source pills by default.
  const [moreOpen, setMoreOpen] = useState(false);

  // Facet filters. Any one of them switches the page into history mode.
  const [sources, setSources] = useState<QuerySource[]>([]);
  const [phishing, setPhishing] = useState<PhishingQuery | null>(null);
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
  // Country (ISO 3166-1 alpha-2) and ASN refine the *live* feed only: the geo
  // lookup runs on the read path over the hot ring, and durable history has no
  // geo index, so these stay separate from the history facets below.
  const [geoCountry, setGeoCountry] = useState("");
  const [geoAsn, setGeoAsn] = useState("");

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

  // Normalise the live geo filter: an upper-cased country code and a parsed ASN,
  // each left undefined when blank or unparseable so the feed is not narrowed.
  const geoCountryVal = geoCountry.trim().toUpperCase() || undefined;
  const geoAsnVal = useMemo(() => {
    const raw = geoAsn.trim();
    if (raw === "") return undefined;
    const n = Number(raw);
    return Number.isInteger(n) && n >= 0 ? n : undefined;
  }, [geoAsn]);

  const facetsActive =
    sources.length > 0 ||
    phishing !== null ||
    recordType !== null ||
    responseCode !== null ||
    resolver !== null ||
    device !== null ||
    group !== null ||
    startMs !== null ||
    endMs !== null;

  // How many of the folded-away filters are set, so the disclosure can show a
  // count without opening (the source pills and phishing live outside it). The
  // geo filter only counts while it actually applies: a history facet disables
  // it (it is live-only), so counting it then would overstate what is in effect.
  const moreActiveCount =
    (recordType !== null ? 1 : 0) +
    (responseCode !== null ? 1 : 0) +
    (resolver !== null ? 1 : 0) +
    (device !== null || group !== null ? 1 : 0) +
    (startMs !== null || endMs !== null ? 1 : 0) +
    (!facetsActive && (geoCountryVal !== undefined || geoAsnVal !== undefined) ? 1 : 0);

  // Memoised so its identity is stable between renders: the history hook keys
  // its first-page load on this object.
  const historyFilter = useMemo<HistoryFilter>(
    () => ({
      sources,
      phishing,
      recordType,
      responseCode,
      resolver,
      device,
      group,
      startMs,
      endMs,
      q: query,
    }),
    [sources, phishing, recordType, responseCode, resolver, device, group, startMs, endMs, query],
  );

  // A text search is a filter too, so it reads the full durable history (DuckDB),
  // not the in-memory ring: a name entered minutes later still turns up, instead
  // of ageing out of the hot ring the moment newer queries push it off.
  const searching = query.trim().length > 0;
  // A country/ASN filter applies only to the live feed, so when one is set a
  // search stays live (needle plus geo) instead of switching to the history view,
  // which has no geo filter and would silently drop it.
  const geoActive = geoCountryVal !== undefined || geoAsnVal !== undefined;
  const explore = facetsActive || (searching && !geoActive);

  const liveState = useLogs(query, live, !explore, geoCountryVal, geoAsnVal);
  const history = useHistory(historyFilter, explore);

  function toggleSource(source: QuerySource) {
    setSources((prev) =>
      prev.includes(source) ? prev.filter((value) => value !== source) : [...prev, source],
    );
  }

  function resetFilters() {
    setSources([]);
    setPhishing(null);
    setRecordType(null);
    setResponseCode(null);
    setResolver(null);
    setDevice(null);
    setGroup(null);
    setStartStr("");
    setEndStr("");
    setGeoCountry("");
    setGeoAsn("");
  }

  const term = query.trim();

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <div className={styles.headline}>
          <h1 className={styles.title}>{t("logs.title")}</h1>
          <label className={styles.live}>
            <span className={styles.liveLabel} data-on={live && !explore}>
              <span className={styles.liveDot} aria-hidden="true" />
              {explore ? t("logs.paused") : live ? t("logs.live") : t("logs.paused")}
            </span>
            <Toggle
              checked={live && !explore}
              onChange={setLive}
              label={t("logs.liveToggle")}
              disabled={explore}
            />
          </label>
        </div>
        <p className={styles.scopeNote}>{t("logs.scopeNote")}</p>

        <div className={styles.search}>
          <svg
            className={styles.searchIcon}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="7" />
            <path d="M16.5 16.5L21 21" />
          </svg>
          <input
            type="search"
            className={styles.searchInput}
            placeholder={t("logs.searchPlaceholder")}
            value={input}
            onChange={(event) => setInput(event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </div>

        <div className={styles.filterBar} aria-label={t("logs.filters")}>
          {SOURCE_FACETS.map((source) => {
            const on = sources.includes(source);
            return (
              <button
                key={source}
                type="button"
                className={styles.pill}
                data-on={on || undefined}
                aria-pressed={on}
                onClick={() => toggleSource(source)}
              >
                <span className={styles.pillDot} data-source={source} aria-hidden="true" />
                {sourceLabel(source, lang)}
              </button>
            );
          })}

          <PhishingFilter value={phishing} onChange={setPhishing} />

          <button
            type="button"
            className={styles.moreButton}
            data-on={moreActiveCount > 0 || undefined}
            aria-expanded={moreOpen}
            onClick={() => setMoreOpen((value) => !value)}
          >
            {t("logs.moreFilters")}
            {moreActiveCount > 0 ? <span className={styles.moreCount}>{moreActiveCount}</span> : null}
            <span className={styles.caret} data-open={moreOpen || undefined} aria-hidden="true">
              ›
            </span>
          </button>

          {facetsActive && (
            <button type="button" className={styles.reset} onClick={resetFilters}>
              {t("logs.filterReset")}
            </button>
          )}
        </div>

        {moreOpen && (
          <div className={styles.morePanel}>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("logs.filterType")}</span>
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

            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("logs.filterCode")}</span>
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

            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("logs.filterResolver")}</span>
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

            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("logs.filterDeviceGroup")}</span>
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

            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("logs.filterFrom")}</span>
              <input
                type="datetime-local"
                className={styles.select}
                value={startStr}
                onChange={(event) => setStartStr(event.target.value)}
              />
            </label>

            <label className={styles.field}>
              <span className={styles.fieldLabel}>{t("logs.filterTo")}</span>
              <input
                type="datetime-local"
                className={styles.select}
                value={endStr}
                onChange={(event) => setEndStr(event.target.value)}
              />
            </label>

            <fieldset className={styles.geoField} disabled={facetsActive}>
              <legend className={styles.fieldLabel}>{t("logs.filterGeo")}</legend>
              <div className={styles.geoInputs}>
                <input
                  type="text"
                  className={styles.select}
                  value={geoCountry}
                  onChange={(event) => setGeoCountry(event.target.value)}
                  placeholder={t("logs.filterGeoCountry")}
                  maxLength={2}
                  autoComplete="off"
                  spellCheck={false}
                />
                <input
                  type="number"
                  className={styles.select}
                  value={geoAsn}
                  onChange={(event) => setGeoAsn(event.target.value)}
                  placeholder={t("logs.filterGeoAsn")}
                  min={0}
                  step={1}
                  autoComplete="off"
                />
              </div>
              <span className={styles.fieldHint}>
                {facetsActive ? t("logs.filterGeoLiveOnly") : t("logs.filterGeoHint")}
              </span>
            </fieldset>
          </div>
        )}
      </header>

      <section className={styles.results}>
        {explore ? (
          <HistoryResults
            state={history}
            openKey={openKey}
            setOpenKey={setOpenKey}
            searching={searching}
            term={term}
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
  searching,
  term,
}: {
  state: ReturnType<typeof useHistory>;
  openKey: string | null;
  setOpenKey: (key: string | null) => void;
  searching: boolean;
  term: string;
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
    const empty = searching ? t("logs.noMatches", { term }) : t("logs.historyEmpty");
    return <p className={styles.note}>{error ? t("logs.historyError", { error }) : empty}</p>;
  }
  return (
    <>
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

/** The stream: one expandable row per entry. A colour-coded dot carries the
 *  source, the domain leads, and only the tags a row actually has (phishing,
 *  geo, the answering node) show up — so an ordinary row stays quiet. In the
 *  history view the rows span several days, so the time also shows the date. */
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
  const { t, lang } = useI18n();
  const { nodeLabels, selfLabel, showNodes } = useNodeLabels();
  return (
    <ol className={styles.list}>
      {rows.map((row, index) => {
        const key = `${row.at_ms}-${index}`;
        const open = openKey === key;
        return (
          <li
            key={key}
            className={styles.item}
            data-blocked={row.isBlocked || undefined}
            data-watched={row.watched || undefined}
          >
            <button
              type="button"
              className={styles.row}
              aria-expanded={open}
              onClick={() => setOpenKey(open ? null : key)}
            >
              <span
                className={styles.dot}
                data-source={row.source}
                title={row.sourceText}
                aria-label={row.sourceText}
              />
              <span className={styles.time}>
                {showDate && <span className={styles.date}>{row.date}</span>}
                {row.time}
              </span>
              <span className={styles.client} title={row.deviceName ? row.client : undefined}>
                {row.deviceName ?? row.client}
              </span>
              <span className={styles.domainCell}>
                <span className={styles.domainLine}>
                  <span className={styles.domain} title={row.name}>
                    {row.name}
                  </span>
                  {row.phishing &&
                    (() => {
                      const kindLabel =
                        row.phishing.kind === "lookalike"
                          ? t("phishing.kindLookalike")
                          : row.phishing.kind === "punycode"
                            ? t("phishing.kindPunycode")
                            : t("phishing.kindNrd");
                      const verdict = row.phishing.blocked
                        ? t("phishing.badgeBlock")
                        : t("phishing.badgeWarn");
                      const detail = row.phishing.brand
                        ? `${kindLabel} · ${row.phishing.brand}`
                        : kindLabel;
                      return (
                        <span
                          className={styles.phishingPill}
                          data-mode={row.phishing.blocked ? "block" : "warn"}
                          title={`${t("phishing.title")}: ${detail} · ${verdict}`}
                        >
                          {t("phishing.badge")}
                        </span>
                      );
                    })()}
                  {!row.phishing && row.reason && (
                    <span
                      className={styles.reasonPill}
                      data-reason={row.reason}
                      title={t(REASON_KEY[row.reason])}
                    >
                      {t(REASON_KEY[row.reason])}
                    </span>
                  )}
                </span>
                {row.addresses.length > 0 ? (
                  <span className={styles.resolved} title={row.addresses.join(", ")}>
                    <span className={styles.resolvedArrow} aria-hidden="true">
                      →
                    </span>{" "}
                    {row.addresses.join(", ")}
                  </span>
                ) : null}
                {row.networks && row.networks.length > 0 ? (
                  <span className={styles.networks}>
                    {row.watched && (
                      <span className={styles.watchedBadge} title={t("logs.watchedHint")}>
                        {t("logs.watchedBadge")}
                      </span>
                    )}
                    {row.networks.map((net, i) => (
                      <span
                        key={i}
                        className={styles.network}
                        title={
                          net.asn != null
                            ? `${countryName(net.country, lang)} · AS${net.asn}${
                                net.org ? ` ${net.org}` : ""
                              }`
                            : countryName(net.country, lang)
                        }
                      >
                        {describeGeo(net, lang)}
                      </span>
                    ))}
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
