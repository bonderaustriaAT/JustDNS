import { type ReactNode, useEffect, useId, useRef } from "react";

import styles from "./ConfirmDialog.module.css";

interface ConfirmDialogProps {
  /** Short question, e.g. "Delete rewrite?". */
  title: string;
  /** One line of consequence, e.g. what is removed. */
  body: string;
  /** Optional richer detail (an identity box, a list of what will happen…),
   *  rendered under the body line. Plain-string dialogs leave it unset. */
  details?: ReactNode;
  /** Label on the destructive confirm button. */
  confirmLabel: string;
  /** Label on the dismiss button. */
  cancelLabel: string;
  /** While true, the buttons are disabled (the action is in flight). */
  busy?: boolean;
  /** When true, only the confirm button is disabled (e.g. the form in `details`
   *  is incomplete or invalid). Cancel stays usable. */
  confirmDisabled?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** A small modal that asks the user to confirm a destructive action. It traps
 *  nothing fancy: Escape and a backdrop click cancel, the cancel button takes
 *  focus on open (the safe default for a delete), and the confirm button is
 *  styled as destructive. Reusable for any "are you sure?" step. */
export function ConfirmDialog({
  title,
  body,
  details,
  confirmLabel,
  cancelLabel,
  busy = false,
  confirmDisabled = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const bodyId = useId();

  // Focus the dismiss button once, when the dialog opens. This runs on mount
  // only: a dependency on `busy`/`onCancel` (an inline arrow from the caller,
  // new on every render) would re-fire on each keystroke and yank focus out of
  // an input in `details` — the classic "type one letter and lose the field" bug.
  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  // Escape cancels. The listener is registered once; it reads the latest
  // `busy`/`onCancel` through refs so re-renders never re-run this effect.
  const onCancelRef = useRef(onCancel);
  onCancelRef.current = onCancel;
  const busyRef = useRef(busy);
  busyRef.current = busy;
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape" && !busyRef.current) onCancelRef.current();
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div
      className={styles.overlay}
      onMouseDown={(event) => {
        // Only a click on the backdrop itself (not a drag out of the dialog)
        // dismisses, and never while the action is running.
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
      >
        <h2 id={titleId} className={styles.title}>
          {title}
        </h2>
        <p id={bodyId} className={styles.body}>
          {body}
        </p>
        {details}
        <div className={styles.actions}>
          <button
            type="button"
            ref={cancelRef}
            className={styles.cancel}
            onClick={onCancel}
            disabled={busy}
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            className={styles.confirm}
            onClick={onConfirm}
            disabled={busy || confirmDisabled}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
