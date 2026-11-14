import { NavLink, useLocation } from "react-router-dom";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useAuth } from "../state/auth";
import { api } from "../lib/api";
import { LANGS, useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useSettings } from "../state/settings";
import { InstallBanner } from "./InstallBanner";
import { UpdateBanner } from "./UpdateBanner";
import styles from "./Shell.module.css";

/** Line icons for the sidebar and the mobile tab bar, drawn in the current text
 *  colour so the active entry tints with it. Kept inline to avoid an icon
 *  dependency. */
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
  devices: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <rect x="2" y="4" width="20" height="12" rx="1.5" />
      <path d="M8 20h8M12 16v4" strokeLinecap="round" />
    </svg>
  ),
  dnssec: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3l7 3v5c0 4.2-2.8 7.4-7 9-4.2-1.6-7-4.8-7-9V6l7-3z" />
      <rect x="9" y="11" width="6" height="5" rx="1" />
      <path d="M10.3 11V9.6a1.7 1.7 0 0 1 3.4 0V11" strokeLinecap="round" />
    </svg>
  ),
  lists: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3l7 3v5c0 4.2-2.8 7.4-7 9-4.2-1.6-7-4.8-7-9V6l7-3z" />
      <path d="M9 12l2 2 4-4" strokeLinecap="round" />
    </svg>
  ),
  privacy: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3l7 3v5c0 4.2-2.8 7.4-7 9-4.2-1.6-7-4.8-7-9V6l7-3z" />
      <path d="M8.3 11.2c1-1.6 2.3-2.4 3.7-2.4s2.7.8 3.7 2.4c-1 1.6-2.3 2.4-3.7 2.4s-2.7-.8-3.7-2.4z" strokeLinecap="round" />
      <circle cx="12" cy="11.2" r="1" fill="currentColor" stroke="none" />
    </svg>
  ),
  rewrites: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 7h11l-3-3M4 7l3 3" />
      <path d="M20 17H9l3 3M20 17l-3-3" />
    </svg>
  ),
  services: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <circle cx="17.5" cy="17.5" r="3.7" />
      <path d="M14.9 20.1l5.2-5.2" strokeLinecap="round" />
    </svg>
  ),
  schedules: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="13" r="8" />
      <path d="M12 9v4l2.5 2.5" />
      <path d="M9 2h6" />
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
  users: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="9" cy="8" r="3.2" />
      <path d="M3.5 19a5.5 5.5 0 0 1 11 0" />
      <path d="M15.8 5.4a3 3 0 0 1 0 5.4" />
      <path d="M17.2 19a5.6 5.6 0 0 0-2.3-4.4" />
    </svg>
  ),
  audit: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="5" y="3" width="14" height="18" rx="2" />
      <path d="M9 8h6M9 12h6M9 16h4" />
    </svg>
  ),
  system: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M3 12h3l2.5 6 4-13 2.5 10 2-3h4" />
    </svg>
  ),
  ha: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="4" width="7" height="7" rx="1.5" />
      <rect x="14" y="13" width="7" height="7" rx="1.5" />
      <path d="M10 7h2.5a2 2 0 0 1 2 2v3M14 17h-2.5a2 2 0 0 1-2-2v-3" />
    </svg>
  ),
  reports: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M6 3h8l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
      <path d="M13 3v5h5" />
      <path d="M9 13v4M12.5 11v6M16 14v3" />
    </svg>
  ),
  certs: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="10" cy="9" r="6" />
      <path d="M10 6.5l1 2 2.2.2-1.7 1.5.5 2.1L10 12.2 8 13.4l.5-2.1L6.8 9.7 9 9.5z" fill="currentColor" stroke="none" />
      <path d="M7.4 14.2L6 21l4-2 4 2-1.4-6.8" />
    </svg>
  ),
  release: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M7 11V8a5 5 0 0 1 9.6-2" />
      <rect x="4.5" y="11" width="15" height="9" rx="2" />
      <path d="M12 15v2.5" />
    </svg>
  ),
  // Parent-group icons: horizontal sliders for "Rules", a cog for "System", kept
  // distinct from the vertical-slider Settings child so the two never clash.
  rules: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M4 7h16M4 12h16M4 17h16" />
      <circle cx="9" cy="7" r="2.3" fill="var(--bg)" />
      <circle cx="15" cy="12" r="2.3" fill="var(--bg)" />
      <circle cx="8" cy="17" r="2.3" fill="var(--bg)" />
    </svg>
  ),
  admin: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.2 5.2l2.1 2.1M16.7 16.7l2.1 2.1M18.8 5.2l-2.1 2.1M7.3 16.7l-2.1 2.1" />
    </svg>
  ),
};

