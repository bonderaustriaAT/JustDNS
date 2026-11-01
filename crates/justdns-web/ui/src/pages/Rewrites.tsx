import { useEffect, useState, type FormEvent } from "react";

import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
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

  async function run(key: string, action: () => Promise<RewritesView>): Promise<boolean> {
    mark(key, true);
    setError(null);
    try {
      setView(await action());
      return true;
    } catch (err) {
      setError(errorMessage(err));
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
        <h1 className={styles.title}>DNS-Rewrites</h1>
        <p className={styles.lead}>{error ?? "Lädt …"}</p>
      </div>
    );
  }

  const busy = pending.size > 0;

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
          placeholder={editing ? "Name" : "Name(n), z. B. nas.home drucker.home"}
          aria-label={editing ? "Name" : "Name oder mehrere Namen"}
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
        <button type="submit" className={styles.addButton} disabled={busy}>
          {editing ? "Aktualisieren" : "Speichern"}
        </button>
        {editing ? (
          <button
            type="button"
            className={styles.cancelButton}
            onClick={resetForm}
            disabled={busy}
          >
            Abbrechen
          </button>
        ) : null}
      </form>
      {!editing ? (
        <p className={styles.hint}>
          Mehrere Namen durch Leerzeichen oder Komma trennen, um sie auf dieselbe IP zu zeigen.
        </p>
      ) : null}

      {view.rewrites.length === 0 ? (
        <p className={styles.empty}>Noch keine Rewrites angelegt.</p>
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
  return (
    <li className={styles.entry} data-editing={editing || undefined} data-off={!rewrite.enabled || undefined}>
      <div className={styles.entryToggle}>
        <Toggle
          label={`${rewrite.name} ${rewrite.enabled ? "deaktivieren" : "aktivieren"}`}
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
          aria-label={`${rewrite.name} bearbeiten`}
          title="Bearbeiten"
          disabled={busy}
          onClick={onEdit}
        >
          Bearbeiten
        </button>
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
      </div>
    </li>
  );
}
