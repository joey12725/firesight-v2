// Many scenarios at once: one small panel each, the same three figures with their
// change from the plan, and the scenario's median net worth drawn against the plan's
// on axes shared by every panel, so the panels can be compared with each other.
import { useMemo } from 'react';
import type { Summary } from '@/engine/output-types';
import { Tag, cx, moneyCompact } from '@/ui';
import { ChartSyncProvider, Legend, age as fmtAge, ofHundred, quantileSeries, useChartTheme } from '@/ui/charts';
import type { ScenarioComparison } from '../types';
import { PairedFan } from './PairedFan';
import { formatDelta, rankRows, type MetricDef, type MetricKey } from './model';

interface Figure {
  key: MetricKey;
  label: string;
  value: string;
  delta: string;
  /** Good, bad or neither, for the small arrow's word; colour is not used. */
  moved: boolean;
}

function figures(base: Summary, s: Summary): Figure[] {
  const success = [base.headline.success_rate.value, s.headline.success_rate.value] as const;
  const fi = [base.headline.fi_age.p50, s.headline.fi_age.p50] as const;
  const end = [base.headline.terminal_net_worth.p50, s.headline.terminal_net_worth.p50] as const;
  const diff = (a: number | null, b: number | null) => (a === null || b === null ? null : b - a);
  const dSuccess = diff(success[0], success[1]);
  const dFi = diff(fi[0], fi[1]);
  const dEnd = diff(end[0], end[1]);
  return [
    { key: 'success', label: 'Plan holds', value: ofHundred(success[1]).text, delta: formatDelta(dSuccess, 'share'), moved: dSuccess !== null && Math.abs(dSuccess) >= 0.0005 },
    { key: 'fi_age', label: 'Work optional', value: fi[1] === null ? 'no typical age' : `at ${fmtAge(fi[1])}`, delta: dFi === null ? (fi[0] !== null ? `plan: ${fmtAge(fi[0])}` : '') : formatDelta(dFi, 'age'), moved: dFi !== null && Math.abs(dFi) >= 0.05 },
    { key: 'terminal', label: 'Wealth at the end', value: end[1] === null ? 'n/a' : moneyCompact(end[1]), delta: formatDelta(dEnd, 'dollars'), moved: dEnd !== null && Math.abs(dEnd) >= 500 },
  ];
}

/** Which line is which in every panel's small chart. */
export function BoardKey({ className }: { className?: string }) {
  const theme = useChartTheme();
  return (
    <Legend
      className={className}
      items={[
        { key: 'b', label: 'The scenario, median net worth', color: theme.scenario.line, swatch: 'line' },
        { key: 'a', label: 'The plan', color: theme.baseline.median, swatch: 'line' },
      ]}
    />
  );
}

export interface BoardProps {
  base: Summary;
  comparison: ScenarioComparison[];
  metric: MetricDef;
  selected: string | null;
  onSelect: (id: string) => void;
}

export function Board({ base, comparison, metric, selected, onSelect }: BoardProps) {
  const order = useMemo(() => rankRows(comparison, metric).map((r) => r.id), [comparison, metric]);
  const sorted = order.map((id) => comparison.find((c) => c.id === id)).filter((c): c is ScenarioComparison => c !== undefined);
  // One y axis for every panel: the highest median any plan reaches.
  const top = useMemo(() => {
    const medians = [base, ...comparison.map((c) => c.summary)].flatMap((s) => (s ? quantileSeries(s, 'net_worth', 0.5) : [])).filter((v): v is number => v !== null);
    return Math.max(1, ...medians);
  }, [base, comparison]);

  return (
    <ol className="grid grid-cols-1 gap-x-8 gap-y-6 @xl:grid-cols-2 @5xl:grid-cols-3">
      {sorted.map((c, i) => {
        const active = c.id === selected;
        const figs = c.summary ? figures(base, c.summary) : null;
        return (
          <li key={c.id} className={cx('min-w-0 border-t pt-3', active ? 'border-ink-1' : 'border-line-strong')}>
            <div className="flex items-baseline gap-2">
              <span className="num text-xs text-ink-3">{i + 1}</span>
              <button type="button" aria-pressed={active} onClick={() => onSelect(c.id)} className={cx('min-w-0 truncate rounded-xs text-left text-sm hover:underline', active ? 'font-semibold text-ink-1' : 'font-medium text-ink-1')}>
                {c.name}
              </button>
              {active && <Tag className="ml-auto">Compared below</Tag>}
            </div>
            {!c.summary || !figs ? (
              <p className="mt-2 text-xs text-ink-3">Simulating…</p>
            ) : (
              <>
                <dl className="mt-2 space-y-1">
                  {figs.map((f) => (
                    <div key={f.key} className="flex items-baseline gap-2 text-xs">
                      <dt className={cx('w-[6.5rem] shrink-0', f.key === metric.key ? 'text-ink-1' : 'text-ink-3')}>{f.label}</dt>
                      <dd className="min-w-0 truncate font-medium whitespace-nowrap tabular-nums text-ink-1">{f.value}</dd>
                      <dd className={cx('ml-auto shrink-0 text-right whitespace-nowrap tabular-nums', f.moved ? 'text-ink-2' : 'text-ink-3')}>{f.delta}</dd>
                    </div>
                  ))}
                </dl>
                <div className="mt-2" aria-hidden>
                  <ChartSyncProvider>
                    <PairedFan base={base} scenario={c.summary} series="net_worth" name={c.name} bare height={58} top={top} />
                  </ChartSyncProvider>
                </div>
              </>
            )}
          </li>
        );
      })}
    </ol>
  );
}
