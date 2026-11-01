import { useMemo, useState, type ReactNode } from "react";

import { InfoHint } from "../components/InfoHint";
import { Toggle } from "../components/Toggle";
import { api } from "../lib/api";
import { useSettings, type UpstreamMode } from "../state/settings";
import styles from "./Settings.module.css";

/** The "clear DNS cache" action: a one-shot button that drops every cached
 *  answer, with a short confirmation once it is done. */
function ClearCacheButton() {
  const [state, setState] = useState<"idle" | "busy" | "done" | "error">("idle");

  async function clear() {
    setState("busy");
    try {
      await api.clearCache();
      setState("done");
      window.setTimeout(() => setState("idle"), 2500);
    } catch {
      setState("error");
    }
  }

  return (
    <div className={styles.action}>
      <button
        type="button"
        className={styles.actionButton}
        onClick={clear}
        disabled={state === "busy"}
      >
        {state === "busy" ? "Wird geleert …" : "DNS-Cache leeren"}
      </button>
      {state === "done" ? <span className={styles.actionDone}>Cache geleert.</span> : null}
      {state === "error" ? (
        <span className={styles.actionError}>Ließ sich nicht leeren.</span>
      ) : null}
    </div>
  );
}

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
    <section className={styles.category} aria-labelledby={`${id}-title`}>
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
  details,
  control,
  disabled = false,
}: {
  label: string;
  description: string;
  details: string;
  control: ReactNode;
  disabled?: boolean;
}) {
  return (
    <div className={styles.row} data-disabled={disabled || undefined}>
      <div className={styles.text}>
        <span className={styles.labelLine}>
          <span className={styles.label}>{label}</span>
          <InfoHint label={label} text={details} />
        </span>
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

export function Settings() {
  const { settings, update } = useSettings();
  const [selected, setSelected] = useState("schutz");

  const categories = useMemo<CategoryMeta[]>(() => {
    const base: CategoryMeta[] = [{ id: "schutz", title: "Schutz" }];
    if (settings.expertMode) {
      base.push(
        { id: "upstream", title: "Upstream-Resolver" },
        { id: "verschluesselung", title: "Verschlüsselung" },
        { id: "netzwerk", title: "Netzwerk" },
        { id: "cache", title: "Cache" },
        { id: "protokoll", title: "Protokollierung" },
      );
    }
    return base;
  }, [settings.expertMode]);

  // Fall back to the first category when the selected one is hidden (e.g. the
  // expert mode that revealed it was switched off).
  const active = categories.some((c) => c.id === selected) ? selected : categories[0].id;

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
            <button
              key={category.id}
              type="button"
              className={active === category.id ? styles.railActive : styles.railLink}
              aria-current={active === category.id || undefined}
              onClick={() => setSelected(category.id)}
            >
              {category.title}
            </button>
          ))}
          {!settings.expertMode && (
            <p className={styles.railHint}>
              Für die technischen Kategorien den Expertenmodus einschalten.
            </p>
          )}
        </nav>

        <div className={styles.sections}>
          {active === "schutz" && (
            <Category
              id="schutz"
              title="Schutz"
              description="Was für alle Geräte im Netzwerk gefiltert wird."
            >
              <Row
                label="Werbung & Tracker blockieren"
                description="Filtert bekannte Werbe- und Trackingdienste."
                details="Gleicht jede DNS-Anfrage gegen Listen bekannter Werbe- und Tracking-Domains ab. Treffer werden nicht aufgelöst, sondern geblockt — für alle Geräte im Netzwerk, ohne dort etwas zu installieren."
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
                details="Vergleicht Anfragen mit Listen von Phishing-, Malware- und Betrugs-Domains. Verbindungen zu diesen Zielen werden gar nicht erst aufgebaut, weil der Name nicht aufgelöst wird."
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
                details="Leitet Google, Bing und YouTube per DNS auf ihre jugendsicheren Adressen um, sodass explizite Ergebnisse ausgefiltert werden — unabhängig von den Einstellungen am einzelnen Gerät."
                control={
                  <Toggle
                    label="Sichere Suche erzwingen"
                    checked={settings.safeSearch}
                    onChange={(v) => update("safeSearch", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "upstream" && (
            <Category
              id="upstream"
              title="Upstream-Resolver"
              description="An welche Resolver Anfragen weitergereicht werden und wie gewählt wird."
            >
              <Row
                label="Auswahlmodus"
                description="Wie zwischen den konfigurierten Resolvern gewählt wird."
                details="Racing fragt alle Resolver gleichzeitig und nimmt die erste Antwort. Fallback geht sie der Reihe nach durch. Lastverteilt wechselt reihum. Regelbasiert wählt den Resolver anhand von Domain-Regeln."
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
                    <span
                      className={styles.resolverTag}
                      data-warn={!settings.encryptUpstream || undefined}
                    >
                      {settings.encryptUpstream ? "DoT" : "Klartext"}
                    </span>
                  </li>
                ))}
              </ul>
            </Category>
          )}

          {active === "verschluesselung" && (
            <Category
              id="verschluesselung"
              title="Verschlüsselung"
              description="Wie Anfragen an die Resolver übertragen werden."
            >
              <Row
                label="Upstream verschlüsseln"
                description="Anfragen über DNS-over-TLS statt im Klartext senden."
                details="Sendet Anfragen an die Resolver über DNS-over-TLS (Port 853), sodass niemand im Netzweg mitlesen kann, welche Domains abgefragt werden. Ausgeschaltet gehen die Anfragen im Klartext über Port 53."
                control={
                  <Toggle
                    label="Upstream verschlüsseln"
                    checked={settings.encryptUpstream}
                    onChange={(v) => update("encryptUpstream", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "netzwerk" && (
            <Category
              id="netzwerk"
              title="Netzwerk"
              description="Wie der Resolver mit IP-Protokollen umgeht."
            >
              <Row
                label="IPv6 (AAAA-Antworten)"
                description="AAAA-Anfragen beantworten. Standardmäßig aus, Geräte nutzen dann IPv4."
                details="Ausgeschaltet beantwortet JustDNS AAAA-Anfragen mit einer leeren Antwort, sodass Geräte auf IPv4 ausweichen. Einschalten, wenn im Netzwerk echtes IPv6 genutzt werden soll — dann werden AAAA-Anfragen an die Resolver weitergereicht."
                control={
                  <Toggle
                    label="IPv6 (AAAA-Antworten)"
                    checked={settings.ipv6}
                    onChange={(v) => update("ipv6", v)}
                  />
                }
              />
            </Category>
          )}

          {active === "cache" && (
            <Category
              id="cache"
              title="Cache"
              description="Wie zwischengespeicherte Antworten behandelt werden."
            >
              <Row
                label="Abgelaufene Einträge weiterreichen"
                description="Serve-Stale: bei nicht erreichbarem Upstream vorübergehend abgelaufene Antworten ausliefern."
                details="Ist kein Resolver erreichbar, liefert der Cache vorübergehend bereits abgelaufene Antworten weiter, damit die Namensauflösung bei kurzen Ausfällen nicht komplett stoppt."
                control={
                  <Toggle
                    label="Abgelaufene Einträge weiterreichen"
                    checked={settings.cacheServeStale}
                    onChange={(v) => update("cacheServeStale", v)}
                  />
                }
              />
              <Row
                label="DNS-Cache leeren"
                description="Verwirft alle zwischengespeicherten Antworten sofort."
                details="Löscht den gesamten Antwort-Cache. Die nächste Anfrage je Name wird wieder frisch beim Upstream geholt oder von einem Rewrite beantwortet. Nützlich, wenn sich eine IP geändert hat oder ein Rewrite sofort greifen soll."
                control={<ClearCacheButton />}
              />
            </Category>
          )}

          {active === "protokoll" && (
            <Category
              id="protokoll"
              title="Protokollierung"
              description="Was über aufgelöste Anfragen gespeichert wird."
            >
              <Row
                label="Anfrageprotokoll"
                description="Speichert aufgelöste Anfragen für den Abfrage-Explorer."
                details="Hält zu jeder Anfrage Zeitpunkt, Gerät, Domain und das Ergebnis (aufgelöst oder geblockt) fest, damit der Abfrage-Explorer sie später anzeigen kann."
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
                details="Legt fest, nach wie vielen Tagen Protokolleinträge automatisch gelöscht werden. Nur einstellbar, wenn das Anfrageprotokoll aktiv ist."
                disabled={!settings.queryLog}
                control={
                  <label className={styles.number}>
                    <input
                      type="number"
                      min={1}
                      max={365}
                      value={settings.logRetentionDays}
                      disabled={!settings.queryLog}
                      onChange={(e) => update("logRetentionDays", Number(e.target.value))}
                      aria-label="Aufbewahrung in Tagen"
                    />
                    <span>Tage</span>
                  </label>
                }
              />
            </Category>
          )}
        </div>
      </div>
    </div>
  );
}
