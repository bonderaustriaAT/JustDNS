import { Sparkline } from "../components/Sparkline";
import { blockRate, overview } from "../lib/mockData";
import styles from "./Overview.module.css";

const de = new Intl.NumberFormat("de-DE");

function timeAgo(minutes: number): string {
  if (minutes < 1) return "gerade eben";
  if (minutes === 1) return "vor 1 Min.";
  return `vor ${minutes} Min.`;
}

export function Overview() {
  const data = overview;

  return (
    <div className={styles.page}>
      <section className={styles.hero}>
        <p className={styles.status}>
          <span className={styles.dot} aria-hidden="true" />
          Schutz aktiv
        </p>
        <h1 className={styles.figure}>{de.format(data.blockedToday)}</h1>
        <p className={styles.caption}>Anfragen heute blockiert</p>
      </section>

      <section className={styles.activity} aria-label="Aktivität der letzten 24 Stunden">
        <Sparkline values={data.activity} />
        <p className={styles.activityNote}>Anfragen pro Stunde, letzte 24 Stunden</p>
      </section>

      <dl className={styles.stats}>
        <div className={styles.stat}>
          <dt>Anfragen heute</dt>
          <dd>{de.format(data.queriesToday)}</dd>
        </div>
        <div className={styles.stat}>
          <dt>Blockrate</dt>
          <dd>{blockRate(data)}&thinsp;%</dd>
        </div>
        <div className={styles.stat}>
          <dt>Aktive Geräte</dt>
          <dd>{data.activeClients}</dd>
        </div>
      </dl>

      <section className={styles.recent}>
        <h2 className={styles.recentTitle}>Zuletzt blockiert</h2>
        <ul className={styles.list}>
          {data.recentBlocks.map((entry) => (
            <li key={`${entry.domain}-${entry.minutesAgo}`} className={styles.row}>
              <span className={styles.domain}>{entry.domain}</span>
              <span className={styles.client}>{entry.client}</span>
              <span className={styles.tag}>{entry.list}</span>
              <span className={styles.ago}>{timeAgo(entry.minutesAgo)}</span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
