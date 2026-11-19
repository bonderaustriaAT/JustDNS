import { useEffect, useRef, useState } from "react";

import { api, ApiError } from "../lib/api";
import { useI18n } from "../lib/i18n";
import styles from "./BlockingPause.module.css";

/** How often the paused view redraws its countdown. */
const TICK_MS = 1_000;
/** The preset durations, in seconds, plus a custom span given in hours. */
const MIN_1 = 60;
const MIN_5 = 5 * 60;
/** The server caps a pause at 24 hours; mirror that bound on the custom input. */
const MAX_HOURS = 24;

type Choice = "min1" | "min5" | "custom";

/** Format a remaining span as `m:ss`, or `h:mm:ss` once it reaches an hour. */
function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1_000));
  const hours = Math.floor(total / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  const seconds = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(seconds)}`
    : `${minutes}:${pad(seconds)}`;
}

interface BlockingPauseProps {
  /** The current pause deadline (Unix ms) or null when blocking is active. Owned
   *  by the overview so the status line and this card never disagree. */
  untilMs: number | null;
  /** Push a freshly learned deadline up to the owner (after arming, clearing or
   *  when the running pause reaches its deadline). */
  onChange: (untilMs: number | null) => void;
}

/**
 * The troubleshooting "pause all blocking" control: an inline card that turns off
 * lists, phishing/NRD and CNAME uncloaking for 1 minute, 5 minutes or a custom
 * number of hours, then shows a ring countdown with a resume action while a pause
 * is in force. Admin only; the server also gates it and expires the pause on its
 * own. The pause deadline is owned by the overview and passed in, so the page's
 * protection banner reflects the same state.
 */
export function BlockingPause({ untilMs, onChange }: BlockingPauseProps) {
  const { t } = useI18n();
  // The span the active pause started from, so the ring can show how much is
  // left. Seeded from the remaining time when a pause is already running on load.
  const [totalMs, setTotalMs] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [choice, setChoice] = useState<Choice>("min5");
  const [hours, setHours] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // True once the ring's reference span is set for the current pause, so a poll
  // that re-delivers the same deadline does not reset a shrinking ring.
  const seededTotal = useRef(false);

  // Keep the ring's reference span in step with the deadline the owner provides:
  // clear it when blocking resumes, and seed it from what is left for a pause we
  // did not start here (`pause` seeds it itself before the deadline arrives).
  useEffect(() => {
    if (untilMs == null) {
      setTotalMs(null);
      seededTotal.current = false;
    } else if (!seededTotal.current) {
      setTotalMs(Math.max(1_000, untilMs - Date.now()));
      seededTotal.current = true;
    }
  }, [untilMs]);

  // Tick the clock only while a pause is running, so the countdown advances and
  // clears itself the moment it reaches zero.
  useEffect(() => {
    if (untilMs == null) return;
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, [untilMs]);

  const remainingMs = untilMs == null ? 0 : untilMs - now;
  // The deadline has passed: tell the owner so the whole page falls back to the
  // active view without waiting for the next poll.
  useEffect(() => {
    if (untilMs != null && remainingMs <= 0) {
      onChange(null);
    }
  }, [untilMs, remainingMs, onChange]);

  async function pause() {
    const seconds =
      choice === "min1" ? MIN_1 : choice === "min5" ? MIN_5 : hours * 3_600;
    setBusy(true);
    setError(null);
    try {
      const state = await api.pauseBlocking(seconds);
      // Seed the ring from the requested span before the new deadline arrives,
      // so it starts full rather than from the round-trip remainder.
      setTotalMs(seconds * 1_000);
      seededTotal.current = true;
      setNow(Date.now());
      onChange(state.pausedUntilMs);
    } catch (err) {
      setError(
        t("blocking.error", {
          error: err instanceof ApiError ? err.message : String(err),
        }),
      );
    } finally {
      setBusy(false);
    }
  }

  async function resume() {
    setBusy(true);
    setError(null);
    try {
      const state = await api.resumeBlocking();
      onChange(state.pausedUntilMs);
    } catch (err) {
      setError(
        t("blocking.error", {
          error: err instanceof ApiError ? err.message : String(err),
        }),
      );
    } finally {
      setBusy(false);
    }
  }

  if (untilMs != null && remainingMs > 0) {
    // The ring depletes from full to empty across the pause; guard against a
    // seed that is smaller than the remaining time (clamped to [0, 1]).
    const fraction =
      totalMs && totalMs > 0 ? Math.min(1, Math.max(0, remainingMs / totalMs)) : 0;
    const radius = 34;
    const circumference = 2 * Math.PI * radius;
    const offset = circumference * (1 - fraction);
    return (
      <section className={`${styles.card} ${styles.paused}`} aria-live="polite">
        <div className={styles.ringRow}>
          <div className={styles.ring}>
            <svg viewBox="0 0 80 80" aria-hidden="true">
              <circle className={styles.ringTrack} cx="40" cy="40" r={radius} />
              <circle
                className={styles.ringFill}
                cx="40"
                cy="40"
                r={radius}
                strokeDasharray={circumference}
                strokeDashoffset={offset}
                transform="rotate(-90 40 40)"
              />
            </svg>
            <div className={styles.ringLabel}>
              <span className={styles.ringTime}>{formatRemaining(remainingMs)}</span>
              <span className={styles.ringUnit}>{t("blocking.remaining")}</span>
            </div>
          </div>
          <div className={styles.pausedBody}>
            <h2 className={styles.pausedTitle}>{t("blocking.activeTitle")}</h2>
            <p className={styles.pausedNote}>{t("blocking.activeNote")}</p>
            <button
              type="button"
              className={styles.resume}
              onClick={resume}
              disabled={busy}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true" className={styles.resumeIcon}>
                <path d="M8 5v14l11-7z" fill="currentColor" />
              </svg>
              {busy ? t("blocking.resuming") : t("blocking.resume")}
            </button>
            {error && <p className={styles.error}>{error}</p>}
          </div>
        </div>
      </section>
    );
  }

  return (
    <section className={styles.card}>
      <div className={styles.head}>
        <span className={styles.icon} aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 3l7 3v5c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z" />
            <path d="M5 4l14 14" />
          </svg>
        </span>
        <div>
          <h2 className={styles.title}>{t("blocking.title")}</h2>
          <p className={styles.subtitle}>{t("blocking.subtitle")}</p>
        </div>
      </div>
      <p className={styles.intro}>{t("blocking.intro")}</p>

      <div className={styles.controls}>
        <div className={styles.segmented} role="radiogroup" aria-label={t("blocking.title")}>
          <button
            type="button"
            role="radio"
            aria-checked={choice === "min1"}
            data-active={choice === "min1"}
            className={styles.segment}
            onClick={() => setChoice("min1")}
          >
            {t("blocking.min1")}
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={choice === "min5"}
            data-active={choice === "min5"}
            className={styles.segment}
            onClick={() => setChoice("min5")}
          >
            {t("blocking.min5")}
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={choice === "custom"}
            data-active={choice === "custom"}
            className={styles.segment}
            onClick={() => setChoice("custom")}
          >
            {t("blocking.customLabel")}
          </button>
        </div>

        {choice === "custom" && (
          <label className={styles.hours}>
            <input
              type="number"
              min={1}
              max={MAX_HOURS}
              step={1}
              value={hours}
              onChange={(event) => {
                const next = Math.round(Number(event.target.value));
                setHours(Number.isFinite(next) ? Math.min(MAX_HOURS, Math.max(1, next)) : 1);
              }}
              aria-label={t("blocking.customLabel")}
            />
            <span>{t("blocking.hoursUnit")}</span>
          </label>
        )}
      </div>

      <button type="button" className={styles.pause} onClick={pause} disabled={busy}>
        {busy ? t("blocking.pausing") : t("blocking.pauseAction")}
      </button>
      {error && <p className={styles.error}>{error}</p>}
    </section>
  );
}
