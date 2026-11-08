import { useLayoutEffect, useMemo, useRef, useState } from "react";

import { useI18n } from "../lib/i18n";
import styles from "./ActivityChart.module.css";

interface ActivityChartProps {
  /** Queries per hour over the last values.length hours, oldest first. */
  values: number[];
}

const HEIGHT = 150;
const PAD_X = 10;
const PAD_TOP = 12;
const PAD_BOTTOM = 28;
const HOUR_MS = 3_600_000;

function hourLabel(ms: number): string {
  return `${new Date(ms).getHours().toString().padStart(2, "0")}:00`;
}

export function ActivityChart({ values }: ActivityChartProps) {
  const { t, locale } = useI18n();
  const nf = useMemo(() => new Intl.NumberFormat(locale), [locale]);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(640);
  const [hover, setHover] = useState<number | null>(null);

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    // Measure synchronously before the first paint: relying on the observer's
    // first callback alone can leave the chart stuck at its seed width, which
    // then overflows a narrow (phone) container and widens the whole page.
    const measure = () => {
      const w = el.getBoundingClientRect().width;
      if (w) setWidth(w);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Anchor the trace to real clock hours ending at the current hour.
  const endOfRange = useMemo(() => {
    const d = new Date();
    d.setMinutes(0, 0, 0);
    return d.getTime();
  }, []);

  const n = values.length;
  const max = Math.max(...values, 1);
  const plotW = Math.max(width - PAD_X * 2, 1);
  const plotH = HEIGHT - PAD_TOP - PAD_BOTTOM;
  const baseY = PAD_TOP + plotH;

  const x = (i: number) => PAD_X + (i / (n - 1)) * plotW;
  const y = (v: number) => PAD_TOP + plotH - (v / max) * plotH;
  const timeAt = (i: number) => endOfRange - (n - 1 - i) * HOUR_MS;

  const points = values.map((v, i) => [x(i), y(v)] as const);
  const line = points.map(([px, py], i) => `${i === 0 ? "M" : "L"}${px.toFixed(1)} ${py.toFixed(1)}`).join(" ");
  const area = `${line} L${x(n - 1).toFixed(1)} ${baseY} L${x(0).toFixed(1)} ${baseY} Z`;

  // A handful of time ticks across the axis, always including the latest hour.
  const tickStep = Math.max(1, Math.round((n - 1) / 4));
  const ticks: number[] = [];
  for (let i = 0; i < n - 1; i += tickStep) ticks.push(i);
  ticks.push(n - 1);

  function handleMove(event: React.PointerEvent<HTMLDivElement>) {
    const rect = wrapRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mx = event.clientX - rect.left;
    const i = Math.round(((mx - PAD_X) / plotW) * (n - 1));
    setHover(Math.min(n - 1, Math.max(0, i)));
  }

  const summary = t("chart.summary", { n, max: nf.format(max) });

  return (
    <figure
      className={styles.wrap}
      ref={wrapRef}
      onPointerMove={handleMove}
      onPointerLeave={() => setHover(null)}
      style={{ height: HEIGHT }}
    >
      <svg width={width} height={HEIGHT} role="img" aria-label={summary} className={styles.svg}>
        <path d={area} className={styles.area} />
        <path d={line} className={styles.line} />

        <line x1={PAD_X} y1={baseY} x2={width - PAD_X} y2={baseY} className={styles.axis} />
        {ticks.map((i) => (
          <text
            key={i}
            x={x(i)}
            y={HEIGHT - 8}
            className={styles.tick}
            textAnchor={i === 0 ? "start" : i === n - 1 ? "end" : "middle"}
          >
            {hourLabel(timeAt(i))}
          </text>
        ))}

        <circle cx={x(n - 1)} cy={y(values[n - 1])} r={3.5} className={styles.last} />

        {hover !== null && (
          <>
            <line x1={x(hover)} y1={PAD_TOP} x2={x(hover)} y2={baseY} className={styles.guide} />
            <circle cx={x(hover)} cy={y(values[hover])} r={4.5} className={styles.marker} />
          </>
        )}
      </svg>

      {hover !== null && (
        <div
          className={styles.tip}
          style={{
            left: Math.min(Math.max(x(hover), 54), width - 54),
            top: Math.max(y(values[hover]) - 14, 4),
          }}
        >
          <span className={styles.tipValue}>{nf.format(values[hover])}</span>
          <span className={styles.tipMeta}>{t("chart.tipMeta", { time: hourLabel(timeAt(hover)) })}</span>
        </div>
      )}

      <figcaption className={styles.caption}>{t("chart.caption", { n })}</figcaption>
    </figure>
  );
}
