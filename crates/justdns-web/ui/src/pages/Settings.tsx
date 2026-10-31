import type { ReactNode } from "react";

import { Toggle } from "../components/Toggle";
import { useSettings, type UpstreamMode } from "../state/settings";
import styles from "./Settings.module.css";

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className={styles.group}>
      <h2 className={styles.groupTitle}>{title}</h2>
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

export function Settings() {
  const { settings, update } = useSettings();

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Einstellungen</h1>

      <Group title="Schutz">
        <Row
          label="Werbung & Tracker blockieren"
          description="Filtert bekannte Werbe- und Trackingdienste für alle Geräte im Netzwerk."
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
      </Group>

      <section className={styles.expertBand}>
        <div className={styles.text}>
          <span className={styles.label}>Expertenmodus</span>
          <span className={styles.description}>
            Blendet technische Einstellungen ein — Upstream-Resolver, Verschlüsselung, Cache und
            Protokollierung. Für den reinen Werbeblocker nicht nötig.
          </span>
        </div>
        <Toggle
          label="Expertenmodus"
          checked={settings.expertMode}
          onChange={(v) => update("expertMode", v)}
        />
      </section>

      {settings.expertMode ? (
        <>
          <Group title="Upstream-Resolver">
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
            <div className={styles.readout}>
              <span>86.54.11.1</span>
              <span>protective.joindns4.eu</span>
            </div>
            <div className={styles.readout}>
              <span>9.9.9.9</span>
              <span>dns.quad9.net</span>
            </div>
          </Group>

          <Group title="Verschlüsselung">
            <Row
              label="Upstream verschlüsseln"
              description="Anfragen an die Resolver über DNS-over-TLS statt im Klartext senden."
              control={
                <Toggle
                  label="Upstream verschlüsseln"
                  checked={settings.encryptUpstream}
                  onChange={(v) => update("encryptUpstream", v)}
                />
              }
            />
          </Group>

          <Group title="Cache">
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
          </Group>

          <Group title="Protokollierung">
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
          </Group>
        </>
      ) : (
        <p className={styles.hint}>
          Für technische Einstellungen den Expertenmodus einschalten.
        </p>
      )}
    </div>
  );
}
