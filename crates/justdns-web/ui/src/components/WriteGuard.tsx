import type { ReactNode } from "react";
import { useAuth } from "../state/auth";
import styles from "./WriteGuard.module.css";

/**
 * Wraps a page (or a region) whose controls must be read-only unless the current
 * session may write. On a read-only follower — and for a viewer who lacks edit
 * rights — `canEdit` is false, so the nested `<fieldset>` disables every form
 * control it contains. This is defence in depth over the backend's follower gate
 * (which already answers 403): the page simply stops offering edits instead of
 * letting the user try and fail. Read-only controls that are plain links still work.
 */
export function WriteGuard({ children }: { children: ReactNode }) {
  const { canEdit } = useAuth();
  return (
    <fieldset className={styles.guard} disabled={!canEdit}>
      {children}
    </fieldset>
  );
}
