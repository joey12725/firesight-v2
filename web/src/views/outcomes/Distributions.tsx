// The per-life outcomes as quantile dot plots: each dot is one of 100 equally likely
// lives. Lives with no place on an axis (never retired, never cut, never short) are a
// group of their own beside it, so the picture always adds up to every life.
import { useMemo } from 'react';
import type { ScalarDistribution, Spread, Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { age as fmtAge, createTimeAxis, type TableData } from '@/ui/charts';
import { QuantileDots, type DotGroup } from '@/ui/charts/ext/outcomes-QuantileDots';
import { curveDots, distribution, livesDots, sameDots } from './data';

const NEVER: DotGroup = { key: 'never', label: 'Never', side: 'right', hollow: true };

function ageInterval(spread: Spread, what: string): string | undefined {
  const { p10, p50, p90 } = spread;
  if (p10 === null) return undefined;
  if (p50 === null) return `${what}: 1 in 10 by ${fmtAge(p10)}; fewer than half ever`;
  if (p90 === null) return `${what}: half by ${fmtAge(p50)}; 1 in 10 by ${fmtAge(p10)}`;
  return `Half by ${fmtAge(p50)} · middle 80 of 100 lives: ${fmtAge(p10)} to ${fmtAge(p90)}`;
}

/** True when the plan retires at the moment work becomes optional, so the two ages are one distribution. */
export function agesCoincide(summary: Summary): boolean {
  return sameDots(summary.fi.dots, summary.retirement.dots);
}

export function AgesChart({ summary }: { summary: Summary }) {
  const time = useMemo(() => createTimeAxis({ ages: summary.axis.ages, years: summary.axis.years }), [summary.axis.ages, summary.axis.years]);
  const fi = useMemo(() => curveDots(summary.fi, summary.paths), [summary.fi, summary.paths]);
  const retirement = useMemo(() => curveDots(summary.retirement, summary.paths), [summary.retirement, summary.paths]);
  const same = agesCoincide(summary);
  const real = [...fi, ...retirement].filter((d): d is number => d !== null);
  const domain: [number, number] | undefined = real.length ? [Math.floor(Math.min(...real)), Math.floor(Math.max(...real)) + 1] : undefined;
  const range = (lo: number, hi: number) => (hi - lo <= 1.001 ? `Age ${Math.round(lo)}` : `Ages ${Math.round(lo)} to ${Math.round(hi) - 1}`);
  const neverMost = Math.max(fi.filter((d) => d === null).length, retirement.filter((d) => d === null).length);
  const common = { steps: [1, 2, 5, 10], domain, groupWidth: same ? undefined : Math.max(30, Math.ceil(neverMost / 8) * 9), format: fmtAge, formatRange: range, axis: { kind: 'age' as const, time }, groups: [NEVER] };
  if (same) {
    return <QuantileDots {...common} dots={retirement} noun="Age at retirement" interval={summary.headline.retirement_age} intervalText={ageInterval(summary.headline.retirement_age, 'Retired')} height={236} />;
  }
  return (
    <div className="space-y-1">
      <QuantileDots {...common} axis={{ kind: 'age', time, years: false }} dots={fi} noun="Age at which work becomes optional" rowLabel="Work becomes optional" interval={summary.headline.fi_age} height={156} />
      <QuantileDots {...common} dots={retirement} noun="Age at retirement" rowLabel="Actually retires" interval={summary.headline.retirement_age} intervalText={ageInterval(summary.headline.retirement_age, 'Retired')} height={202} />
    </div>
  );
}

export function agesTable(summary: Summary): TableData {
  const fi = summary.fi;
  const ret = summary.retirement;
  const rows = fi.ages
    .map((a, i) => ({ age: a, fi: fi.counts[i] ?? 0, fi_by: fi.cumulative[i] ?? 0, retired: ret.counts[ret.ages.indexOf(a)] ?? 0, retired_by: ret.cumulative[ret.ages.indexOf(a)] ?? 0 }))
    .filter((r, i, all) => r.fi > 0 || r.retired > 0 || (i > 0 && ((all[i - 1]?.fi ?? 0) > 0 || (all[i - 1]?.retired ?? 0) > 0)));
  const share = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: 'Simulated lives by the age work becomes optional and the age of retirement',
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'fi', label: 'Work becomes optional at this age (lives)' },
      { key: 'fi_by', label: 'Optional by the end of this age', format: share },
      { key: 'retired', label: 'Retires at this age (lives)' },
      { key: 'retired_by', label: 'Retired by the end of this age', format: share },
    ],
    rows: [...rows, { age: 'Never', fi: null, fi_by: fi.never, retired: null, retired_by: ret.never }],
  };
}

function moneyRange(lo: number, hi: number) {
  return `${moneyCompact(lo)} to ${moneyCompact(hi)}`;
}

function quantileInterval(dist: ScalarDistribution | undefined) {
  const q = dist?.quantiles ?? [];
  return { p10: q[2] ?? null, p50: q[4] ?? null, p90: q[6] ?? null };
}

