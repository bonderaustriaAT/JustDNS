import { useCallback, useEffect, useState } from "react";

import { api } from "./api";

/** How often the shared blocking-pause state is re-read, so a pause armed on
 *  another tab or node shows here too. */
const POLL_MS = 15_000;
/** How often "now" advances while a pause runs, so the state clears itself the
 *  moment the deadline passes without waiting for the next poll. */
const TICK_MS = 1_000;

/**
 * The shared troubleshooting blocking-pause state for the overview: the deadline
 * (Unix ms, or null when active), whether a pause is currently in force, and a
 * setter so the pause card can push an instant update after arming or clearing
 * it. One reader for the page, so the status line and the admin card never
 * disagree.
 */
export function useBlockingPause() {
  const [untilMs, setUntilMs] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const refresh = useCallback(
    (signal?: AbortSignal) =>
      api
        .blockingState(signal)
        .then((state) => setUntilMs(state.pausedUntilMs))
        .catch(() => {
          // A transient read failure leaves the last known state on screen.
        }),
    [],
  );

  useEffect(() => {
    const controller = new AbortController();
    refresh(controller.signal);
    const timer = setInterval(() => refresh(controller.signal), POLL_MS);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [refresh]);

  // Tick only while a pause is running, so the derived state flips back the
  // instant the deadline is reached.
  useEffect(() => {
    if (untilMs == null) return;
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, [untilMs]);

  const isPaused = untilMs != null && untilMs > now;
  return { untilMs, isPaused, setUntilMs };
}
