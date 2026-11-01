import { useEffect, useState } from "react";

import { Toggle } from "../components/Toggle";
import { LOG_LIMIT, useLogs } from "../lib/useLogs";
import { LogDetail } from "./LogDetail";
import styles from "./Logs.module.css";

/** Wait this long after the last keystroke before filtering, so typing a word
 *  does not fire a request per character. */
const SEARCH_DEBOUNCE_MS = 250;

export function Logs() {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [live, setLive] = useState(true);
  const [openKey, setOpenKey] = useState<string | null>(null);

  useEffect(() => {
    const id = setTimeout(() => setQuery(input), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [input]);

  const { rows, error, loading } = useLogs(query, live);
  const term = query.trim();
  const searching = term.length > 0;

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <div className={styles.headline}>
          <h1 className={styles.title}>DNS-Protokoll</h1>
          <label className={styles.live}>
            <span className={styles.liveLabel} data-on={live}>
              <span className={styles.liveDot} aria-hidden="true" />
              {live ? "Live" : "Pausiert"}
            </span>
            <Toggle checked={live} onChange={setLive} label="Live-Aktualisierung" />
          </label>
        </div>
        <input
          type="search"
          className={styles.input}
          placeholder="Nach Domain oder IP filtern …"
          value={input}
          onChange={(event) => setInput(event.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
      </header>

      {rows === null ? (
        <p className={styles.note}>
          {error ? `Protokoll nicht erreichbar: ${error}` : loading ? "Lade Protokoll …" : "Keine Daten."}
        </p>
      ) : rows.length === 0 ? (
        <p className={styles.note}>
          {searching ? `Keine Treffer für »${term}«.` : "Noch keine Anfragen aufgezeichnet."}
        </p>
      ) : (
        <>
          <p className={styles.count}>
            {searching
              ? `${rows.length}${rows.length === LOG_LIMIT ? "+" : ""} Treffer`
              : `Neueste ${rows.length} Anfragen`}
            {error ? " · Aktualisierung unterbrochen" : ""}
          </p>
          <ol className={styles.list}>
            <li className={styles.columns} aria-hidden="true">
              <span>Zeit</span>
              <span>Client</span>
              <span>Domain</span>
              <span className={styles.detailsHead}>Details</span>
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
                    <span className={styles.time}>{row.time}</span>
                    <span className={styles.client}>{row.client}</span>
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
        </>
      )}
    </div>
  );
}
