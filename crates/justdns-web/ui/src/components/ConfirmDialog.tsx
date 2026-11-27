import { type ReactNode, useEffect, useId, useRef } from "react";

import { trapTab } from "../lib/focus";
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
 *  focus like the Modal (Tab wraps inside, Escape and a backdrop click cancel,
 *  the cancel button takes focus on open — the safe default for a delete — and
 *  focus returns to the trigger on close), and the confirm button is styled as
 *  destructive. Reusable for any "are you sure?" step. */
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
  const dialogRef = useRef<HTMLDivElement>(null);
  const restoreTo = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const bodyId = useId();

  // Set the initial focus once, when the dialog opens. When `details` carries a
  // form (e.g. a password field), focus that field so the dialog is usable from
  // the keyboard at once; otherwise focus the dismiss button — the safe default
  // for a destructive "are you sure?". This runs on mount only: a dependency on
  // `busy`/`onCancel` (an inline arrow from the caller, new on every render)
  // would re-fire on each keystroke and yank focus out of the field — the classic
  // "type one letter and lose the field" bug. Remember the trigger so closing
  // returns the keyboard to where the user came from, like the Modal does.
  useEffect(() => {
    restoreTo.current = document.activeElement as HTMLElement | null;
    const field = dialogRef.current?.querySelector<HTMLElement>(
      "input, textarea, select",
    );
    if (field) field.focus();
    else cancelRef.current?.focus();
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = "";
      restoreTo.current?.focus();
    };
  }, []);

  // Escape cancels and Tab stays inside the dialog, the same trap the Modal
  // uses. The listener is registered once; it reads the latest `busy`/`onCancel`
  // through refs so re-renders never re-run this effect.
  const onCancelRef = useRef(onCancel);
  onCancelRef.current = onCancel;
  const busyRef = useRef(busy);
  busyRef.current = busy;
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const node = dialogRef.current;
      if (event.key === "Escape" && !busyRef.current) {
        onCancelRef.current();
        return;
      }
      if (node && trapTab(node, event)) event.preventDefault();
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
        ref={dialogRef}
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
