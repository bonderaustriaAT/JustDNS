import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { NavLink } from "react-router-dom";

import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import {
  ApiError,
  api,
  type ListEntry,
  type ListKind,
  type ListsView,
  type Preset,
  type Subscription,
} from "../lib/api";
import { useI18n, type Lang } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import styles from "./Lists.module.css";

type T = ReturnType<typeof useI18n>["t"];

/** Pick the message key for a mutation's failure, shown in the banner. */
function errorKey(error: unknown): MessageKey {
  if (error instanceof ApiError) {
    if (error.status === 400) return "lists.badDomain";
    if (error.status === 404) return "common.notFound";
  }
  return "common.saveFailed";
}

/** Relative time for a feed's last update, e.g. "vor 2 Std.". */
function relativeTime(ms: number, t: T): string {
  const diff = Date.now() - ms;
  if (diff < 60_000) return t("time.justUpdated");
  const minutes = Math.round(diff / 60_000);
  if (minutes < 60) return t("time.minutesAgo", { n: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t("time.hoursAgo", { n: hours });
  const days = Math.round(hours / 24);
  return days === 1 ? t("time.oneDayAgo") : t("time.daysAgo", { n: days });
}

/** The catalogue categories in the order the filter shows them. */
const CATEGORY_ORDER: Preset["category"][] = ["ads", "security", "privacy", "bypass", "allow"];

/** A five-pip meter for a list's false-positive risk: how much legitimate
 *  traffic it might catch by mistake. The subject here is risk, so it reads as
 *  a scale, not a badge. */
function RiskMeter({ level }: { level: number }) {
  const { t } = useI18n();
  const clamped = Math.max(1, Math.min(5, level));
  return (
    <span
      className={styles.meter}
      data-level={clamped}
      role="img"
      aria-label={t("risk.aria", { level: t(`risk.${clamped}` as MessageKey) })}
    >
      {[1, 2, 3, 4, 5].map((pip) => (
        <span key={pip} className={styles.pip} data-on={pip <= clamped || undefined} />
      ))}
    </span>
  );
}

/** Longest note we keep, matching the server's cap. */
const NOTE_MAX = 200;

/** The inline note form a row turns into when its note is edited, so editing
 *  happens in place rather than in a separate panel. Seeded from the current
 *  note; an empty value clears it. */
function NoteEditor({
  entry,
  onSave,
  onCancel,
  busy,
}: {
  entry: ListEntry;
  onSave: (note: string) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  const { t } = useI18n();
  const [note, setNote] = useState(entry.note ?? "");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  function submit(event: FormEvent) {
    event.preventDefault();
    onSave(note.trim());
  }

  return (
    <form className={styles.noteEdit} onSubmit={submit}>
      <span className={styles.entryName}>{entry.domain}</span>
      <input
        ref={inputRef}
        className={styles.noteInput}
        value={note}
        onChange={(event) => setNote(event.target.value)}
        onKeyDown={(event) => event.key === "Escape" && onCancel()}
        placeholder={t("lists.notePlaceholder")}
        aria-label={t("lists.editNoteAria", { name: entry.domain })}
        maxLength={NOTE_MAX}
        autoComplete="off"
      />
      <button type="submit" className={styles.noteSave} disabled={busy}>
        {t("lists.noteSave")}
      </button>
      <button type="button" className={styles.noteCancel} onClick={onCancel} disabled={busy}>
        {t("lists.noteCancel")}
      </button>
    </form>
  );
}

/** An editor for one manual list: an add field over a list of removable rows.
 *  Deliberately not a free-text box — each entry is its own row with its own
 *  remove control, so the list stays legible and a single typo can't wipe it.
 *  Each entry can carry an optional note, shown under the name and edited in
 *  place; the add field has its own optional note input beside the domain. */
function EntryEditor({
  title,
  hint,
  accent,
  placeholder,
  entries,
  count,
  busy,
  onAdd,
  onRemove,
  onSetNote,
}: {
  title: string;
  hint: string;
  accent: "allow" | "block";
  placeholder: string;
  entries: ListEntry[];
  count: number;
  busy: (domain: string) => boolean;
  onAdd: (domain: string, note: string | undefined) => void;
  onRemove: (domain: string) => void;
  onSetNote: (domain: string, note: string) => void;
}) {
  const { t, locale } = useI18n();
  const [draft, setDraft] = useState("");
  const [noteDraft, setNoteDraft] = useState("");
  const [editing, setEditing] = useState<string | null>(null);

  function submit(event: FormEvent) {
    event.preventDefault();
    const domain = draft.trim();
    if (!domain) return;
    onAdd(domain, noteDraft.trim() || undefined);
    setDraft("");
    setNoteDraft("");
  }

  return (
    <section className={styles.editor} data-accent={accent} aria-labelledby={`${accent}-title`}>
      <div className={styles.editorHead}>
        <h3 id={`${accent}-title`} className={styles.editorTitle}>
          {title}
          <InfoHint label={title} text={hint} />
        </h3>
        <span className={styles.editorCount}>
          {t("lists.countActive", {
            count: count.toLocaleString(locale),
            noun: t("lists.countNoun"),
          })}
        </span>
      </div>

      <form className={styles.addRow} onSubmit={submit}>
        <input
          className={styles.input}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={placeholder}
          aria-label={t("lists.addDomainAria", { title })}
          autoComplete="off"
          spellCheck={false}
        />
        <input
          className={styles.noteField}
          value={noteDraft}
          onChange={(event) => setNoteDraft(event.target.value)}
          placeholder={t("lists.notePlaceholder")}
          aria-label={t("lists.noteFieldAria", { title })}
          maxLength={NOTE_MAX}
          autoComplete="off"
        />
        <button type="submit" className={styles.addButton}>
          {t("common.add")}
        </button>
      </form>

      {entries.length === 0 ? (
        <p className={styles.empty}>{t("lists.noOwnEntries")}</p>
      ) : (
        <ul className={styles.entries}>
          {entries.map((entry) =>
            editing === entry.domain ? (
              <li key={entry.domain} className={styles.entry}>
                <NoteEditor
                  entry={entry}
                  busy={busy(entry.domain)}
                  onCancel={() => setEditing(null)}
                  onSave={(note) => {
                    onSetNote(entry.domain, note);
                    setEditing(null);
                  }}
                />
              </li>
            ) : (
              <li key={entry.domain} className={styles.entry}>
                <div className={styles.entryMain}>
                  <span className={styles.entryName}>{entry.domain}</span>
                  {entry.note ? <span className={styles.entryNote}>{entry.note}</span> : null}
                </div>
                <div className={styles.entryActions}>
                  <button
                    type="button"
                    className={styles.noteButton}
                    aria-label={t(entry.note ? "lists.editNoteAria" : "lists.addNoteAria", {
                      name: entry.domain,
                    })}
                    onClick={() => setEditing(entry.domain)}
                  >
                    {t("lists.noteLabel")}
                  </button>
                  <button
                    type="button"
                    className={styles.remove}
                    aria-label={t("lists.removeAria", { name: entry.domain })}
                    title={t("common.remove")}
                    onClick={() => onRemove(entry.domain)}
                  >
                    ✕
                  </button>
                </div>
              </li>
            ),
          )}
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
  const { t, lang } = useI18n();
  return (
    <article className={styles.card} data-active={preset.active || undefined}>
      <header className={styles.cardHead}>
        <div className={styles.cardTitleWrap}>
          <h3 className={styles.cardTitle}>{preset.name}</h3>
          <div className={styles.cardTags}>
            <span className={styles.kind} data-kind={preset.kind}>
              {preset.kind === "allow" ? t("lists.kindAllow") : t("lists.kindBlock")}
            </span>
            <span className={styles.categoryTag}>
              {t(`category.${preset.category}` as MessageKey)}
            </span>
          </div>
        </div>
        <Toggle
          label={t("lists.activateAria", { name: preset.name })}
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

      {preset.active && sub ? (
        <p className={styles.status}>{feedStatusText(sub, t, lang)}</p>
      ) : null}
    </article>
  );
}

/** Which part of the Block/Allow area this page shows. The header nav splits it
 *  so no one page runs long: "mine" holds the user's own entries (manual
 *  allow/block plus URL subscriptions), "presets" the curated catalogue. */
export type ListsSection = "mine" | "presets";

export function Lists({ section }: { section: ListsSection }) {
  const { t, lang, locale } = useI18n();
  const [view, setView] = useState<ListsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [refreshing, setRefreshing] = useState(false);
  const [presetFilter, setPresetFilter] = useState<string>("all");

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
        setError(t("lists.loadFailed"));
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

  /** Run a mutation, replace the view with its result, surface any error. */
  async function run(key: string, action: () => Promise<ListsView>) {
    mark(key, true);
    setError(null);
    try {
      setView(await action());
    } catch (err) {
      setError(t(errorKey(err)));
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
        setError(t("lists.someRefreshFailed", { n: failed }));
      }
    } catch {
      setError(t("lists.refreshFailed"));
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
        <h1 className={styles.title}>{t("nav.blockAllow")}</h1>
        <p className={styles.lead}>{error ?? t("common.loading")}</p>
      </div>
    );
  }

  const feeds = view.subscriptions.filter((sub) => !sub.preset);

  // Build the filter chips from the categories actually present, each with its
  // count, so the bar mirrors the catalogue. "Alle" always leads.
  const categoryCounts = view.presets.reduce<Record<string, number>>((acc, preset) => {
    acc[preset.category] = (acc[preset.category] ?? 0) + 1;
    return acc;
  }, {});
  const filterChips = [
    { key: "all", label: t("lists.filterAll"), count: view.presets.length },
    ...CATEGORY_ORDER.filter((category) => categoryCounts[category]).map((category) => ({
      key: category,
      label: t(`category.${category}` as MessageKey),
      count: categoryCounts[category],
    })),
  ];
  const visiblePresets =
    presetFilter === "all"
      ? view.presets
      : view.presets.filter((preset) => preset.category === presetFilter);

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <div>
          <h1 className={styles.title}>{t("nav.blockAllow")}</h1>
          {section === "mine" ? <p className={styles.lead}>{t("lists.lead")}</p> : null}
        </div>
        <button
          type="button"
          className={styles.refresh}
          onClick={refreshAll}
          disabled={refreshing}
        >
          {refreshing ? t("lists.refreshing") : t("lists.refreshAll")}
        </button>
      </div>

      {/* On phones the header's Block/Allow menu is replaced by the bottom tab
          bar, so the two sub-pages get an in-page switch of their own here. */}
      <nav className={styles.subnav} aria-label={t("nav.blockAllow")}>
        <NavLink
          to="/listen/url"
          className={({ isActive }) => (isActive ? styles.subnavActive : styles.subnavLink)}
        >
          {t("nav.listsUrl")}
        </NavLink>
        <NavLink
          to="/listen/fertige"
          className={({ isActive }) => (isActive ? styles.subnavActive : styles.subnavLink)}
        >
          {t("nav.listsPresets")}
        </NavLink>
      </nav>

      {error ? (
        <p className={styles.banner} role="alert">
          {error}
        </p>
      ) : null}

      {section === "mine" ? (
      <div className={styles.editors}>
        <EntryEditor
          title={t("lists.allowTitle")}
          hint={t("lists.allowHint")}
          accent="allow"
          placeholder={t("lists.allowPlaceholder")}
          entries={view.allow}
          count={view.allowActive}
          busy={(domain) => pending.has(`allow:${domain}`)}
          onAdd={(domain, note) => run(`allow:${domain}`, () => api.addAllow(domain, note))}
          onRemove={(domain) => run(`allow:${domain}`, () => api.removeAllow(domain))}
          onSetNote={(domain, note) => run(`allow:${domain}`, () => api.setAllowNote(domain, note))}
        />
        <EntryEditor
          title={t("lists.blockTitle")}
          hint={t("lists.blockHint")}
          accent="block"
          placeholder={t("lists.blockPlaceholder")}
          entries={view.block}
          count={view.blockActive}
          busy={(domain) => pending.has(`block:${domain}`)}
          onAdd={(domain, note) => run(`block:${domain}`, () => api.addBlock(domain, note))}
          onRemove={(domain) => run(`block:${domain}`, () => api.removeBlock(domain))}
          onSetNote={(domain, note) => run(`block:${domain}`, () => api.setBlockNote(domain, note))}
        />
      </div>
      ) : null}

      {section === "presets" ? (
      <section className={styles.section} aria-labelledby="presets-title">
        <div className={styles.sectionHead}>
          <h2 id="presets-title" className={styles.sectionTitle}>
            {t("lists.presetsTitle")}
          </h2>
          <p className={styles.sectionLead}>{t("lists.presetsLead")}</p>
        </div>

        <div className={styles.filters} role="tablist" aria-label={t("lists.filterAria")}>
          {filterChips.map((chip) => (
            <button
              key={chip.key}
              type="button"
              role="tab"
              aria-selected={presetFilter === chip.key}
              className={styles.filter}
              data-active={presetFilter === chip.key || undefined}
              onClick={() => setPresetFilter(chip.key)}
            >
              {chip.label}
              <span className={styles.filterCount}>{chip.count}</span>
            </button>
          ))}
        </div>

        <div className={styles.cards}>
          {visiblePresets.map((preset) => (
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
      ) : null}

      {section === "mine" ? (
      <section className={styles.section} aria-labelledby="feeds-title">
        <div className={styles.sectionHead}>
          <h2 id="feeds-title" className={styles.sectionTitle}>
            {t("lists.feedsTitle")}
          </h2>
          <p className={styles.sectionLead}>{t("lists.feedsLead")}</p>
        </div>

        <form className={styles.feedForm} onSubmit={addFeed}>
          <input
            className={styles.input}
            value={feedName}
            onChange={(event) => setFeedName(event.target.value)}
            placeholder={t("lists.feedNamePlaceholder")}
            aria-label={t("lists.feedNameAria")}
            autoComplete="off"
          />
          <input
            className={styles.input}
            value={feedUrl}
            onChange={(event) => setFeedUrl(event.target.value)}
            placeholder={t("lists.feedUrlPlaceholder")}
            aria-label={t("lists.feedUrlAria")}
            autoComplete="off"
            spellCheck={false}
          />
          <select
            className={styles.select}
            value={feedKind}
            onChange={(event) => setFeedKind(event.target.value as ListKind)}
            aria-label={t("lists.feedKindAria")}
          >
            <option value="block">{t("lists.kindBlock")}</option>
            <option value="allow">{t("lists.kindAllow")}</option>
          </select>
          <button type="submit" className={styles.addButton} disabled={pending.has("add-feed")}>
            {t("lists.subscribe")}
          </button>
        </form>

        {feeds.length === 0 ? (
          <p className={styles.empty}>{t("lists.noFeeds")}</p>
        ) : (
          <ul className={styles.feeds}>
            {feeds.map((feed) => (
              <li key={feed.id} className={styles.feed} data-off={!feed.enabled || undefined}>
                <div className={styles.feedMain}>
                  <span className={styles.feedName}>{feed.name}</span>
                  <span className={styles.feedUrl}>{feed.url}</span>
                  <span className={styles.feedStatus}>
                    <span className={styles.kind} data-kind={feed.kind}>
                      {feed.kind === "allow" ? t("lists.kindAllow") : t("lists.kindBlock")}
                    </span>
                    {feedStatusText(feed, t, lang, locale)}
                  </span>
                </div>
                <div className={styles.feedActions}>
                  <Toggle
                    label={t("lists.feedActiveAria", { name: feed.name })}
                    checked={feed.enabled}
                    disabled={pending.has(`feed:${feed.id}`)}
                    onChange={(on) =>
                      run(`feed:${feed.id}`, () => api.setSubscriptionEnabled(feed.id, on))
                    }
                  />
                  <button
                    type="button"
                    className={styles.remove}
                    aria-label={t("lists.removeAria", { name: feed.name })}
                    title={t("common.remove")}
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
      ) : null}
    </div>
  );
}

/** Human-readable status for a feed row from its last-fetch fields. Number
 *  formatting follows the given locale; the relative time follows `lang`. */
function feedStatusText(
  feed: { lastError?: string; lastCount?: number; lastFetchedMs?: number },
  t: T,
  lang: Lang,
  locale: string = lang === "de" ? "de-DE" : "en-US",
): ReactNode {
  if (feed.lastError) {
    return <span className={styles.statusError}>{t("lists.feedFailed")}</span>;
  }
  if (feed.lastCount !== undefined && feed.lastFetchedMs !== undefined) {
    return (
      <span className={styles.statusOk}>
        {t("lists.feedCount", {
          count: feed.lastCount.toLocaleString(locale),
          time: relativeTime(feed.lastFetchedMs, t),
        })}
      </span>
    );
  }
  return <span className={styles.statusPending}>{t("lists.feedLoading")}</span>;
}
