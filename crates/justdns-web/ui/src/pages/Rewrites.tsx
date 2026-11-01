import { useEffect, useState, type FormEvent } from "react";

import { InfoHint } from "../components/InfoHint";
import { ApiError, api, type Rewrite, type RewritesView } from "../lib/api";
import styles from "./Rewrites.module.css";

/** Turn a mutation's failure into a short German message for the banner. */
function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 400) return "Name oder IP-Adresse ist ungültig.";
    if (error.status === 404) return "Der Eintrag wurde nicht gefunden.";
  }
  return "Die Änderung ließ sich nicht speichern.";
}

/** Split a free-text IP field into separate addresses: entries are separated by
 *  commas or whitespace, so "192.168.20.5, 192.168.20.6" and one-per-space both
 *  work. Empty fragments are dropped. */
function splitIps(raw: string): string[] {
  return raw
    .split(/[\s,]+/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

export function Rewrites() {
  const [view, setView] = useState<RewritesView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());

  const [name, setName] = useState("");
  const [ips, setIps] = useState("");

  useEffect(() => {
    const controller = new AbortController();
    api
      .rewrites(controller.signal)
      .then(setView)
      .catch(() => {
        if (controller.signal.aborted) return;
        setError("Die Rewrites konnten nicht geladen werden.");
      });
    return () => controller.abort();
  }, []);

  function mark(key: string, on: boolean) {
    setPending((prev) => {
      const next = new Set(prev);
      if (on) next.add(key);
      else next.delete(key);
      return next;
    });
  }

  async function run(key: string, action: () => Promise<RewritesView>) {
    mark(key, true);
    setError(null);
    try {
      setView(await action());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      mark(key, false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const host = name.trim();
    const addresses = splitIps(ips);
    if (!host || addresses.length === 0) return;
    run(`set:${host}`, () => api.setRewrite(host, addresses)).then(() => {
      setName("");
      setIps("");
    });
  }

  if (!view) {
    return (
      <div className={styles.page}>
        <h1 className={styles.title}>DNS-Rewrites</h1>
        <p className={styles.lead}>{error ?? "Lädt …"}</p>
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <div>
          <h1 className={styles.title}>
            DNS-Rewrites
            <InfoHint
              label="DNS-Rewrites"
              text="Lass JustDNS einen Namen selbst beantworten, statt ihn an den Upstream weiterzugeben — etwa um einen lokalen Dienst über einen festen Namen erreichbar zu machen (nas.home → 192.168.20.5). Mehrere IPs sind erlaubt; IPv4 beantwortet A-Abfragen, IPv6 beantwortet AAAA-Abfragen. Es greift nur der exakte Name; Subdomains trägst du einzeln ein."
            />
          </h1>
          <p className={styles.lead}>
            Feste Zuordnungen von Name zu IP-Adresse, die JustDNS lokal beantwortet. Ein Rewrite
            gewinnt vor jeder Sperrliste und erreicht den Upstream nie.
          </p>
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
          placeholder="Name, z. B. nas.home"
          aria-label="Name"
          autoComplete="off"
          spellCheck={false}
        />
        <input
          className={styles.input}
          value={ips}
          onChange={(event) => setIps(event.target.value)}
          placeholder="IP-Adresse(n), z. B. 192.168.20.5"
          aria-label="IP-Adressen"
          autoComplete="off"
          spellCheck={false}
        />
        <button type="submit" className={styles.addButton} disabled={pending.size > 0}>
          Speichern
        </button>
      </form>

      {view.rewrites.length === 0 ? (
        <p className={styles.empty}>Noch keine Rewrites angelegt.</p>
      ) : (
        <ul className={styles.entries}>
          {view.rewrites.map((rewrite) => (
            <RewriteRow
              key={rewrite.name}
              rewrite={rewrite}
              busy={pending.has(`set:${rewrite.name}`)}
              onRemove={() => run(`set:${rewrite.name}`, () => api.removeRewrite(rewrite.name))}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

/** One rewrite: the name, its addresses as chips, and a remove control. */
function RewriteRow({
  rewrite,
  busy,
  onRemove,
}: {
  rewrite: Rewrite;
  busy: boolean;
  onRemove: () => void;
}) {
  return (
    <li className={styles.entry}>
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
      <button
        type="button"
        className={styles.remove}
        aria-label={`${rewrite.name} entfernen`}
        title="Entfernen"
        disabled={busy}
        onClick={onRemove}
      >
        ✕
      </button>
    </li>
  );
}