/** The destinations shown in the mobile tab bar: the most-used pages, for thumb
 *  reach. The rest of the navigation (schedules, rewrites, settings and the admin
 *  system pages) lives in the slide-in drawer behind the hamburger. The
 *  Block/Allow tab lands on the first of its sub-pages. */
const TABS: { to: string; icon: string; labelKey: MessageKey; end?: boolean }[] = [
  { to: "/", icon: "overview", labelKey: "nav.tab.overview", end: true },
  { to: "/protokoll", icon: "log", labelKey: "nav.tab.log" },
  { to: "/geraete", icon: "devices", labelKey: "nav.tab.devices" },
  { to: "/listen/url", icon: "lists", labelKey: "nav.tab.lists" },
];

/** The sub-pages the "Block/Allow" area is split across. The user's own entries
 *  (manual allow/block plus URL subscriptions) live together on the first page. */
const LIST_ITEMS = [
  { to: "/listen/url", labelKey: "nav.listsUrl" as const },
  { to: "/listen/fertige", labelKey: "nav.listsPresets" as const },
];

/** A single sidebar link: icon plus label, active-aware. */
function NavRow({
  to,
  icon,
  label,
  end,
  badge,
}: {
  to: string;
  icon: string;
  label: string;
  end?: boolean;
  badge?: number;
}) {
  return (
    <NavLink
      to={to}
      end={end}
      className={({ isActive }) => (isActive ? styles.navItemActive : styles.navItem)}
    >
      <span className={styles.navIcon}>{ICONS[icon]}</span>
      <span className={styles.navText}>{label}</span>
      {badge !== undefined && badge > 0 && (
        <span className={styles.navBadge} aria-label={`${badge}`}>
          {badge > 99 ? "99+" : badge}
        </span>
      )}
    </NavLink>
  );
}

/** "Block/Allow" in the sidebar: an expandable row whose two sub-pages appear
 *  indented beneath it. Auto-expands while the user is anywhere under /listen. */
function ListsGroup() {
  const { t } = useI18n();
  const location = useLocation();
  const onLists = location.pathname.startsWith("/listen");
  const [open, setOpen] = useState(onLists);

  // Opening the area (via the tab bar or a deep link) unfolds the group so the
  // active sub-page is visible; the user can still collapse it by hand.
  useEffect(() => {
    if (onLists) setOpen(true);
  }, [onLists]);

  return (
    <>
      <button
        type="button"
        className={onLists ? styles.navItemActive : styles.navItem}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <span className={styles.navIcon}>{ICONS.lists}</span>
        <span className={styles.navText}>{t("nav.blockAllow")}</span>
        <span className={styles.caret} data-open={open || undefined} aria-hidden="true">
          ›
        </span>
      </button>
      {open && (
        <div className={styles.subNav}>
          {LIST_ITEMS.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) => (isActive ? styles.navSubActive : styles.navSub)}
            >
              {t(item.labelKey)}
            </NavLink>
          ))}
        </div>
      )}
    </>
  );
}

/** A collapsible sidebar section: an icon + label header that folds its child
 *  links in and out. It auto-opens while the active route is one of its children
 *  (so a deep link lands with the right section unfolded) and can still be folded
 *  by hand. Keeping the sections folded is what keeps the whole list short enough
 *  to fit without a scrollbar. */
