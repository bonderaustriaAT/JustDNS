import { useId } from "react";

import styles from "./InfoHint.module.css";

interface InfoHintProps {
  label: string;
  text: string;
}

// A small marker next to a setting name. Hovering or focusing it reveals a
// precise explanation of what the setting does.
export function InfoHint({ label, text }: InfoHintProps) {
  const id = useId();
  return (
    <span className={styles.wrap}>
      <button
        type="button"
        className={styles.trigger}
        aria-label={`Erklärung zu ${label}`}
        aria-describedby={id}
      >
        i
      </button>
      <span role="tooltip" id={id} className={styles.bubble}>
        {text}
      </span>
    </span>
  );
}
