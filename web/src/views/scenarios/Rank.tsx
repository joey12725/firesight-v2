// All enabled scenarios on one measure: the paired difference from the plan as a dot
// with its 95% interval, best first, against a zero line that is the plan itself. A
// scenario whose interval crosses zero is drawn hollow and grey and says "within noise":
// its place in the order means nothing.
import { useMemo } from 'react';
import { ChartFrame, SvgLayer, linearScale, niceDomain, textWidth, type TableData } from '@/ui/charts';
import { IntervalMark, ValueAxisX, ZeroRule, zeroTicks } from '@/ui/charts/ext/scenarios-marks';
import type { ScenarioComparison } from '../types';
import { formatDelta, formatDeltaTick, rankRows, type MetricDef, type RankRow } from './model';

export function rankTable(comparison: ScenarioComparison[], metric: MetricDef): TableData {
  const scale = metric.unit === 'share' ? 100 : 1;
  return {
    caption: `${metric.axis}: each scenario minus the plan over the same simulated lives, with its 95% interval`,
    columns: [
      { key: 'scenario', label: 'Scenario', align: 'left' },
      { key: 'value', label: metric.unit === 'share' ? 'Difference, lives per 100' : metric.unit === 'age' ? 'Average difference, years' : 'Average difference, dollars' },
      { key: 'lo', label: '95% interval, low' },
      { key: 'hi', label: '95% interval, high' },
      { key: 'paired', label: 'Lives paired' },
      { key: 'noise', label: 'Verdict', align: 'left' },
    ],
    rows: rankRows(comparison, metric).map((r) => ({
      scenario: r.name,
      value: r.value === null ? null : r.value * scale,
      lo: r.lo === null ? null : r.lo * scale,
      hi: r.hi === null ? null : r.hi * scale,
      paired: r.paired || null,
      noise: r.pending ? 'still running' : r.value === null ? 'no life to pair' : r.withinNoise ? 'within noise' : 'differs from the plan',
    })),
  };
}

const clip = (text: string, width: number, size = 12) => {
  if (textWidth(text, size) <= width) return text;
  let t = text;
  while (t.length > 4 && textWidth(`${t}…`, size) > width) t = t.slice(0, -1);
  return `${t.trimEnd()}…`;
};

export interface RankProps {
  comparison: ScenarioComparison[];
  metric: MetricDef;
  selected: string | null;
  onSelect: (id: string) => void;
}

export function Rank({ comparison, metric, selected, onSelect }: RankProps) {
  const rows = useMemo(() => rankRows(comparison, metric), [comparison, metric]);
  const top = 26;
  const bottom = 42;
  return (
    <ChartFrame
      height={(w) => top + bottom + rows.length * (w < 520 ? 50 : 38)}
      margin={{ top, right: 0, bottom, left: 0 }}
      aria-label={`${metric.axis}, each scenario against the plan`}
    >
      {(frame) => {
        const { theme, width } = frame;
        const stacked = width < 520;
        const rowHeight = stacked ? 50 : 38;
        const nameWidth = stacked ? 0 : Math.min(Math.round(width * 0.3), Math.max(90, ...rows.map((r) => textWidth(r.name, 12) + 14)));
        const valueWidth = stacked ? 0 : Math.max(96, ...rows.map((r) => textWidth(label(r, metric), 12) + textWidth(note(r), 11) + 18));
        const x0 = nameWidth + 6;
        const x1 = width - valueWidth - 6;
        const values = rows.flatMap((r) => (r.value === null ? [] : [r.lo ?? r.value, r.hi ?? r.value, r.value]));
        const [lo, hi] = niceDomain(Math.min(0, ...values), Math.max(0, ...values), 4);
        const pad = (hi - lo || 1) * 0.04;
        const x = linearScale(lo === hi ? [-1, 1] : [lo - pad, hi + pad], [x0, x1]);
        const ticks = lo === hi ? [0] : zeroTicks(x, Math.max(2, Math.floor((x1 - x0) / 90)));
        const plotBottom = top + rows.length * rowHeight;
        return (
          <>
            <SvgLayer>
              <ValueAxisX x={x} ticks={ticks} format={(v) => formatDeltaTick(v, metric.unit)} y={plotBottom} title={metric.axis} rule={false} />
              <ZeroRule x={x(0)} label="The plan" y0={top} y1={plotBottom} />
              {rows.map((r, i) => {
                const yTop = top + i * rowHeight;
                const y = yTop + (stacked ? 34 : rowHeight / 2);
                const active = r.id === selected;
                return (
                  <g key={r.id} fontSize={12}>
                    {active && <rect x={0} y={yTop + 2} width={width} height={rowHeight - 4} rx={4} fill={theme.ink.tertiary} opacity={0.1} />}
                    <text x={stacked ? 4 : nameWidth} y={stacked ? yTop + 17 : y + 4} textAnchor={stacked ? 'start' : 'end'} fill={theme.ink.primary} fontWeight={active ? 600 : 400}>
                      {clip(r.name, stacked ? width * 0.55 : nameWidth - 6)}
                    </text>
                    {r.value !== null && r.lo !== null && r.hi !== null ? (
                      <IntervalMark x={x(r.value)} lo={x(r.lo)} hi={x(r.hi)} y={y} color={theme.scenario.line} hollow={r.withinNoise} />
                    ) : (
                      <line x1={x0} x2={x1} y1={y} y2={y} stroke={theme.grid} strokeWidth={1} strokeDasharray="2 4" />
                    )}
                    <text x={stacked ? width - 4 : x1 + 14} y={stacked ? yTop + 17 : y + 4} textAnchor={stacked ? 'end' : 'start'} fill={r.value === null ? theme.ink.tertiary : theme.ink.primary} stroke={theme.surface} strokeWidth={4} paintOrder="stroke" strokeLinejoin="round" style={{ fontVariantNumeric: 'tabular-nums' }}>
                      <tspan fontWeight={r.value !== null && !r.withinNoise ? 600 : 400}>{label(r, metric)}</tspan>
                      <tspan fontSize={11} fill={theme.label}>
                        {note(r) ? ` ${note(r)}` : ''}
                      </tspan>
                    </text>
                  </g>
                );
              })}
            </SvgLayer>
            {/* One button per row: the rows are the way into the life-by-life comparison. */}
            {rows.map((r, i) => (
              <button
                key={r.id}
                type="button"
                aria-pressed={r.id === selected}
                aria-label={`${r.name}: ${label(r, metric)}${r.value !== null && r.lo !== null && r.hi !== null ? `, 95% interval ${formatDeltaTick(r.lo, metric.unit)} to ${formatDeltaTick(r.hi, metric.unit)}` : ''}${note(r) ? `, ${note(r)}` : ''}. Compare with the plan in detail.`}
                onClick={() => onSelect(r.id)}
                className="absolute left-0 w-full cursor-pointer rounded-sm hover:bg-ink-1/[0.04]"
                style={{ top: top + i * rowHeight + 2, height: rowHeight - 4 }}
              />
            ))}
          </>
        );
      }}
    </ChartFrame>
  );
}

function label(r: RankRow, metric: MetricDef): string {
  if (r.pending) return 'Running…';
  if (r.value === null) return 'No life to pair';
  return formatDelta(r.value, metric.unit);
}

function note(r: RankRow): string {
  return r.value !== null && r.withinNoise ? 'within noise' : '';
}
