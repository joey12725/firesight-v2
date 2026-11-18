// "Rich, broke or dead", done properly: the share of all simulated lives in each of the
// six life stages, year by year. Trouble sits on the baseline where its height can be
// read; "died" comes down from the top like a survival curve.
import { useMemo } from 'react';
import type { Status, Summary } from '@/engine/output-types';
import {
  AgeAxis,
  ChartFrame,
  Cursor,
  Legend,
  PlotClip,
  STATUS_LABEL,
  STATUS_SHORT,
  STATUS_STACK,
  SvgLayer,
  TipRow,
  TipTitle,
  ValueAxis,
  inkOn,
  linearScale,
  ofHundred,
  stackSeries,
  statusShares,
  summaryTime,
  textWidth,
  useAnimated,
  useChartSync,
  useChartTheme,
  useTimeRange,
  type TableData,
} from '@/ui/charts';

export function statusTable(summary: Summary): TableData {
  return {
    caption: 'Share of all simulated lives in each life stage, by age',
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      ...[...STATUS_STACK].reverse().map((s) => ({ key: s, label: STATUS_LABEL[s], format: (v: number) => `${(v * 100).toFixed(1)}%` })),
    ],
    rows: summary.axis.ages.map((age, y) => ({
      age: Math.floor(age),
      year: summary.axis.years[y] ?? null,
      ...Object.fromEntries(STATUS_STACK.map((s) => [s, statusShares(summary, s)[y] ?? 0])),
    })),
  };
}

/** The statuses that ever hold a visible share of lives. */
export function presentStatuses(summary: Summary): Status[] {
  return STATUS_STACK.filter((s) => Math.max(0, ...statusShares(summary, s)) >= 0.004);
}

export function StatusLegend({ summary, className }: { summary: Summary; className?: string }) {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend className={className} items={[...presentStatuses(summary)].reverse().map((s) => ({ key: s, label: STATUS_LABEL[s], color: theme.status[s] }))} />
    </div>
  );
}

export function StatusStack({ summary, compact, height }: { summary: Summary; compact?: boolean; height?: number }) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(summary), [summary]);
  const range = useTimeRange(time, sync);
  const target = useMemo(() => STATUS_STACK.map((s) => statusShares(summary, s)), [summary]);
  const shares = useAnimated(target, { resetKey: `${summary.axis.start}:${summary.axis.years.length}`, duration: 360 });
  const tops = useMemo(() => stackSeries(shares), [shares]);
  const { first, last } = range;
  const index = Array.from({ length: last - first + 1 }, (_, k) => first + k);

  return (
    <ChartFrame
      height={(w) => height ?? (compact ? 180 : Math.round(Math.min(340, Math.max(240, w * 0.42))))}
      margin={{ top: compact ? 8 : 22, right: compact ? 8 : 12, bottom: compact ? 22 : 36, left: compact ? 30 : 52 }}
      aria-label="Share of simulated lives in each life stage, by age"
    >
      {(frame) => {
        const { plot, theme } = frame;
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const y = linearScale([0, 1], [plot.bottom, plot.y]);
        const xs = index.map((i) => x(time.ages[i] as number));

        const areas = STATUS_STACK.map((status, s) => {
          const upper = tops[s] ?? [];
          const lower = s === 0 ? null : (tops[s - 1] ?? []);
          let d = '';
          index.forEach((i, k) => (d += `${k ? 'L' : 'M'}${(xs[k] as number).toFixed(1)},${y(upper[i] ?? 0).toFixed(1)}`));
          for (let k = index.length - 1; k >= 0; k--) d += `L${(xs[k] as number).toFixed(1)},${y(lower ? (lower[index[k] as number] ?? 0) : 0).toFixed(1)}`;
          // A direct label where the band is thickest, if the whole name fits inside it there.
          const text = STATUS_SHORT[status];
          const step = Math.max(1, (xs[1] ?? 1) - (xs[0] ?? 0));
          const half = Math.ceil((textWidth(text, 11) / 2 + 5) / step);
          const edge = (k: number) => {
            const i = index[Math.min(index.length - 1, Math.max(0, k))] as number;
            return { top: y(upper[i] ?? 0), bottom: y(lower ? (lower[i] ?? 0) : 0) };
          };
          let label: { x: number; y: number; text: string } | null = null;
          let room = 15;
          for (let k = half; k < index.length - half; k++) {
            // The tallest box that stays inside the band across the label's width.
            let top = -Infinity;
            let bottom = Infinity;
            for (let j = k - half; j <= k + half; j++) {
              const e = edge(j);
              top = Math.max(top, e.top);
              bottom = Math.min(bottom, e.bottom);
            }
            if (bottom - top > room) {
              room = bottom - top;
              label = { x: xs[k] as number, y: (top + bottom) / 2 + 4, text };
            }
          }
          return { status, d, label };
        });

        return (
          <>
            <SvgLayer>
              <PlotClip>
                {areas.map((a) => (
                  <path key={a.status} d={a.d} fill={theme.status[a.status]} stroke={theme.surface} strokeWidth={1.5} strokeLinejoin="round" />
                ))}
              </PlotClip>
              <ValueAxis y={y} format="share100" ticks={[0, 0.25, 0.5, 0.75, 1]} title={compact ? undefined : 'Lives, of 100'} grid={false} />
              <AgeAxis x={x} time={time} years={!compact} label={compact ? '' : 'Age'} />
              <g fontSize={11} fontWeight={500}>
                {areas.map(
                  (a) =>
                    a.label && (
                      <text key={a.status} x={a.label.x} y={a.label.y} textAnchor="middle" fill={inkOn(theme.status[a.status])}>
                        {a.label.text}
                      </text>
                    ),
                )}
              </g>
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label="Life stages by age"
              describe={(k) => {
                const i = index[k] as number;
                const parts = [...STATUS_STACK]
                  .reverse()
                  .map((s) => ({ s, v: target[STATUS_STACK.indexOf(s)]?.[i] ?? 0 }))
                  .filter((p) => p.v >= 0.005)
                  .map((p) => `${ofHundred(p.v).n} ${STATUS_LABEL[p.s].toLowerCase()}`);
                return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}. Of 100 lives: ${parts.join(', ')}.`;
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {[...STATUS_STACK].reverse().map((s) => {
                      const v = target[STATUS_STACK.indexOf(s)]?.[i] ?? 0;
                      return v < 0.0005 ? null : <TipRow key={s} color={theme.status[s]} swatch="box" label={STATUS_LABEL[s]} value={ofHundred(v).text} />;
                    })}
                  </>
                );
              }}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}