function NavGroup({
  icon,
  label,
  active,
  children,
}: {
  icon: string;
  label: string;
  active: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(active);

  // Navigating into the section unfolds it; the user keeps manual control
  // otherwise.
  useEffect(() => {
    if (active) setOpen(true);
  }, [active]);

  return (
    <div className={styles.navGroup}>
      <button
        type="button"
        className={active ? styles.navItemActive : styles.navItem}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <span className={styles.navIcon}>{ICONS[icon]}</span>
        <span className={styles.navText}>{label}</span>
        <span className={styles.caret} data-open={open || undefined} aria-hidden="true">
          ›
        </span>
      </button>
      {open && <div className={styles.groupItems}>{children}</div>}
    </div>
  );
}

/** The account menu in the top bar: the signed-in user's name opening a short
 *  menu with their own account and sign out. User management and the audit trail
 *  now live in the sidebar's System group, so this stays small. */
function AccountMenu() {
  const { t } = useI18n();
  const { username, logout } = useAuth();
  const location = useLocation();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => setOpen(false), [location.pathname]);

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
        className={styles.accountButton}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t("nav.accountMenu")}
        onClick={() => setOpen((value) => !value)}
      >
        <span className={styles.user}>{username ?? t("nav.account")}</span>
        <span className={styles.caret} data-open={open || undefined} aria-hidden="true">
          ›
        </span>
      </button>
      {open ? (
        <div className={styles.menuList} role="menu">
          <NavLink
            to="/konto"
            role="menuitem"
            className={({ isActive }) => (isActive ? styles.menuItemActive : styles.menuItem)}
          >
            {t("nav.account")}
          </NavLink>
          <button type="button" role="menuitem" className={styles.menuItem} onClick={() => logout()}>
            {t("account.logout")}
          </button>
        </div>
      ) : null}
    </div>
  );
}

