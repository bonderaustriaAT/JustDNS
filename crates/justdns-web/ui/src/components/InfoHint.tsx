import { useId, useState } from "react";

import { useI18n } from "../lib/i18n";
import styles from "./InfoHint.module.css";

interface InfoHintProps {
  label: string;
  text: string;
}

// A small marker next to a setting name. Hovering or focusing it reveals a
// precise explanation of what the setting does. The bubble is only mounted while
// open, so a hidden 280px-wide tooltip never sticks out and widens the page on a
// phone.
export function InfoHint({ label, text }: InfoHintProps) {
  const { t } = useI18n();
  const id = useId();
  const [open, setOpen] = useState(false);

  return (
    <span
      className={styles.wrap}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <button
        type="button"
        className={styles.trigger}
        aria-label={t("infoHint.aria", { label })}
        aria-describedby={open ? id : undefined}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setOpen((value) => !value)}
      >
        i
      </button>
      {open ? (
        <span role="tooltip" id={id} className={styles.bubble}>
          {text}
        </span>
      ) : null}
    </span>
  );
}
