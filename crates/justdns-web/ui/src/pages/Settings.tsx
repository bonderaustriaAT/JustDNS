import { useEffect, useMemo, useState, type ReactNode } from "react";

import { Toggle } from "../components/Toggle";
import { useSettings, type UpstreamMode } from "../state/settings";
import styles from "./Settings.module.css";

interface CategoryMeta {
  id: string;
  title: string;
}

function Category({
  id,
  title,
  description,
  children,
}: {
  id: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section id={id} className={styles.category} aria-labelledby={`${id}-title`}>
      <div className={styles.categoryHead}>
        <h2 id={`${id}-title`} className={styles.categoryTitle}>
          {title}
        </h2>
        <p className={styles.categoryDescription}>{description}</p>
      </div>
      <div className={styles.rows}>{children}</div>
    </section>
  );
}

function Row({
  label,
  description,
  control,
}: {
  label: string;
  description: string;
  control: ReactNode;
}) {
  return (
    <div className={styles.row}>
      <div className={styles.text}>
        <span className={styles.label}>{label}</span>
        <span className={styles.description}>{description}</span>
      </div>
      <div className={styles.control}>{control}</div>
    </div>
  );
}

const MODE_LABELS: Record<UpstreamMode, string> = {
  racing: "Schnellste Antwort (Racing)",
  fallback: "Der Reihe nach (Fallback)",
  balanced: "Lastverteilt",
  rules: "Regelbasiert",
};

const RESOLVERS = [
  { ip: "86.54.11.1", host: "protective.joindns4.eu", name: "DNS4EU" },
  { ip: "9.9.9.9", host: "dns.quad9.net", name: "Quad9" },
];

// Highlights the category the reader has scrolled to, so the rail doubles as a
// "you are here" marker.
function useActiveCategory(ids: string[]): string {
  const [active, setActive] = useState(ids[0] ?? "");

  useEffect(() => {
    const sections = ids
      .map((id) => document.getElementById(id))
      .filter((el): el is HTMLElement => el !== null);
    if (sections.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "-20% 0px -70% 0px" },
    );

    sections.forEach((section) => observer.observe(section));
    return () => observer.disconnect();
  }, [ids]);

  return active;
}

