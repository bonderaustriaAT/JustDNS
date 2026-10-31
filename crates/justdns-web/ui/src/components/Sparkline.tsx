interface SparklineProps {
  values: number[];
  width?: number;
  height?: number;
}

// A compact activity trace. Drawn as a crisp line over a faint fill; no axes,
// no decoration, just the shape of the last day's traffic.
export function Sparkline({ values, width = 520, height = 120 }: SparklineProps) {
  if (values.length < 2) return null;

  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const stepX = width / (values.length - 1);
  const pad = 6;
  const usable = height - pad * 2;

  const points = values.map((value, index) => {
    const x = index * stepX;
    const y = pad + usable - ((value - min) / span) * usable;
    return [x, y] as const;
  });

  const line = points.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)} ${y.toFixed(1)}`).join(" ");
  const area = `${line} L${width} ${height} L0 ${height} Z`;
  const [lastX, lastY] = points[points.length - 1];

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      height={height}
      preserveAspectRatio="none"
      role="img"
      aria-label="Anfragen pro Stunde in den letzten 24 Stunden"
    >
      <path d={area} fill="var(--signal)" opacity="0.1" />
      <path d={line} fill="none" stroke="var(--signal)" strokeWidth="2" strokeLinejoin="round" />
      <circle cx={lastX} cy={lastY} r="3.5" fill="var(--signal)" />
    </svg>
  );
}
