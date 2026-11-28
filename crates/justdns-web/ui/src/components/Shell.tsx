import { NavLink, useLocation } from "react-router-dom";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useAuth } from "../state/auth";
import { api } from "../lib/api";
import { ICONS } from "../lib/icons";
import { LANGS, useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { useSettings } from "../state/settings";
import { FeatureGuide } from "./FeatureGuide";
import { InstallBanner } from "./InstallBanner";
import { UpdateBanner } from "./UpdateBanner";
import styles from "./Shell.module.css";

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
  { to: "/listen/url", labelKey: "nav.listsUrl" as const, icon: "listUrl" },
  { to: "/listen/fertige", labelKey: "nav.listsPresets" as const, icon: "listPresets" },
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
              <span className={styles.navSubIcon}>{ICONS[item.icon]}</span>
              <span className={styles.navText}>{t(item.labelKey)}</span>
            </NavLink>
          ))}
        </div>
      )}
    </>
  );
}

/** A labelled sidebar section: a quiet group heading above its links, with every
 *  item visible. Grouping the whole list (not just folding the long tail away)
 *  is what keeps it scannable; the heading tells the user what each block is for. */
function NavSection({ label, children }: { label: string; children: ReactNode }) {
  // Tie the heading to its links so assistive tech announces the group, the way
  // the old collapsible button used to convey it.
  const headingId = `nav-section-${label.replace(/\s+/g, "-").toLowerCase()}`;
  return (
    <div className={styles.navSection} role="group" aria-labelledby={headingId}>
      <div className={styles.sectionLabel} id={headingId}>
        {label}
      </div>
      {children}
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

  // The searchable feature guide. Opened from the top bar (or the drawer on
  // mobile), and from Ctrl/Cmd+K anywhere for keyboard users.
  const [guideOpen, setGuideOpen] = useState(false);
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setGuideOpen((value) => !value);
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

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
      {(follower || readOnly) && (
        <div className={styles.globalBanner} role="status">
          <svg
            className={styles.globalBannerIcon}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <rect x="4" y="11" width="16" height="9" rx="2" />
            <path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
          <span>
            {follower ? t("common.followerReadOnly") : t("common.readOnly")}
            {follower && primaryUrl && (
              <>
                {" "}
                <a href={primaryUrl} className={styles.globalBannerLink}>
                  {primaryUrl}
                </a>
              </>
            )}
          </span>
        </div>
      )}

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
            {/* The whole list is grouped under quiet headings, every item visible.
                Grouping (not folding) is what keeps it scannable; each heading
                tells the user what the block below it is for. */}
            <NavSection label={t("nav.sectionOverview")}>
              <NavRow to="/" end icon="overview" label={t("nav.overview")} />
              <NavRow to="/protokoll" icon="log" label={t("nav.log")} />
            </NavSection>

            <NavSection label={t("nav.sectionProtection")}>
              <NavRow to="/privatsphaere" icon="privacy" label={t("nav.privacy")} />
              <NavRow to="/dnssec" icon="dnssec" label={t("nav.dnssec")} />
              <NavRow to="/phishing" icon="phishing" label={t("nav.phishing")} />
            </NavSection>

            <NavSection label={t("nav.sectionRules")}>
              <NavRow to="/geraete" icon="devices" label={t("nav.devices")} />
              <ListsGroup />
              {isAdmin && <NavRow to="/dienste" icon="services" label={t("nav.services")} />}
              <NavRow to="/zeitplaene" icon="schedules" label={t("nav.schedules")} />
              <NavRow to="/rewrites" icon="rewrites" label={t("nav.rewrites")} />
            </NavSection>

            <NavSection label={t("nav.sectionSystem")}>
              <button
                type="button"
                className={styles.navItem}
                onClick={() => {
                  setDrawerOpen(false);
                  setGuideOpen(true);
                }}
              >
                <span className={styles.navIcon}>{ICONS.guide}</span>
                <span className={styles.navText}>{t("guide.open")}</span>
              </button>
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
            </NavSection>
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
            <button
              type="button"
              className={styles.guideButton}
              aria-label={t("guide.open")}
              title={`${t("guide.open")} (Strg+K)`}
              onClick={() => setGuideOpen(true)}
            >
              {ICONS.guide}
            </button>
            <LangSwitch />
            <AccountMenu />
          </div>
        </header>

        <UpdateBanner />
        <InstallBanner />

        <FeatureGuide open={guideOpen} onClose={() => setGuideOpen(false)} />

        <main className={styles.main}>
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