export function Settings() {
  const { settings, update } = useSettings();

  const categories = useMemo<CategoryMeta[]>(() => {
    const base: CategoryMeta[] = [{ id: "schutz", title: "Schutz" }];
    if (settings.expertMode) {
      base.push(
        { id: "upstream", title: "Upstream-Resolver" },
        { id: "verschluesselung", title: "Verschlüsselung" },
        { id: "cache", title: "Cache" },
        { id: "protokoll", title: "Protokollierung" },
      );
    }
    return base;
  }, [settings.expertMode]);

  const active = useActiveCategory(categories.map((c) => c.id));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Einstellungen</h1>

      <section className={styles.expertBand}>
        <div className={styles.text}>
          <span className={styles.label}>Expertenmodus</span>
          <span className={styles.description}>
            Blendet die technischen Kategorien ein — Upstream-Resolver, Verschlüsselung, Cache und
            Protokollierung. Für den reinen Werbeblocker nicht nötig.
          </span>
        </div>
        <Toggle
          label="Expertenmodus"
          checked={settings.expertMode}
          onChange={(v) => update("expertMode", v)}
        />
      </section>

      <div className={styles.layout}>
        <nav className={styles.rail} aria-label="Kategorien">
          {categories.map((category) => (
            <a
              key={category.id}
              href={`#${category.id}`}
              className={active === category.id ? styles.railActive : styles.railLink}
            >
              {category.title}
            </a>
          ))}
        </nav>

        <div className={styles.sections}>
          <Category
            id="schutz"
            title="Schutz"
            description="Was für alle Geräte im Netzwerk gefiltert wird."
          >
            <Row
              label="Werbung & Tracker blockieren"
              description="Filtert bekannte Werbe- und Trackingdienste."
              control={
                <Toggle
                  label="Werbung & Tracker blockieren"
                  checked={settings.blockAdsTrackers}
                  onChange={(v) => update("blockAdsTrackers", v)}
                />
              }
            />
            <Row
              label="Schadsoftware blockieren"
              description="Sperrt Domains, die für Phishing und Schadsoftware bekannt sind."
              control={
                <Toggle
                  label="Schadsoftware blockieren"
                  checked={settings.blockMalware}
                  onChange={(v) => update("blockMalware", v)}
                />
              }
            />
            <Row
              label="Sichere Suche erzwingen"
              description="Erzwingt den Jugendschutzmodus bei großen Suchmaschinen."
              control={
                <Toggle
                  label="Sichere Suche erzwingen"
                  checked={settings.safeSearch}
                  onChange={(v) => update("safeSearch", v)}
                />
              }
            />
          </Category>

          {settings.expertMode ? (
            <>
              <Category
                id="upstream"
                title="Upstream-Resolver"
                description="An welche Resolver Anfragen weitergereicht werden und wie gewählt wird."
              >
                <Row
                  label="Auswahlmodus"
                  description="Wie zwischen den konfigurierten Resolvern gewählt wird."
                  control={
                    <select
                      className={styles.select}
                      aria-label="Auswahlmodus"
                      value={settings.upstreamMode}
                      onChange={(e) => update("upstreamMode", e.target.value as UpstreamMode)}
                    >
                      {(Object.keys(MODE_LABELS) as UpstreamMode[]).map((mode) => (
                        <option key={mode} value={mode}>
                          {MODE_LABELS[mode]}
                        </option>
                      ))}
                    </select>
                  }
                />
                <ul className={styles.resolvers}>
                  {RESOLVERS.map((resolver) => (
                    <li key={resolver.ip} className={styles.resolver}>
                      <span className={styles.resolverIp}>{resolver.ip}</span>
                      <span className={styles.resolverHost}>{resolver.host}</span>
                      <span className={styles.resolverTag}>
                        {settings.encryptUpstream ? "DoT" : "Klartext"}
                      </span>
                    </li>
                  ))}
                </ul>
              </Category>

              <Category
                id="verschluesselung"
                title="Verschlüsselung"
                description="Wie Anfragen an die Resolver übertragen werden."
              >
                <Row
                  label="Upstream verschlüsseln"
                  description="Anfragen über DNS-over-TLS statt im Klartext senden."
                  control={
                    <Toggle
                      label="Upstream verschlüsseln"
                      checked={settings.encryptUpstream}
                      onChange={(v) => update("encryptUpstream", v)}
                    />
                  }
                />
              </Category>

              <Category
                id="cache"
                title="Cache"
                description="Wie zwischengespeicherte Antworten behandelt werden."
              >
                <Row
                  label="Abgelaufene Einträge weiterreichen"
                  description="Serve-Stale: bei nicht erreichbarem Upstream vorübergehend abgelaufene Antworten ausliefern."
                  control={
                    <Toggle
                      label="Abgelaufene Einträge weiterreichen"
                      checked={settings.cacheServeStale}
                      onChange={(v) => update("cacheServeStale", v)}
                    />
                  }
                />
              </Category>

              <Category
                id="protokoll"
                title="Protokollierung"
                description="Was über aufgelöste Anfragen gespeichert wird."
              >
                <Row
                  label="Anfrageprotokoll"
                  description="Speichert aufgelöste Anfragen für den Abfrage-Explorer."
                  control={
                    <Toggle
                      label="Anfrageprotokoll"
                      checked={settings.queryLog}
                      onChange={(v) => update("queryLog", v)}
                    />
                  }
                />
                <Row
                  label="Aufbewahrung"
                  description="Wie lange Protokolleinträge gespeichert bleiben."
                  control={
                    <label className={styles.number}>
                      <input
                        type="number"
                        min={1}
                        max={365}
                        value={settings.logRetentionDays}
                        onChange={(e) => update("logRetentionDays", Number(e.target.value))}
                        aria-label="Aufbewahrung in Tagen"
                      />
                      <span>Tage</span>
                    </label>
                  }
                />
              </Category>
            </>
          ) : (
            <p className={styles.hint}>
              Für die technischen Kategorien den Expertenmodus einschalten.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
