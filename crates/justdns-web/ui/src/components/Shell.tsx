import { NavLink, useLocation } from "react-router-dom";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useAuth } from "../state/auth";
import { LANGS, useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useSettings } from "../state/settings";
import { InstallBanner } from "./InstallBanner";
import styles from "./Shell.module.css";

/** Line icons for the mobile tab bar, drawn in the current text colour so the
 *  active tab tints with it. Kept inline to avoid an icon dependency. */
const ICONS: Record<string, ReactNode> = {
  overview: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <rect x="3" y="3" width="7" height="9" rx="1.5" />
      <rect x="14" y="3" width="7" height="5" rx="1.5" />
      <rect x="14" y="12" width="7" height="9" rx="1.5" />
      <rect x="3" y="16" width="7" height="5" rx="1.5" />
    </svg>
  ),
  log: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M8 6h12M8 12h12M8 18h12" />
      <circle cx="3.5" cy="6" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="3.5" cy="12" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="3.5" cy="18" r="1.2" fill="currentColor" stroke="none" />
    </svg>
  ),
  lists: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3l7 3v5c0 4.2-2.8 7.4-7 9-4.2-1.6-7-4.8-7-9V6l7-3z" />
      <path d="M9 12l2 2 4-4" strokeLinecap="round" />
    </svg>
  ),
  rewrites: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 7h11l-3-3M4 7l3 3" />
      <path d="M20 17H9l3 3M20 17l-3-3" />
    </svg>
  ),
  settings: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M6 5v14M12 5v14M18 5v14" />
      <circle cx="6" cy="9" r="2.3" fill="var(--bg)" />
      <circle cx="12" cy="15" r="2.3" fill="var(--bg)" />
      <circle cx="18" cy="8" r="2.3" fill="var(--bg)" />
    </svg>
  ),
};

/** The destinations shown in the mobile tab bar, mirroring the header nav. The
 *  Block/Allow tab lands on the first of its sub-pages; the rest map 1:1. */
const TABS: { to: string; icon: string; labelKey: MessageKey; end?: boolean }[] = [
  { to: "/", icon: "overview", labelKey: "nav.tab.overview", end: true },
  { to: "/protokoll", icon: "log", labelKey: "nav.tab.log" },
  { to: "/listen/url", icon: "lists", labelKey: "nav.tab.lists" },
  { to: "/rewrites", icon: "rewrites", labelKey: "nav.tab.rewrites" },
  { to: "/einstellungen", icon: "settings", labelKey: "nav.tab.settings" },
];

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
  const location = useLocation();

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
      <InstallBanner />
      <main className={styles.main}>{children}</main>
      <nav className={styles.tabbar} aria-label={t("nav.primary")}>
        {TABS.map((tab) => {
          // The Block/Allow tab covers both of its sub-pages, so it reads active
          // across the whole /listen area, not just its landing page.
          const forceActive = tab.to === "/listen/url" && location.pathname.startsWith("/listen");
          return (
            <NavLink
              key={tab.to}
              to={tab.to}
              end={tab.end}
              className={({ isActive }) =>
                isActive || forceActive ? styles.tabActive : styles.tab
              }
            >
              <span className={styles.tabIcon}>{ICONS[tab.icon]}</span>
              <span className={styles.tabLabel}>{t(tab.labelKey)}</span>
            </NavLink>
          );
        })}
      </nav>
    </div>
  );
}
