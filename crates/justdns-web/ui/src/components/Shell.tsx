import { NavLink, useLocation } from "react-router-dom";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useAuth } from "../state/auth";
import { LANGS, useI18n } from "../lib/i18n";
import { useSettings } from "../state/settings";
import styles from "./Shell.module.css";

/** The sub-pages the "Block/Allow" area is split across, shown as a dropdown in
 *  the header so no single page runs long. The user's own entries (manual
 *  allow/block plus URL subscriptions) live together on the first page. */
const LIST_ITEMS = [
  { to: "/listen/url", labelKey: "nav.listsUrl" as const },
  { to: "/listen/fertige", labelKey: "nav.listsPresets" as const },
];

/** The "Block/Allow" header entry: a button that opens a menu of its sub-pages.
 *  Closes on outside click, Escape, or navigating — the usual menu behaviour. */
function ListsMenu() {
  const { t } = useI18n();
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
        {t("nav.blockAllow")}
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
              {t(item.labelKey)}
            </NavLink>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** A compact segmented control to switch the interface language, in the header
 *  so it is reachable from every page. The current language reads as pressed. */
function LangSwitch() {
  const { lang, setLang, t } = useI18n();

  return (
    <div className={styles.langSwitch} role="group" aria-label={t("lang.label")}>
      {LANGS.map((code) => (
        <button
          key={code}
          type="button"
          className={styles.langButton}
          data-active={code === lang || undefined}
          aria-pressed={code === lang}
          onClick={() => setLang(code)}
        >
          {t(`lang.${code}`)}
        </button>
      ))}
    </div>
  );
}

export function Shell({ children }: { children: ReactNode }) {
  const { username, logout } = useAuth();
  const { t } = useI18n();
  const { settings } = useSettings();

  // Show the instance name next to the wordmark (and in the tab title) when the
  // user has set one that is not the default.
  const instance = settings.instanceName.trim();
  const showInstance = instance !== "" && instance !== "JustDNS";
  useEffect(() => {
    document.title = showInstance ? `${instance} · JustDNS` : "JustDNS";
  }, [instance, showInstance]);

  return (
    <div className={styles.shell}>
      <header className={styles.bar}>
        <div className={styles.inner}>
          <div className={styles.brand}>
            <span className={styles.mark} aria-hidden="true" />
            <span className={styles.wordmark}>JustDNS</span>
            {showInstance && <span className={styles.instance}>{instance}</span>}
          </div>
          <nav className={styles.nav}>
            <NavLink
              to="/"
              end
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              {t("nav.overview")}
            </NavLink>
            <NavLink
              to="/protokoll"
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              {t("nav.log")}
            </NavLink>
            <ListsMenu />
            <NavLink
              to="/rewrites"
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              {t("nav.rewrites")}
            </NavLink>
            <NavLink
              to="/einstellungen"
              className={({ isActive }) => (isActive ? styles.active : styles.link)}
            >
              {t("nav.settings")}
            </NavLink>
          </nav>
          <div className={styles.account}>
            <LangSwitch />
            {username && <span className={styles.user}>{username}</span>}
            <button type="button" className={styles.logout} onClick={() => logout()}>
              {t("account.logout")}
            </button>
          </div>
        </div>
      </header>
      <main className={styles.main}>{children}</main>
    </div>
  );
}
