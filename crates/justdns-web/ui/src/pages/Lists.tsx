import { useEffect, useState, type FormEvent, type ReactNode } from "react";

import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import {
  ApiError,
  api,
  type ListKind,
  type ListsView,
  type Preset,
  type Subscription,
} from "../lib/api";
import styles from "./Lists.module.css";

/** Turn a mutation's failure into a short German message for the banner. */
function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 400) return "Das ist keine gültige Domain oder URL.";
    if (error.status === 404) return "Der Eintrag wurde nicht gefunden.";
  }
  return "Die Änderung ließ sich nicht speichern.";
}

/** Relative time for a feed's last update, e.g. "vor 2 Std.". */
function relativeTime(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 60_000) return "gerade aktualisiert";
  const minutes = Math.round(diff / 60_000);
  if (minutes < 60) return `vor ${minutes} Min.`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `vor ${hours} Std.`;
  const days = Math.round(hours / 24);
  return `vor ${days} ${days === 1 ? "Tag" : "Tagen"}`;
}

const RISK_WORDS = ["sehr gering", "gering", "mittel", "erhöht", "hoch"];

/** A five-pip meter for a list's false-positive risk: how much legitimate
 *  traffic it might catch by mistake. The subject here is risk, so it reads as
 *  a scale, not a badge. */
function RiskMeter({ level }: { level: number }) {
  const clamped = Math.max(1, Math.min(5, level));
  return (
    <span
      className={styles.meter}
      data-level={clamped}
      role="img"
      aria-label={`False-Positive-Risiko: ${RISK_WORDS[clamped - 1]}`}
    >
      {[1, 2, 3, 4, 5].map((pip) => (
        <span key={pip} className={styles.pip} data-on={pip <= clamped || undefined} />
      ))}
    </span>
  );
}

/** An editor for one manual list: an add field over a list of removable rows.
 *  Deliberately not a free-text box — each entry is its own row with its own
 *  remove control, so the list stays legible and a single typo can't wipe it. */
