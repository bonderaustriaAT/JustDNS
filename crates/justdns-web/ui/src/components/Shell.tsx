import { NavLink } from "react-router-dom";
import type { ReactNode } from "react";

import styles from "./Shell.module.css";

export function Shell({ children }: { children: ReactNode }) {
  return (
    <div className={styles.shell}>
      <header className={styles.bar}>
        <div className={styles.brand}>
          <span className={styles.mark} aria-hidden="true" />
          <span className={styles.wordmark}>JustDNS</span>
        </div>
        <nav className={styles.nav}>
          <NavLink to="/" end className={({ isActive }) => (isActive ? styles.active : styles.link)}>
            Übersicht
          </NavLink>
          <NavLink
            to="/einstellungen"
            className={({ isActive }) => (isActive ? styles.active : styles.link)}
          >
            Einstellungen
          </NavLink>
        </nav>
      </header>
      <main className={styles.main}>{children}</main>
    </div>
  );
}
