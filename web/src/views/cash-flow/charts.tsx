// The charts of the Cash flow view that stack over the age axis, and the tables behind
// them. Each is the kit's stacked-area extension fed from the model.
import { useMemo } from 'react';
import type { Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { quantileSeries, useChartTheme, type TableData } from '@/ui/charts';
import { StackLegend, StackedAreas, type StackMoment, type StackSeries } from '@/ui/charts/ext/cashflow-StackedAreas';
import type { CashFlowModel, Group } from './model';

const fmt = (v: number) => moneyCompact(v);
const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

function useStack(groups: Group[], side: 'up' | 'down'): StackSeries[] {
  const theme = useChartTheme();
  return useMemo(() => groups.map((g) => ({ key: g.key, label: g.label, short: g.short, color: theme.categorical[g.color] as string, values: g.values, side })), [groups, side, theme]);
}

const resetKey = (model: CashFlowModel) => `${model.time.years[0]}:${model.time.length}`;

/** Rows by year for a set of groups, optionally expanded to their parts. */
function rows(model: CashFlowModel, from: number, columns: { key: string; values: readonly (number | null)[] }[]) {
  const out: Record<string, number | string | null>[] = [];
  for (let i = from; i <= model.end; i++) {
    out.push({ age: Math.floor(model.time.ages[i] as number), year: model.time.years[i] ?? null, ...Object.fromEntries(columns.map((c) => [c.key, c.values[i] ?? null])) });
  }
  return out;
}

const AGE_COLUMNS = [
  { key: 'age', label: 'Age' },
  { key: 'year', label: 'Year', format: String },
];

// ---- money in against money out --------------------------------------------------------

export function flowsTable(model: CashFlowModel): TableData {
  const parts = [...model.moneyIn.flatMap((g) => g.parts.map((p) => ({ ...p, key: `in_${p.key}`, label: `In: ${p.label}` }))), ...model.moneyOut.flatMap((g) => g.parts.map((p) => ({ ...p, key: `out_${p.key}`, label: `Out: ${p.label}` })))];
  const columns = [{ key: 'total_in', label: 'Money in', values: model.totalIn }, ...parts, { key: 'total_out', label: 'Money out', values: model.totalOut }];
  return {
    caption: "Average money in and money out per living household by age, today's dollars a year",
    columns: [...AGE_COLUMNS, ...columns.map((c) => ({ key: c.key, label: c.label, format: fmt }))],
    rows: rows(model, model.flowStart, columns),
  };
}

export function FlowsLegend({ model }: { model: CashFlowModel }) {
  const up = useStack(model.moneyIn, 'up');
  const down = useStack(model.moneyOut, 'down');
  return (
    <div className="mb-2 grid gap-x-8 gap-y-1 @3xl:grid-cols-[auto_1fr]">
      <div className="flex gap-2">
        <span className="w-8 shrink-0 text-xs text-ink-3">In</span>
        <StackLegend series={up} />
      </div>
      <div className="flex gap-2">
        <span className="w-8 shrink-0 text-xs text-ink-3">Out</span>
        <StackLegend series={down} />
      </div>
    </div>
  );
}

export function MoneyFlows({ model, moments, selected, onSelect, compact }: { model: CashFlowModel; moments?: StackMoment[]; selected?: number | null; onSelect?: (index: number) => void; compact?: boolean }) {
  const up = useStack(model.moneyIn, 'up');
  const down = useStack(model.moneyOut, 'down');
  const series = useMemo(() => [...up, ...down], [up, down]);
  return (
    <StackedAreas
      time={model.time}
      series={series}
      from={model.flowStart}
      to={model.end}
      title="Average per household a year, today's dollars"
      sides={{ up: 'Money in', down: 'Money out' }}
      totals={{ up: 'Money in', down: 'Money out' }}
      moments={moments}
      selected={selected}
      onSelect={onSelect}
      tooltipNote={onSelect ? () => 'Click to see this year as a flow.' : undefined}
      compact={compact}
      resetKey={resetKey(model)}
      label="Average money in and money out by age"
    />
  );
}

// ---- the balance sheet -----------------------------------------------------------------

export function balanceTable(model: CashFlowModel, summary: Summary): TableData {
  const groups = [...model.assets, ...model.liabilities];
  const columns = [...groups.map((g) => ({ key: g.key, label: g.label, values: g.values })), { key: 'net_worth', label: 'Average net worth', values: model.netWorth }, { key: 'median', label: 'Median net worth', values: quantileSeries(summary, 'net_worth', 0.5) }];
  return {
    caption: "Average year-end balance sheet per living household by age, today's dollars",
    columns: [...AGE_COLUMNS, ...columns.map((c) => ({ key: c.key, label: c.label, format: fmt }))],
    rows: rows(model, 0, columns),
  };
}

function useBalance(model: CashFlowModel, summary: Summary) {
  const theme = useChartTheme();
  const up = useStack(model.assets, 'up');
  const down = useStack(model.liabilities, 'down');
  const series = useMemo(() => [...up, ...down], [up, down]);
  const lines = useMemo(() => [{ key: 'median', label: 'Median', color: theme.ink.primary, values: quantileSeries(summary, 'net_worth', 0.5), strong: true }], [summary, theme]);
  return { series, lines };
}

export function BalanceLegend({ model, summary }: { model: CashFlowModel; summary: Summary }) {
  const { series, lines } = useBalance(model, summary);
  return <StackLegend className="mb-2" series={series} lines={lines.map((l) => ({ ...l, label: 'Net worth of the middle life' }))} />;
}

export function BalanceSheet({ model, summary, moments }: { model: CashFlowModel; summary: Summary; moments?: StackMoment[] }) {
  const { series, lines } = useBalance(model, summary);
  return (
    <StackedAreas
      time={model.time}
      series={series}
      lines={lines}
      to={model.end}
      title="Average per household at year end, today's dollars"
      sides={{ up: 'Owned', down: 'Owed' }}
      totals={{ up: 'Owned', down: 'Owed' }}
      moments={moments}
      resetKey={resetKey(model)}
      label="Average balance sheet by age: accounts and property above the line, debts below"
    />
  );
}

// ---- paid into and drawn from accounts -----------------------------------------------

export function accountsTable(model: CashFlowModel): TableData {
  const columns = [...model.savingBy.map((g) => ({ key: g.key, label: `Paid into: ${g.label}`, values: g.values })), ...model.withdrawBy.map((g) => ({ key: g.key, label: `Drawn from: ${g.label}`, values: g.values }))];
  return {
    caption: "Average saving by destination and withdrawals by account per living household, today's dollars a year",
    columns: [...AGE_COLUMNS, ...columns.map((c) => ({ key: c.key, label: c.label, format: fmt }))],
    rows: rows(model, model.flowStart, columns),
  };
}

function useAccounts(model: CashFlowModel) {
  const up = useStack(model.savingBy, 'up');
  const down = useStack(model.withdrawBy, 'down');
  return useMemo(() => [...up, ...down], [up, down]);
}

export function AccountsLegend({ model }: { model: CashFlowModel }) {
  const series = useAccounts(model);
  // One swatch per account: the same account wears the same colour above and below the line.
  const seen = new Set<string>();
  const unique = series.filter((s) => (seen.has(s.label) ? false : (seen.add(s.label), true))).map((s) => ({ ...s, side: 'up' as const }));
  return <StackLegend className="mb-2" series={unique.reverse()} />;
}

export function AccountFlows({ model }: { model: CashFlowModel }) {
  const series = useAccounts(model);
  return (
    <StackedAreas
      time={model.time}
      series={series}
      from={model.flowStart}
      to={model.end}
      title="Average per household a year, today's dollars"
      sides={{ up: 'Paid in', down: 'Drawn out' }}
      totals={{ up: 'Paid in', down: 'Drawn out' }}
      height={(w) => Math.round(Math.min(340, Math.max(260, w * 0.5)))}
      resetKey={resetKey(model)}
      label="Average saving by destination and withdrawals by account, by age"
    />
  );
}

// ---- tax --------------------------------------------------------------------------------

export function taxTable(model: CashFlowModel, summary: Summary): TableData {
  const columns = model.taxes.map((g) => ({ key: g.key, label: g.label, values: g.values }));
  return {
    caption: "Average tax paid per living household by age, today's dollars a year, and tax as a share of all money coming in",
    columns: [...AGE_COLUMNS, ...columns.map((c) => ({ key: c.key, label: c.label, format: fmt })), { key: 'median', label: 'Median total tax', format: fmt }, { key: 'share', label: 'Share of money in', format: pct }],
    rows: rows(model, model.flowStart, [...columns, { key: 'median', values: quantileSeries(summary, 'total_tax', 0.5) }, { key: 'share', values: model.taxShare }]),
  };
}

/** The middle life's total tax each year: the reference that shows how far the lucky lives lift the average. */
function useMedianTax(summary: Summary) {
  const theme = useChartTheme();
  return useMemo(() => [{ key: 'median', label: 'Median', color: theme.ink.primary, values: quantileSeries(summary, 'total_tax', 0.5), strong: true }], [summary, theme]);
}

export function TaxLegend({ model, summary }: { model: CashFlowModel; summary: Summary }) {
  const series = useStack(model.taxes, 'up');
  const lines = useMedianTax(summary);
  return <StackLegend className="mb-2" series={series} lines={lines.map((l) => ({ ...l, label: 'Tax paid by the middle life' }))} />;
}

export function TaxChart({ model, summary }: { model: CashFlowModel; summary: Summary }) {
  const series = useStack(model.taxes, 'up');
  const lines = useMedianTax(summary);
  return (
    <StackedAreas
      time={model.time}
      series={series}
      lines={lines}
      from={model.flowStart}
      to={model.end}
      title="Average tax paid a year, today's dollars"
      totals={{ up: 'Tax paid' }}
      strip={{ label: 'Tax as a share of money in', values: model.taxShare, format: (v) => `${Math.round(v * 100)}%` }}
      height={(w) => Math.round(Math.min(300, Math.max(220, w * 0.42))) + 66}
      resetKey={resetKey(model)}
      label="Average tax paid by kind, by age"
    />
  );
}