/** A compact segmented control to switch the interface language. */
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
  const { t } = useI18n();
  const { canEdit, isAdmin, isPrimary, primaryUrl } = useAuth();
  const { settings } = useSettings();
  const location = useLocation();

  // The slide-in navigation drawer (mobile only).
  const [drawerOpen, setDrawerOpen] = useState(false);

  // The pending access-request count, shown as a badge on the Freigaben entry so
  // a new "unblock this" ask is visible without opening the page. Polled while an
  // admin is signed in, and re-read on navigation so acting on the page clears it.
  const [accessPending, setAccessPending] = useState(0);
  useEffect(() => {
    if (!isAdmin) {
      setAccessPending(0);
      return;
    }
    const controller = new AbortController();
    const refresh = () => {
      api
        .accessRequests(controller.signal)
        .then((requests) => setAccessPending(requests.length))
        .catch(() => {
          /* a transient failure just leaves the last count in place */
        });
    };
    refresh();
    const timer = window.setInterval(refresh, 20000);
    return () => {
      controller.abort();
      window.clearInterval(timer);
    };
  }, [isAdmin, location.pathname]);

  // Navigating closes the drawer, so picking an entry takes you there without a
  // second tap to dismiss.
  useEffect(() => setDrawerOpen(false), [location.pathname]);

  // While the drawer is open, Escape closes it and the page behind it does not
  // scroll.
  useEffect(() => {
    if (!drawerOpen) return;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setDrawerOpen(false);
    }
    document.addEventListener("keydown", onKey);
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = "";
    };
  }, [drawerOpen]);

  // Editor pages the signed-in role may only read get a one-line notice, so the
  // reason a save would be refused is clear before they try. Content pages need
  // edit rights; settings needs admin.
  const path = location.pathname;
  const needsEdit =
    path.startsWith("/geraete") ||
    path.startsWith("/listen") ||
    path.startsWith("/zeitplaene") ||
    path.startsWith("/rewrites");
  const needsAdmin = path.startsWith("/einstellungen") || path.startsWith("/zertifikate");
  // On a read-only follower every page is read-only — except the HA page itself,
  // which stays usable so the follower can be repaired or take over a dead primary.
  const follower = !isPrimary && !path.startsWith("/ha");
  const readOnly = follower || (needsEdit && !canEdit) || (needsAdmin && !isAdmin);

  // Which collapsible section the current route belongs to, so it unfolds (and
  // its header reads active) on a deep link or a reload.
  const onRules = ["/geraete", "/listen", "/dienste", "/zeitplaene", "/rewrites"].some((p) =>
    path.startsWith(p),
  );
  const onSystem = [
    "/einstellungen",
    "/zertifikate",
    "/freigaben",
    "/benutzer",
    "/audit",
    "/system",
    "/berichte",
    "/ha",
  ].some((p) => path.startsWith(p));

  // Show the instance name next to the wordmark (and in the tab title) when the
  // user has set one that is not the default.
  const instance = settings.instanceName.trim();
  const showInstance = instance !== "" && instance !== "JustDNS";
  useEffect(() => {
    document.title = showInstance ? `${instance} · JustDNS` : "JustDNS";
  }, [instance, showInstance]);

  const brand = (
    <div className={styles.brand}>
      <span className={styles.mark} aria-hidden="true" />
      <span className={styles.wordmark}>JustDNS</span>
      {showInstance && <span className={styles.instance}>{instance}</span>}
    </div>
  );

  return (
    <div className={styles.shell}>
      {drawerOpen && (
        <div className={styles.scrim} onClick={() => setDrawerOpen(false)} aria-hidden="true" />
      )}

      <aside className={styles.sidebar} data-open={drawerOpen || undefined}>
        <div className={styles.sidebarInner}>
          <div className={styles.sidebarHead}>
            {brand}
            <button
              type="button"
              className={styles.closeDrawer}
              aria-label={t("nav.close")}
              onClick={() => setDrawerOpen(false)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                <path d="M6 6l12 12M18 6L6 18" />
              </svg>
            </button>
          </div>

          <nav className={styles.groups} aria-label={t("nav.primary")}>
            {/* The most-used pages sit at the top level; the rest fold into two
                collapsible sections so the list stays short. */}
            <div className={styles.navTop}>
              <NavRow to="/" end icon="overview" label={t("nav.overview")} />
              <NavRow to="/protokoll" icon="log" label={t("nav.log")} />
              <NavRow to="/privatsphaere" icon="privacy" label={t("nav.privacy")} />
              <NavRow to="/dnssec" icon="dnssec" label={t("nav.dnssec")} />
            </div>

            <NavGroup icon="rules" label={t("nav.sectionRules")} active={onRules}>
              <NavRow to="/geraete" icon="devices" label={t("nav.devices")} />
              <ListsGroup />
              {isAdmin && <NavRow to="/dienste" icon="services" label={t("nav.services")} />}
              <NavRow to="/zeitplaene" icon="schedules" label={t("nav.schedules")} />
              <NavRow to="/rewrites" icon="rewrites" label={t("nav.rewrites")} />
            </NavGroup>

            <NavGroup icon="admin" label={t("nav.sectionSystem")} active={onSystem}>
              <NavRow to="/einstellungen" icon="settings" label={t("nav.settings")} />
              {isAdmin && <NavRow to="/zertifikate" icon="certs" label={t("nav.certs")} />}
              {isAdmin && (
                <NavRow
                  to="/freigaben"
                  icon="release"
                  label={t("nav.access")}
                  badge={accessPending}
                />
              )}
              {isAdmin && <NavRow to="/benutzer" icon="users" label={t("nav.users")} />}
              {isAdmin && <NavRow to="/audit" icon="audit" label={t("nav.audit")} />}
              {isAdmin && <NavRow to="/system" icon="system" label={t("nav.systemLogs")} />}
              {isAdmin && <NavRow to="/berichte" icon="reports" label={t("nav.reports")} />}
              {isAdmin && <NavRow to="/ha" icon="ha" label={t("nav.ha")} />}
            </NavGroup>
          </nav>
        </div>
      </aside>

      <div className={styles.content}>
        <header className={styles.topbar}>
          <button
            type="button"
            className={styles.hamburger}
            aria-label={t("nav.menu")}
            aria-expanded={drawerOpen}
            onClick={() => setDrawerOpen(true)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <path d="M4 7h16M4 12h16M4 17h16" />
            </svg>
          </button>
          <div className={styles.topBrand}>{brand}</div>
          <div className={styles.topbarActions}>
            <LangSwitch />
            <AccountMenu />
          </div>
        </header>

        <UpdateBanner />
        <InstallBanner />

        <main className={styles.main}>
          {follower ? (
            <p className={styles.readOnlyBanner} role="status">
              {t("common.followerReadOnly")}
              {primaryUrl && (
                <>
                  {" "}
                  <a href={primaryUrl} className={styles.readOnlyLink}>
                    {primaryUrl}
                  </a>
                </>
              )}
            </p>
          ) : (
            readOnly && (
              <p className={styles.readOnlyBanner} role="status">
                {t("common.readOnly")}
              </p>
            )
          )}
          {children}
          <footer className={styles.footer}>
            JustDNS <span className={styles.version}>{__APP_VERSION__}</span>
          </footer>
        </main>

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
                className={({ isActive }) => (isActive || forceActive ? styles.tabActive : styles.tab)}
              >
                <span className={styles.tabIcon}>{ICONS[tab.icon]}</span>
                <span className={styles.tabLabel}>{t(tab.labelKey)}</span>
              </NavLink>
            );
          })}
        </nav>
      </div>
    </div>
  );
}