function EntryEditor({
  title,
  hint,
  accent,
  placeholder,
  entries,
  count,
  countNoun,
  onAdd,
  onRemove,
}: {
  title: string;
  hint: string;
  accent: "allow" | "block";
  placeholder: string;
  entries: string[];
  count: number;
  countNoun: string;
  onAdd: (domain: string) => void;
  onRemove: (domain: string) => void;
}) {
  const [draft, setDraft] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    const domain = draft.trim();
    if (!domain) return;
    onAdd(domain);
    setDraft("");
  }

  return (
    <section className={styles.editor} data-accent={accent} aria-labelledby={`${accent}-title`}>
      <div className={styles.editorHead}>
        <h3 id={`${accent}-title`} className={styles.editorTitle}>
          {title}
          <InfoHint label={title} text={hint} />
        </h3>
        <span className={styles.editorCount}>
          {count.toLocaleString("de-DE")} {countNoun} aktiv
        </span>
      </div>

      <form className={styles.addRow} onSubmit={submit}>
        <input
          className={styles.input}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={placeholder}
          aria-label={`${title}: Domain hinzufügen`}
          autoComplete="off"
          spellCheck={false}
        />
        <button type="submit" className={styles.addButton}>
          Hinzufügen
        </button>
      </form>

      {entries.length === 0 ? (
        <p className={styles.empty}>Noch keine eigenen Einträge.</p>
      ) : (
        <ul className={styles.entries}>
          {entries.map((domain) => (
            <li key={domain} className={styles.entry}>
              <span className={styles.entryName}>{domain}</span>
              <button
                type="button"
                className={styles.remove}
                aria-label={`${domain} entfernen`}
                title="Entfernen"
                onClick={() => onRemove(domain)}
              >
                ✕
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** One card in the curated catalogue, with its description, risk meter, live
 *  status and the one-click toggle. */
function PresetCard({
  preset,
  sub,
  pending,
  onToggle,
}: {
  preset: Preset;
  sub?: Subscription;
  pending: boolean;
  onToggle: (on: boolean) => void;
}) {
  return (
    <article className={styles.card} data-active={preset.active || undefined}>
      <header className={styles.cardHead}>
        <div className={styles.cardTitleWrap}>
          <h3 className={styles.cardTitle}>{preset.name}</h3>
          <span className={styles.kind} data-kind={preset.kind}>
            {preset.kind === "allow" ? "Freigabe" : "Sperrliste"}
          </span>
        </div>
        <Toggle
          label={`${preset.name} aktivieren`}
          checked={preset.active}
          disabled={pending}
          onChange={onToggle}
        />
      </header>

      <p className={styles.cardText}>{preset.description}</p>

      <div className={styles.risk}>
        <RiskMeter level={preset.fpLevel} />
        <p className={styles.riskText}>{preset.falsePositive}</p>
      </div>

      {preset.active && sub ? <p className={styles.status}>{feedStatusText(sub)}</p> : null}
    </article>
  );
}

export function Lists() {
  const [view, setView] = useState<ListsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [refreshing, setRefreshing] = useState(false);

  // Hand-added subscription form.
  const [feedName, setFeedName] = useState("");
  const [feedUrl, setFeedUrl] = useState("");
  const [feedKind, setFeedKind] = useState<ListKind>("block");

  useEffect(() => {
    const controller = new AbortController();
    api
      .lists(controller.signal)
      .then(setView)
      .catch(() => {
        if (controller.signal.aborted) return;
        setError("Die Listen konnten nicht geladen werden.");
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

  /** Run a mutation, replace the view with its result, surface any error. */
  async function run(key: string, action: () => Promise<ListsView>) {
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

  async function refreshAll() {
    setRefreshing(true);
    setError(null);
    try {
      const response = await api.refreshLists();
      setView(response.lists);
      const failed = response.outcomes.filter((outcome) => outcome.error).length;
      if (failed > 0) {
        setError(`${failed} Liste(n) konnten nicht aktualisiert werden.`);
      }
    } catch {
      setError("Die Aktualisierung ist fehlgeschlagen.");
    } finally {
      setRefreshing(false);
    }
  }

  function addFeed(event: FormEvent) {
    event.preventDefault();
    const url = feedUrl.trim();
    if (!url) return;
    run(`add-feed`, () => api.addSubscription(feedName.trim(), url, feedKind)).then(() => {
      setFeedName("");
      setFeedUrl("");
    });
  }

  if (!view) {
    return (
      <div className={styles.page}>
        <h1 className={styles.title}>Listen</h1>
        <p className={styles.lead}>
          {error ?? "Lädt …"}
        </p>
      </div>
    );
  }

  const feeds = view.subscriptions.filter((sub) => !sub.preset);

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <div>
          <h1 className={styles.title}>Listen</h1>
          <p className={styles.lead}>
            Lege fest, was JustDNS für das ganze Netzwerk sperrt oder immer durchlässt. Eine
            Freigabe gewinnt dabei immer — ein freigegebener Name wird nie geblockt, egal was eine
            Sperrliste sagt.
          </p>
        </div>
        <button
          type="button"
          className={styles.refresh}
          onClick={refreshAll}
          disabled={refreshing}
        >
          {refreshing ? "Aktualisiert …" : "Alle Listen aktualisieren"}
        </button>
      </div>

      {error ? (
        <p className={styles.banner} role="alert">
          {error}
        </p>
      ) : null}

      <div className={styles.editors}>
        <EntryEditor
          title="Immer erlauben"
          hint="Domains auf dieser Liste werden nie geblockt — auch dann nicht, wenn sie auf einer aktiven Sperrliste stehen. Gut, um einen Fehlalarm (false positive) gezielt zu beheben."
          accent="allow"
          placeholder="z. B. meine-bank.de"
          entries={view.allow}
          count={view.allowActive}
          countNoun="Domains"
          onAdd={(domain) => run(`allow:${domain}`, () => api.addAllow(domain))}
          onRemove={(domain) => run(`allow:${domain}`, () => api.removeAllow(domain))}
        />
        <EntryEditor
          title="Immer blockieren"
          hint="Deine eigene Sperrliste. Domains hier werden immer geblockt, zusätzlich zu den aktivierten fertigen Listen. Eine Freigabe sticht eine Sperre."
          accent="block"
          placeholder="z. B. werbung.example.com"
          entries={view.block}
          count={view.blockActive}
          countNoun="Domains"
          onAdd={(domain) => run(`block:${domain}`, () => api.addBlock(domain))}
          onRemove={(domain) => run(`block:${domain}`, () => api.removeBlock(domain))}
        />
      </div>

      <section className={styles.section} aria-labelledby="presets-title">
        <div className={styles.sectionHead}>
          <h2 id="presets-title" className={styles.sectionTitle}>
            Fertige Listen
          </h2>
          <p className={styles.sectionLead}>
            Kuratierte Hagezi-Listen — dieselben, die für AdGuard Home und Pi-hole genutzt werden.
            Mit einem Klick aktiviert. Jede Liste nennt, was sie sperrt und wie hoch das Risiko für
            Fehlalarme ist.
          </p>
        </div>
        <div className={styles.cards}>
          {view.presets.map((preset) => (
            <PresetCard
              key={preset.key}
              preset={preset}
              sub={view.subscriptions.find((sub) => sub.preset === preset.key)}
              pending={pending.has(`preset:${preset.key}`)}
              onToggle={(on) =>
                run(`preset:${preset.key}`, () => api.setPreset(preset.key, on))
              }
            />
          ))}
        </div>
      </section>

      <section className={styles.section} aria-labelledby="feeds-title">
        <div className={styles.sectionHead}>
          <h2 id="feeds-title" className={styles.sectionTitle}>
            Eigene Listen per URL
          </h2>
          <p className={styles.sectionLead}>
            Abonniere jede Blockliste oder Freigabeliste im Hosts- oder AdGuard-Format über ihre
            URL. JustDNS lädt sie regelmäßig neu.
          </p>
        </div>

        <form className={styles.feedForm} onSubmit={addFeed}>
          <input
            className={styles.input}
            value={feedName}
            onChange={(event) => setFeedName(event.target.value)}
            placeholder="Name (optional)"
            aria-label="Name der Liste"
            autoComplete="off"
          />
          <input
            className={styles.input}
            value={feedUrl}
            onChange={(event) => setFeedUrl(event.target.value)}
            placeholder="https://…/liste.txt"
            aria-label="URL der Liste"
            autoComplete="off"
            spellCheck={false}
          />
          <select
            className={styles.select}
            value={feedKind}
            onChange={(event) => setFeedKind(event.target.value as ListKind)}
            aria-label="Art der Liste"
          >
            <option value="block">Sperrliste</option>
            <option value="allow">Freigabeliste</option>
          </select>
          <button type="submit" className={styles.addButton} disabled={pending.has("add-feed")}>
            Abonnieren
          </button>
        </form>

        {feeds.length === 0 ? (
          <p className={styles.empty}>Noch keine eigenen URL-Listen abonniert.</p>
        ) : (
          <ul className={styles.feeds}>
            {feeds.map((feed) => (
              <li key={feed.id} className={styles.feed} data-off={!feed.enabled || undefined}>
                <div className={styles.feedMain}>
                  <span className={styles.feedName}>{feed.name}</span>
                  <span className={styles.feedUrl}>{feed.url}</span>
                  <span className={styles.feedStatus}>
                    <span className={styles.kind} data-kind={feed.kind}>
                      {feed.kind === "allow" ? "Freigabe" : "Sperrliste"}
                    </span>
                    {feedStatusText(feed)}
                  </span>
                </div>
                <div className={styles.feedActions}>
                  <Toggle
                    label={`${feed.name} aktiv`}
                    checked={feed.enabled}
                    disabled={pending.has(`feed:${feed.id}`)}
                    onChange={(on) =>
                      run(`feed:${feed.id}`, () => api.setSubscriptionEnabled(feed.id, on))
                    }
                  />
                  <button
                    type="button"
                    className={styles.remove}
                    aria-label={`${feed.name} entfernen`}
                    title="Entfernen"
                    disabled={pending.has(`feed:${feed.id}`)}
                    onClick={() => run(`feed:${feed.id}`, () => api.removeSubscription(feed.id))}
                  >
                    ✕
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/** Human-readable status for a feed row from its last-fetch fields. */
function feedStatusText(feed: {
  lastError?: string;
  lastCount?: number;
  lastFetchedMs?: number;
}): ReactNode {
  if (feed.lastError) {
    return <span className={styles.statusError}>Aktualisierung fehlgeschlagen</span>;
  }
  if (feed.lastCount !== undefined && feed.lastFetchedMs !== undefined) {
    return (
      <span className={styles.statusOk}>
        {feed.lastCount.toLocaleString("de-DE")} Domains · {relativeTime(feed.lastFetchedMs)}
      </span>
    );
  }
  return <span className={styles.statusPending}>wird geladen …</span>;
}
