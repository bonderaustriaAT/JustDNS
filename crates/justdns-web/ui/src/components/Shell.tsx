import { NavLink } from "react-router-dom";
import type { ReactNode } from "react";

import { useAuth } from "../state/auth";
import styles from "./Shell.module.css";

export function Shell({ children }: { children: ReactNode }) {
  const { username, logout } = useAuth();

  return (
    <div className={styles.shell}>
      <header className={styles.bar}>
        <div className={styles.inner}>
          <div className={styles.brand}>
            <span className={styles.mark} aria-hidden="true" />
            <span className={styles.wordmark}>JustDNS</span>
          </div>
          <nav className={styles.nav}>
            <NavLink
              to="/"
              end
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              Übersicht
            </NavLink>
            <NavLink
              to="/protokoll"
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              Protokoll
            </NavLink>
            <NavLink
              to="/listen"
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              Listen
            </NavLink>
            <NavLink
              to="/einstellungen"
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              Einstellungen
            </NavLink>
          </nav>
          <div className={styles.account}>
            {username && <span className={styles.user}>{username}</span>}
            <button type="button" className={styles.logout} onClick={() => logout()}>
              Abmelden
            </button>
          </div>
        </div>
      </header>
      <main className={styles.main}>{children}</main>
    </div>
  );
}
