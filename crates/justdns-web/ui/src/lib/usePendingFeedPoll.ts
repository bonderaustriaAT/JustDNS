// SPDX-FileCopyrightText: 2026 JustDNS contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef } from "react";

import { api, type ListsView, type Subscription } from "./api";

/** How often the pending feeds are re-checked. */
const POLL_MS = 3_000;
/** Hard cap on consecutive polls, so a source the server never resolves (a hung
 *  download, a lost background task after a restart) does not keep the page
 *  polling forever. Sixty ticks ≈ three minutes; the manual refresh remains. */
const MAX_POLLS = 60;

/** Whether a feed is still resolving its first fetch: switched on, but the
 *  server has neither a rule count nor an error for it yet. Those are the rows
 *  that would otherwise read "loading…" forever, because the download runs as a
 *  background task the page never hears from again. */
function isPending(sub: Subscription): boolean {
  return sub.enabled && sub.lastFetchedMs === undefined && !sub.lastError;
}

/** While any enabled feed has never been fetched, re-pull the lists view every
 *  few seconds and hand it to `onUpdate` — so a freshly activated preset turns
 *  green (with its rule count) or red (with its error) on its own, instead of
 *  reading "loading…" until someone reloads the page. Stops as soon as nothing
 *  is pending, after the cap, and on unmount; a failed poll retries quietly on
 *  the next tick, like the phishing page's hit preview.
 *
 * `subs` is the page's current view of the subscriptions (both list pages hold
 * it in state), `onUpdate` receives the whole fresh `ListsView` so each page
 * can update exactly what it renders from it. */
export function usePendingFeedPoll(
  subs: Subscription[] | null,
  onUpdate: (lists: ListsView) => void,
) {
  const updateRef = useRef(onUpdate);
  updateRef.current = onUpdate;
  // Survives effect re-runs (every poll updates the view, which re-runs the
  // effect and restarts the interval); only a resolved view resets it.
  const pollsRef = useRef(0);

  useEffect(() => {
    const pending = (subs ?? []).some(isPending);
    if (!pending) {
      pollsRef.current = 0;
      return;
    }
    const timer = window.setInterval(async () => {
      if (++pollsRef.current > MAX_POLLS) {
        window.clearInterval(timer);
        return;
      }
      try {
        const lists = await api.lists();
        updateRef.current(lists);
      } catch {
        /* Transient — the next tick retries. */
      }
    }, POLL_MS);
    return () => window.clearInterval(timer);
    // The view object changes on every poll update, restarting the interval;
    // the cap lives in a ref above, so restarting does not reset it.
  }, [subs]);
}
