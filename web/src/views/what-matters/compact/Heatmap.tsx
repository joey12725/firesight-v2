import type { AssistantChartProps } from '@/assistant/charts';
import type { SweepGrid } from '@/assistant/tools/explore';
import { moneyCompact } from '@/ui';
import { contourLevels, formatContour, formatLevel, type MetricId, type Unit } from '../model';
import { HeatPlot } from '../TradeoffMap';

const UNIT: Record<SweepGrid['metric'], { unit: Unit; better: 1 | -1; like: MetricId; name: string }> = {
  success_rate: { unit: 'share', better: 1, like: 'holds', name: 'Lives in 100 in which the plan holds' },
  depletion_rate: { unit: 'share', better: -1, like: 'short', name: 'Lives in 100 that run short' },
  median_fi_age: { unit: 'age', better: -1, like: 'fi_age', name: 'Age work becomes optional, middle life' },
  median_retirement_age: { unit: 'age', better: -1, like: 'fi_age', name: 'Retirement age, middle life' },
  median_terminal_net_worth: { unit: 'dollars', better: 1, like: 'wealth', name: 'Net worth at the end, middle life' },
  p10_terminal_net_worth: { unit: 'dollars', better: 1, like: 'wealth', name: 'Net worth at the end, 10th of 100 lives' },
  delivered_spending_ratio: { unit: 'ratio', better: 1, like: 'spending', name: 'Spending delivered, % of plan' },
  median_lifetime_tax: { unit: 'dollars', better: -1, like: 'wealth', name: 'Lifetime tax, middle life' },
};

/** A grid axis value in its own unit: "$105k", "55", "3.5%", "1.1x". */
export function axisLabel(unit: string, v: number): string {
  if (unit === 'dollars') return moneyCompact(v);
  if (unit === 'rate') return `${Math.round(v * 1000) / 10}%`;
  if (unit === 'factor') return `${Math.round(v * 100) / 100}x`;
  return String(Math.round(v * 100) / 100);
}

/** `heatmap` in the assistant's conversation: a stored `sweep` grid, each cell printed with its value. */
export default function HeatmapCompact({ grid }: AssistantChartProps) {
  if (!grid || grid.cells.length === 0) return null;
  const m = UNIT[grid.metric];
  const flat = grid.cells.flat().filter((v): v is number => v !== null);
  if (flat.length === 0) return null;
  const xLabels = grid.x.values.map((v) => axisLabel(grid.x.unit, v));
  const yLabels = grid.y ? grid.y.values.map((v) => axisLabel(grid.y?.unit ?? 'number', v)) : [''];
  const contours = grid.y ? contourLevels(m.like, Math.min(...flat), Math.max(...flat)).map((level) => ({ level, label: formatContour(m.like, level) })) : [];
  const describe = (c: number, r: number) => `${grid.x.label} ${xLabels[c]}${grid.y ? `, ${grid.y.label} ${yLabels[r]}` : ''}: ${formatLevel(m.unit, grid.cells[r]?.[c] ?? null)}.`;
  return (
    <div>
      <HeatPlot xLabels={xLabels} yLabels={yLabels} xTitle={grid.x.label} yTitle={grid.y ? grid.y.label : m.name} values={grid.cells} contours={contours} unit={m.unit} tone={m.better > 0 ? 'cool' : 'warm'} label={`${m.name} by ${grid.x.label}${grid.y ? ` and ${grid.y.label}` : ''}`} describe={describe} compact />
      <p className="mt-1 text-2xs leading-4 text-ink-3">
        {m.name}, {grid.paths.toLocaleString('en-US')} lives per cell, the same lives in every cell. A stronger colour is more; neighbouring cells a point or two apart are not distinguishable.
      </p>
    </div>
  );
}
