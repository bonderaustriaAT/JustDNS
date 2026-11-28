// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useI18n } from "../lib/i18n";
import { ICONS } from "../lib/icons";
import { FEATURES, type FeatureEntry } from "../lib/features";
import { useAuth } from "../state/auth";
import { Modal } from "./Modal";
import styles from "./FeatureGuide.module.css";

type T = ReturnType<typeof useI18n>["t"];

/** The release this build runs as — the same string the footer shows. Local
 *  builds say "dev"; only real release versions (X.Y.Z) count as "new", so the
 *  dot never shows in development. */
const APP_VERSION: string = __APP_VERSION__;
const RELEASE = /^v?\d+\.\d+\.\d+$/;

/** Whether `since` names the currently running release — the entry counts as
 *  "new in this version" and gets the dot. */
function isNewHere(entry: FeatureEntry): boolean {
  return (
    entry.since !== undefined &&
    RELEASE.test(APP_VERSION) &&
    APP_VERSION.replace(/^v/, "") === entry.since
  );
}

/** How well an entry matches `needle`: 3 a title hit, 2 a keyword hit,
 *  1 a description hit, 0 no match. Keeps the ranking local and transparent —
 *  substring search is entirely sufficient for a catalogue this size. */
function score(entry: FeatureEntry, needle: string, lang: "de" | "en", t: T): number {
  const title = t(entry.titleKey).toLowerCase();
  if (title.includes(needle)) return 3;
  const keywords = entry.keywords[lang];
  if (keywords.some((word) => word.toLowerCase().includes(needle))) return 2;
  const desc = t(entry.descKey).toLowerCase();
  if (desc.includes(needle)) return 1;
  return 0;
}

/** The searchable index of every feature on this box: type a name or a
 *  keyword, see what JustDNS can do, jump straight there — or follow the
 *  "learn more" link into the matching guide. Opened from the top bar, the
 *  drawer or Ctrl/Cmd+K, and filtered to the entries the signed-in role can
 *  actually act on. */
export function FeatureGuide({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t, lang } = useI18n();
  const { role } = useAuth();
  const navigate = useNavigate();
  const [needle, setNeedle] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Every change of the query resets the selection to the top hit.
  useEffect(() => setActive(0), [needle]);

  // The entries the signed-in role may use. Admins see everything, an editor
  // everything but admin-only areas, a viewer the read surfaces.
  const entries = useMemo(
    () => FEATURES.filter((entry) => role !== null && entry.roles.includes(role)),
    [role],
  );

  const hits = useMemo(() => {
    const trimmed = needle.trim().toLowerCase();
    if (!trimmed) return entries;
    return entries
      .map((entry) => ({ entry, rank: score(entry, trimmed, lang, t) }))
      .filter((row) => row.rank > 0)
      .sort((a, b) => b.rank - a.rank)
      .slice(0, 8)
      .map((row) => row.entry);
  }, [entries, needle, lang, t]);

  // Keep the active row in view when moving with the keyboard.
  useEffect(() => {
    const row = listRef.current?.children[active] as HTMLElement | undefined;
    row?.scrollIntoView({ block: "nearest" });
  }, [active]);

  function move(delta: number) {
    if (hits.length === 0) return;
    setActive((index) => (index + delta + hits.length) % hits.length);
  }

  function go(entry: FeatureEntry) {
    onClose();
    navigate(entry.route);
  }

  function onKeyDown(event: React.KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const entry = hits[active];
      if (entry) go(entry);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={t("guide.open")}>
      <div className={styles.body} onKeyDown={onKeyDown}>
        <input
          ref={inputRef}
          type="search"
          className={styles.search}
          placeholder={t("guide.searchPlaceholder")}
          value={needle}
          onChange={(event) => setNeedle(event.target.value)}
          autoFocus
        />
        {hits.length === 0 ? (
          <p className={styles.empty}>{t("guide.noResults")}</p>
        ) : (
          <ul className={styles.hits} ref={listRef} role="listbox">
            {hits.map((entry, index) => (
              <li key={entry.id} role="option" aria-selected={index === active}>
                <button
                  type="button"
                  className={index === active ? styles.hitActive : styles.hit}
                  onMouseEnter={() => setActive(index)}
                  onClick={() => go(entry)}
                >
                  <span className={styles.hitIcon}>{ICONS[entry.icon]}</span>
                  <span className={styles.hitText}>
                    <span className={styles.hitTitle}>
                      {t(entry.titleKey)}
                      {isNewHere(entry) && <span className={styles.newBadge}>{t("guide.newBadge")}</span>}
                    </span>
                    <span className={styles.hitDesc}>{t(entry.descKey)}</span>
                    <span className={styles.hitMeta}>
                      <span className={styles.category}>{t(entry.category)}</span>
                      {entry.guide && (
                        <a
                          href={`https://github.com/bonderaustriaAT/JustDNS/blob/main/docs/guides/${entry.guide}.${lang === "de" ? "de.md" : "md"}`}
                          target="_blank"
                          rel="noreferrer noopener"
                          className={styles.guideLink}
                          onClick={(event) => event.stopPropagation()}
                        >
                          {t("guide.moreInfo")}
                        </a>
                      )}
                    </span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className={styles.hint}>{t("guide.openHint")}</p>
      </div>
    </Modal>
  );
}
