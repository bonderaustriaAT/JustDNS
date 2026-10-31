import { ActivityChart } from "../components/ActivityChart";
import { useOverview } from "../lib/useOverview";
import styles from "./Overview.module.css";

const de = new Intl.NumberFormat("de-DE");

function timeAgo(seconds: number): string {
  if (seconds < 5) return "gerade eben";
  if (seconds < 60) return `vor ${seconds} Sek.`;
  const minutes = Math.round(seconds / 60);
  if (minutes === 1) return "vor 1 Min.";
  if (minutes < 60) return `vor ${minutes} Min.`;
  const hours = Math.round(minutes / 60);
  return hours === 1 ? "vor 1 Std." : `vor ${hours} Std.`;
}

export function Overview() {
  const { data, error, loading } = useOverview();

  if (!data) {
    return (
      <div className={styles.page}>
        <p className={styles.caption}>
          {error ? `Daten nicht erreichbar: ${error}` : loading ? "Lade Daten …" : "Keine Daten."}
        </p>
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <section className={styles.hero}>
        <p className={styles.status}>
          <span className={styles.dot} aria-hidden="true" />
          Schutz aktiv
        </p>
        <h1 className={styles.figure}>{de.format(data.total)}</h1>
        <p className={styles.caption}>Anfragen aufgelöst</p>
      </section>

      <ActivityChart values={data.activity} />

      <dl className={styles.stats}>
        <div className={styles.stat}>
          <dt>Aus dem Cache</dt>
          <dd>{data.cacheRate}&thinsp;%</dd>
        </div>
        <div className={styles.stat}>
          <dt>Anfragen gesamt</dt>
          <dd>{de.format(data.total)}</dd>
        </div>
        <div className={styles.stat}>
          <dt>Aktive Geräte</dt>
          <dd>{data.activeClients}</dd>
        </div>
      </dl>

      <section className={styles.recent}>
        <h2 className={styles.recentTitle}>Zuletzt aufgelöst</h2>
        {data.recent.length === 0 ? (
          <p className={styles.caption}>Noch keine Anfragen aufgezeichnet.</p>
        ) : (
          <ul className={styles.list}>
            {data.recent.map((entry, index) => (
              <li key={`${entry.name}-${index}`} className={styles.row}>
                <span className={styles.domain}>{entry.name}</span>
                <span className={styles.client}>{entry.client}</span>
                <span className={styles.tag}>{entry.type}&thinsp;·&thinsp;{entry.source}</span>
                <span className={styles.ago}>{timeAgo(entry.secondsAgo)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
