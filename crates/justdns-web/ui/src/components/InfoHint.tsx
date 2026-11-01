import { useId } from "react";

import { useI18n } from "../lib/i18n";
import styles from "./InfoHint.module.css";

interface InfoHintProps {
  label: string;
  text: string;
}

// A small marker next to a setting name. Hovering or focusing it reveals a
// precise explanation of what the setting does.
export function InfoHint({ label, text }: InfoHintProps) {
  const { t } = useI18n();
  const id = useId();
  return (
    <span className={styles.wrap}>
      <button
        type="button"
        className={styles.trigger}
        aria-label={t("infoHint.aria", { label })}
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
