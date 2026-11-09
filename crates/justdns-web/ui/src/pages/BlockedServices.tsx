import { useEffect, useMemo, useState } from "react";

import { Toggle } from "../components/Toggle";
import { ApiError, api, type Service } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { MessageKey } from "../lib/messages";
import { SERVICE_LOGOS } from "../lib/serviceLogos";
import { useAuth } from "../state/auth";
import styles from "./BlockedServices.module.css";

/** The catalogue categories in the order the filter shows them. */
const CATEGORY_ORDER = ["social", "video", "messaging", "gaming", "music"] as const;

/** Each service's signature colour, so its tile reads as a logo at a glance
 *  without pulling in a brand-icon dependency or trademarked artwork. `fg` is set
 *  only where a light tile needs dark lettering. */
const BRAND: Record<string, { bg: string; fg?: string }> = {
  facebook: { bg: "#1877F2" },
  instagram: { bg: "#E4405F" },
  tiktok: { bg: "#FE2C55" },
  x: { bg: "#ffffff", fg: "#0f1419" },
  snapchat: { bg: "#FFFC00", fg: "#0b0b0b" },
  reddit: { bg: "#FF4500" },
  pinterest: { bg: "#E60023" },
  youtube: { bg: "#FF0000" },
  netflix: { bg: "#E50914" },
  twitch: { bg: "#9146FF" },
  disneyplus: { bg: "#113CCF" },
  whatsapp: { bg: "#25D366" },
  telegram: { bg: "#26A5E4" },
  discord: { bg: "#5865F2" },
  steam: { bg: "#1B6AA5" },
  epicgames: { bg: "#2a2a2a", fg: "#ffffff" },
  spotify: { bg: "#1DB954" },
};

/** The glyph for a service's tile: its first letter. Paired with the brand
 *  colour it reads as a logo without an icon dependency. */
function initial(name: string): string {
  for (const ch of name) {
    if (/\p{L}|\p{N}/u.test(ch)) return ch.toUpperCase();
  }
  return "·";
}

/** One service card: a monochrome initial tile, the name, its category and the
 *  global on/off switch. The active card carries the same green accent spine as
 *  an active block-list card, so "on" reads at a glance across the grid. */
function ServiceCard({
  service,
  pending,
  canEdit,
  onToggle,
}: {
  service: Service;
  pending: boolean;
  canEdit: boolean;
  onToggle: (on: boolean) => void;
}) {
  const { t } = useI18n();
  const logo = SERVICE_LOGOS[service.id];
  const brand = BRAND[service.id] ?? { bg: "var(--signal)" };
  return (
    <article className={styles.card} data-active={service.globalOn || undefined}>
      <div className={styles.cardTop}>
        {logo ? (
          <span className={styles.glyph} data-logo="" aria-hidden="true">
            <svg className={styles.logo} viewBox="0 0 24 24" aria-hidden="true">
              <path d={logo.path} fill="currentColor" />
            </svg>
          </span>
        ) : (
          <span
            className={styles.glyph}
            style={{ background: brand.bg, color: brand.fg ?? "#ffffff" }}
            aria-hidden="true"
          >
            {initial(service.name)}
          </span>
        )}
        <h3 className={styles.cardTitle} title={service.name}>
          {service.name}
        </h3>
      </div>
      <div className={styles.cardBottom}>
        <span className={styles.categoryTag}>
          {t(`serviceCategory.${service.category}` as MessageKey)}
        </span>
        <Toggle
          label={t("blockedServices.toggleAria", { name: service.name })}
          checked={service.globalOn}
          disabled={pending || !canEdit}
          onChange={onToggle}
        />
      </div>
    </article>
  );
}

/** The global blockable-services catalogue: an admin sets which services are
 *  blocked by default for everyone. Per-scope exceptions live in the device,
 *  group and schedule editors. */
export function BlockedServices() {
  const { t } = useI18n();
  const { isAdmin } = useAuth();
  const [services, setServices] = useState<Service[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<string>("all");
  const [search, setSearch] = useState("");

  useEffect(() => {
    const controller = new AbortController();
    api
      .services(controller.signal)
      .then(setServices)
      .catch(() => {
        if (controller.signal.aborted) return;
        setError(t("blockedServices.loadFailed"));
      });
    return () => controller.abort();
  }, [t]);

  async function toggle(service: Service) {
    setPending((prev) => new Set(prev).add(service.id));
    setError(null);
    try {
      setServices(await api.setService(service.id, !service.globalOn));
    } catch (err) {
      setError(err instanceof ApiError && err.status === 403 ? t("common.readOnly") : t("common.saveFailed"));
    } finally {
      setPending((prev) => {
        const next = new Set(prev);
        next.delete(service.id);
        return next;
      });
    }
  }

  // Chips from the categories actually present, each with its count; "Alle" leads.
  const filterChips = useMemo(() => {
    if (!services) return [];
    const counts = services.reduce<Record<string, number>>((acc, service) => {
      acc[service.category] = (acc[service.category] ?? 0) + 1;
      return acc;
    }, {});
    return [
      { key: "all", label: t("blockedServices.filterAll"), count: services.length },
      ...CATEGORY_ORDER.filter((category) => counts[category]).map((category) => ({
        key: category,
        label: t(`serviceCategory.${category}` as MessageKey),
        count: counts[category],
      })),
    ];
  }, [services, t]);

  if (!services) {
    return (
      <div className={styles.page}>
        <h1 className={styles.title}>{t("blockedServices.title")}</h1>
        <p className={styles.lead}>{error ?? t("common.loading")}</p>
      </div>
    );
  }

  const needle = search.trim().toLowerCase();
  const visible = services.filter(
    (service) =>
      (filter === "all" || service.category === filter) &&
      (needle === "" || service.name.toLowerCase().includes(needle)),
  );
  const onCount = services.filter((service) => service.globalOn).length;

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <div>
          <h1 className={styles.title}>{t("blockedServices.title")}</h1>
          <p className={styles.lead}>{t("blockedServices.lead")}</p>
        </div>
        <p className={styles.summary}>
          {t("blockedServices.summary", { n: onCount, total: services.length })}
        </p>
      </div>

      {error ? (
        <p className={styles.banner} role="alert">
          {error}
        </p>
      ) : null}

      <div className={styles.controls}>
        <div className={styles.filters} role="tablist" aria-label={t("blockedServices.filterAria")}>
          {filterChips.map((chip) => (
            <button
              key={chip.key}
              type="button"
              role="tab"
              aria-selected={filter === chip.key}
              className={styles.filter}
              data-active={filter === chip.key || undefined}
              onClick={() => setFilter(chip.key)}
            >
              {chip.label}
              <span className={styles.filterCount}>{chip.count}</span>
            </button>
          ))}
        </div>
        <input
          type="search"
          className={styles.search}
          placeholder={t("blockedServices.search")}
          aria-label={t("blockedServices.searchAria")}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>

      {visible.length === 0 ? (
        <p className={styles.empty}>{t("blockedServices.empty")}</p>
      ) : (
        <div className={styles.cards}>
          {visible.map((service) => (
            <ServiceCard
              key={service.id}
              service={service}
              pending={pending.has(service.id)}
              canEdit={isAdmin}
              onToggle={() => toggle(service)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
