// One year of money as a flow: where the average household's money comes from on the
// left, where it goes on the right. The year is chosen by clicking the chart above, by
// the scrubber, or by one of the plan's moments.
import { useMemo, type CSSProperties } from 'react';
import { IconButton, cx, moneyCompact } from '@/ui';
import { useChartTheme, type TableData } from '@/ui/charts';
import { FlowDiagram, type FlowNode } from '@/ui/charts/ext/cashflow-FlowDiagram';
import type { CashFlowModel, YearNode } from './model';

export interface YearJump {
  index: number;
  label: string;
}

export function yearTable(model: CashFlowModel, nodes: { sources: YearNode[]; uses: YearNode[] }, i: number): TableData {
  const totalIn = model.totalIn[i] ?? 0;
  const totalOut = model.totalOut[i] ?? 0;
  const side = (list: YearNode[], name: string, total: number) => list.filter((n) => (n.values[i] ?? 0) > 0).map((n) => ({ side: name, item: n.label, amount: Math.round(n.values[i] ?? 0), share: total > 0 ? (n.values[i] ?? 0) / total : null }));
  return {
    caption: `Average money in and money out per living household at age ${Math.floor(model.time.ages[i] ?? 0)} (${model.time.years[i]}), today's dollars`,
    columns: [
      { key: 'side', label: 'Side', align: 'left' },
      { key: 'item', label: 'Item', align: 'left' },
      { key: 'amount', label: 'Amount', format: (v) => moneyCompact(v) },
      { key: 'share', label: 'Share of the side', format: (v) => `${(v * 100).toFixed(1)}%` },
    ],
    rows: [...side(nodes.sources, 'Money in', totalIn), ...side(nodes.uses, 'Money out', totalOut)],
  };
}

export function YearControls({ model, year, onChange, jumps }: { model: CashFlowModel; year: number; onChange: (index: number) => void; jumps: YearJump[] }) {
  const { time, flowStart, end } = model;
  const fraction = end > flowStart ? (year - flowStart) / (end - flowStart) : 0;
  const text = `Age ${Math.floor(time.ages[year] ?? 0)}, ${time.years[year]}`;
  return (
    <div className="flex w-full flex-wrap items-center gap-x-3 gap-y-2">
      <div className="flex items-center">
        <IconButton icon="chevronLeft" size="sm" label="Previous year" disabled={year <= flowStart} onClick={() => onChange(Math.max(flowStart, year - 1))} />
        <output className="num min-w-[6.5rem] text-center text-sm font-medium text-ink-1" data-testid="flow-year">
          Age {Math.floor(time.ages[year] ?? 0)} <span className="font-normal text-ink-3">· {time.years[year]}</span>
        </output>
        <IconButton icon="chevronRight" size="sm" label="Next year" disabled={year >= end} onClick={() => onChange(Math.min(end, year + 1))} />
      </div>
      <input
        type="range"
        className="slider h-7 min-w-32 flex-1"
        min={flowStart}
        max={end}
        step={1}
        value={year}
        aria-label="Year shown in the flow"
        aria-valuetext={text}
        style={{ '--fill': `${fraction * 100}%` } as CSSProperties}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <div className="flex flex-wrap gap-1" role="group" aria-label="Jump to a moment in the plan">
        {jumps.map((j) => (
          <button
            key={j.label}
            type="button"
            aria-pressed={j.index === year}
            onClick={() => onChange(j.index)}
            className={cx('h-7 rounded-sm border px-2 text-xs whitespace-nowrap transition-colors', j.index === year ? 'border-line-strong bg-surface-2 font-medium text-ink-1' : 'border-transparent text-ink-2 hover:bg-surface-3 hover:text-ink-1')}
          >
            {j.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function YearFlow({ model, nodes, year, compact }: { model: CashFlowModel; nodes: { sources: YearNode[]; uses: YearNode[] }; year: number; compact?: boolean }) {
  const theme = useChartTheme();
  const { sources, uses } = useMemo(() => {
    const at = (list: YearNode[]): FlowNode[] => list.map((n) => ({ key: n.key, label: n.label, group: n.group, color: theme.categorical[n.color] as string, value: n.values[year] ?? 0 }));
    return { sources: at(nodes.sources), uses: at(nodes.uses) };
  }, [nodes, year, theme]);
  return (
    <FlowDiagram
      sources={sources}
      uses={uses}
      format={moneyCompact}
      trunkLabel={(total) => `${moneyCompact(total)} in, ${moneyCompact(total)} out`}
      headings={compact ? undefined : { sources: 'Comes from', uses: 'Goes to' }}
      compact={compact}
      label={`Average money in and money out at age ${Math.floor(model.time.ages[year] ?? 0)}, as a flow from sources to uses`}
    />
  );
}
