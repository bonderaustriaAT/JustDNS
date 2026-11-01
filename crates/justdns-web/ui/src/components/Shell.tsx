import { NavLink, useLocation } from "react-router-dom";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useAuth } from "../state/auth";
import styles from "./Shell.module.css";

/** The sub-pages the "Block/Allow" area is split across, shown as a dropdown in
 *  the header so no single page runs long. The user's own entries (manual
 *  allow/block plus URL subscriptions) live together on the first page. */
const LIST_ITEMS = [
  { to: "/listen/url", label: "Eigene Listen per URL" },
  { to: "/listen/fertige", label: "Fertige Listen" },
];

/** The "Block/Allow" header entry: a button that opens a menu of its sub-pages.
 *  Closes on outside click, Escape, or navigating — the usual menu behaviour. */
function ListsMenu() {
  const location = useLocation();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const onLists = location.pathname.startsWith("/listen");

  // Any route change closes the menu (e.g. after picking an item).
  useEffect(() => setOpen(false), [location.pathname]);

  // While open, a click outside or an Escape dismisses it.
  useEffect(() => {
    if (!open) return;
    function onPointer(event: PointerEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false);
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className={styles.menu} ref={ref}>
      <button
        type="button"
        className={onLists ? styles.active : styles.link}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        Block/Allow
        <span className={styles.caret} data-open={open || undefined} aria-hidden="true">
          ›
        </span>
      </button>
      {open ? (
        <div className={styles.menuList} role="menu">
          {LIST_ITEMS.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              role="menuitem"
              className={({ isActive }) => (isActive ? styles.menuItemActive : styles.menuItem)}
            >
              {item.label}
            </NavLink>
          ))}
        </div>
      ) : null}
    </div>
  );
}

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
            <ListsMenu />
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