/** Net worth at the end of each life, on a log axis: lives span two or three powers of ten. */
export function WealthChart({ summary }: { summary: Summary }) {
  const dist = distribution(summary, 'terminal_net_worth');
  const dots = useMemo(() => livesDots(dist, summary.paths), [dist, summary.paths]);
  const { p10, p50, p90 } = summary.headline.terminal_net_worth;
  return (
    <QuantileDots
      dots={dots}
      scale="log"
      groupOf={(v) => (v === null || v <= 0 ? 'none' : null)}
      groups={[{ key: 'none', label: 'Nothing', side: 'left', note: 'These lives end with no net worth, or owing money.' }]}
      format={moneyCompact}
      formatRange={moneyRange}
      axis={{ kind: 'value', format: 'money', title: 'Net worth at the end, today’s dollars · log scale' }}
      noun="Net worth at the end of life"
      interval={{ p10: p10 !== null && p10 > 0 ? p10 : null, p50, p90 }}
      intervalText={p10 !== null && p50 !== null && p90 !== null ? `Median ${moneyCompact(p50)} · middle 80 of 100 lives: ${moneyCompact(p10)} to ${moneyCompact(p90)}` : undefined}
      height={190}
    />
  );
}

/** The deepest single-year cut to retirement spending: most lives have none, and say so. */
export function WorstCutChart({ summary }: { summary: Summary }) {
  const dist = distribution(summary, 'worst_spending_ratio');
  const dots = useMemo(() => livesDots(dist, summary.paths).map((d) => (d === null ? null : Math.max(0, 1 - d))), [dist, summary.paths]);
  const percent = (v: number) => `${Math.round(v * 100)}%`;
  return (
    <QuantileDots
      dots={dots}
      groupOf={(v) => (v === null ? 'never' : v <= 0.05 ? 'none' : null)}
      groups={[
        { key: 'none', label: 'On plan', side: 'left', note: 'Spending stays within 5% of the plan in every retired year.' },
        { key: 'never', label: 'Never retired', side: 'right', hollow: true, note: 'No retirement spending to cut.' },
      ]}
      steps={[0.05, 0.1, 0.2]}
      domain={[0.05, 1]}
      format={percent}
      formatRange={(lo, hi) => `A cut of ${percent(lo)} to ${percent(hi)}`}
      axis={{ kind: 'value', format: 'percent', title: 'Deepest cut below planned spending in any one year', ticks: [0.2, 0.4, 0.6, 0.8] }}
      noun="Worst single-year cut to retirement spending"
      height={196}
    />
  );
}

export function TaxChart({ summary }: { summary: Summary }) {
  const dist = distribution(summary, 'lifetime_tax');
  const dots = useMemo(() => livesDots(dist, summary.paths), [dist, summary.paths]);
  return (
    <QuantileDots
      dots={dots}
      scale="log"
      groupOf={(v) => (v === null || v <= 0 ? 'none' : null)}
      groups={[{ key: 'none', label: 'None', side: 'left' }]}
      format={moneyCompact}
      formatRange={moneyRange}
      axis={{ kind: 'value', format: 'money', title: 'Tax paid over the whole life, today’s dollars' }}
      noun="Lifetime tax"
      interval={quantileInterval(dist)}
      height={196}
    />
  );
}

export function ShortYearsChart({ summary }: { summary: Summary }) {
  const dist = distribution(summary, 'years_depleted');
  const dots = useMemo(() => livesDots(dist, summary.paths), [dist, summary.paths]);
  const yrs = (v: number) => `${Math.round(v)} ${Math.round(v) === 1 ? 'year' : 'years'}`;
  return (
    <QuantileDots
      dots={dots}
      groupOf={(v) => (v === null || v < 0.5 ? 'none' : null)}
      groups={[{ key: 'none', label: 'Never short', side: 'left', note: 'Liquid assets always cover essential spending.' }]}
      steps={[1, 2, 5, 10]}
      format={yrs}
      formatRange={(lo, hi) => (hi - lo <= 1.001 ? yrs(lo) : `${Math.round(lo)} to ${Math.round(hi) - 1} years`)}
      axis={{ kind: 'value', format: 'number', title: 'Years spent short of essentials' }}
      noun="Years spent short of essentials"
      height={196}
    />
  );
}

/** One table for the three smaller dot plots: the engine's quantiles of each quantity. */
export function scalarTable(summary: Summary, scalar: 'terminal_net_worth' | 'lifetime_tax' | 'years_depleted' | 'worst_spending_ratio', caption: string, format: (v: number) => string): TableData {
  const dist = distribution(summary, scalar);
  const levels = ['1st', '5th', '10th', '25th', 'Median', '75th', '90th', '95th', '99th'];
  return {
    caption,
    columns: [
      { key: 'level', label: 'Percentile of lives', align: 'left' },
      { key: 'value', label: 'Value', format },
    ],
    rows: [
      ...levels.map((level, i) => ({ level, value: dist?.quantiles[i] ?? null })),
      { level: 'Lives with a value', value: (dist?.count ?? 0).toLocaleString('en-US') },
      { level: 'Lives without one', value: (dist?.missing ?? 0).toLocaleString('en-US') },
    ],
  };
}
