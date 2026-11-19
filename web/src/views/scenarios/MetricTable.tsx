// Every headline measure under both plans, with the paired difference and its 95%
// interval. The small interval drawing in each row is scaled to that row alone: what
// matters is where the interval sits against zero, not its length against other rows.
import type { Comparison, NullableEstimate, OutcomeComparison, PathScalar, ScalarComparison } from '@/engine/output-types';
import { Table, Td, Th, cx, moneyCompact } from '@/ui';
import { ofHundred, useChartTheme, type TableData } from '@/ui/charts';
import { deltaQuantile, homeSaleOf, outcomeOf, scalarOf } from './model';

type Unit = 'share' | 'age' | 'dollars' | 'years' | 'ratio';

export interface MetricRow {
  key: string;
  label: string;
  /** The measure as a noun inside a sentence. */
  short: string;
  /** What the average is over, when it is not every life. */
  among?: string;
  unit: Unit;
  a: number | null;
  b: number | null;
  delta: NullableEstimate;
  withinNoise: boolean;
  /** Whether a higher value is the better outcome; null when neither is. */
  higherIsBetter: boolean | null;
  /** Shares of paired lives better / worse / same, for per-life quantities. */
  lives?: { higher: number; lower: number; equal: number } | null;
  /** Median of the per-life difference. */
  median?: number | null;
}

const OUTCOMES: { outcome: OutcomeComparison['outcome']; label: string; short: string; higherIsBetter: boolean }[] = [
  { outcome: 'success', label: 'The plan holds to the end', short: 'whether the plan holds', higherIsBetter: true },
  { outcome: 'depleted', label: 'Money runs short', short: 'running short', higherIsBetter: false },
  { outcome: 'fi', label: 'Work becomes optional', short: 'whether work becomes optional', higherIsBetter: true },
];

const SCALARS: { scalar: PathScalar; label: string; short: string; unit: Unit; higherIsBetter: boolean | null; among?: string }[] = [
  { scalar: 'fi_age', label: 'Age work becomes optional', short: 'the age work becomes optional', unit: 'age', higherIsBetter: false, among: 'where it does under both' },
  { scalar: 'retirement_age', label: 'Retirement age', short: 'retirement age', unit: 'age', higherIsBetter: null, among: 'where both retire' },
  { scalar: 'investable_at_retirement', label: 'Savings at retirement', short: 'savings at retirement', unit: 'dollars', higherIsBetter: true, among: 'where both retire' },
  { scalar: 'lifetime_spending', label: 'Lifetime spending', short: 'lifetime spending', unit: 'dollars', higherIsBetter: true },
  { scalar: 'plan_delivered_share', label: 'Share of plan delivered in retirement', short: 'the share of plan delivered', unit: 'ratio', higherIsBetter: true, among: 'where both retire' },
  { scalar: 'extra_spending_ratio', label: 'Extra spending above plan', short: 'extra spending above plan', unit: 'ratio', higherIsBetter: true, among: 'where both retire' },
  { scalar: 'years_below_90', label: 'Years with spending cut over 10%', short: 'years of spending cuts', unit: 'years', higherIsBetter: false },
  { scalar: 'lifetime_tax', label: 'Lifetime tax', short: 'lifetime tax', unit: 'dollars', higherIsBetter: false },
  { scalar: 'terminal_net_worth', label: 'Net worth at the end of life', short: 'wealth at the end', unit: 'dollars', higherIsBetter: true },
];

export function metricRows(c: Comparison): MetricRow[] {
  const rows: MetricRow[] = [];
  for (const def of OUTCOMES) {
    const o = outcomeOf(c, def.outcome);
    if (o) rows.push({ key: def.outcome, label: def.label, short: def.short, unit: 'share', a: o.a.value, b: o.b.value, delta: o.difference, withinNoise: o.within_noise, higherIsBetter: def.higherIsBetter });
    // Holding counts a last-resort home sale, so the sale sits directly under it.
    const sale = def.outcome === 'success' ? homeSaleOf(c) : null;
    if (sale) rows.push({ key: 'home_sale', label: 'A home is sold as a last resort', short: 'selling a home as a last resort', unit: 'share', a: sale.a, b: sale.b, delta: sale.difference, withinNoise: sale.withinNoise, higherIsBetter: false });
  }
  for (const def of SCALARS) {
    const s: ScalarComparison | null = scalarOf(c, def.scalar);
    if (!s || s.both === 0) continue;
    rows.push({
      key: def.scalar,
      label: def.label,
      short: def.short,
      among: def.among,
      unit: def.unit,
      a: s.mean_a,
      b: s.mean_b,
      delta: s.mean_delta,
      withinNoise: s.within_noise,
      higherIsBetter: def.higherIsBetter,
      lives: s.b_higher === null || s.a_higher === null || s.equal === null ? null : { higher: s.b_higher, lower: s.a_higher, equal: s.equal },
      median: deltaQuantile(c.quantile_levels, s, 0.5),
    });
  }
  return rows;
}

export function formatLevel(v: number | null, unit: Unit): string {
  if (v === null) return 'n/a';
  switch (unit) {
    case 'share':
      return ofHundred(v).text;
    case 'age':
      return v.toFixed(1);
    case 'dollars':
      return moneyCompact(v);
    case 'years':
      return `${v.toFixed(1)} yr`;
    case 'ratio':
      return `${Math.round(v * 100)}%`;
  }
}

export function formatChange(v: number | null, unit: Unit): string {
  if (v === null) return 'n/a';
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  const a = Math.abs(v);
  switch (unit) {
    case 'share':
      return `${sign}${(a * 100).toFixed(1)}`;
    case 'age':
    case 'years':
      return `${sign}${a.toFixed(a < 0.995 ? 2 : 1)} yr`;
    case 'dollars':
      return `${sign}${moneyCompact(a)}`;
    case 'ratio':
      return `${sign}${(a * 100).toFixed(1)} pts`;
  }
}

export function verdict(row: MetricRow): { text: string; tone: 'good' | 'bad' | 'neutral' } {
  const v = row.delta.value;
  if (v === null) return { text: 'n/a', tone: 'neutral' };
  if (row.delta.lo === 0 && row.delta.hi === 0 && v === 0) return { text: 'Unchanged', tone: 'neutral' };
  if (row.withinNoise) return { text: 'Within noise', tone: 'neutral' };
  if (row.higherIsBetter === null) return { text: v > 0 ? 'Later' : 'Earlier', tone: 'neutral' };
  return (v > 0) === row.higherIsBetter ? { text: 'Better', tone: 'good' } : { text: 'Worse', tone: 'bad' };
}

export function metricTableData(c: Comparison, name: string): TableData {
  const rows = metricRows(c);
  return {
    caption: `Headline measures under the plan and under “${name}”, with the paired difference (scenario minus plan) and its 95% interval`,
    columns: [
      { key: 'measure', label: 'Measure', align: 'left' },
      { key: 'unit', label: 'Unit', align: 'left' },
      { key: 'plan', label: 'Plan' },
      { key: 'scenario', label: 'Scenario' },
      { key: 'delta', label: 'Difference' },
      { key: 'lo', label: '95% interval, low' },
      { key: 'hi', label: '95% interval, high' },
      { key: 'median', label: 'Median per-life difference' },
      { key: 'higher', label: 'Share of lives higher' },
      { key: 'lower', label: 'Share of lives lower' },
      { key: 'verdict', label: 'Verdict', align: 'left' },
    ],
    rows: rows.map((r) => ({
      measure: r.among ? `${r.label} (average, ${r.among})` : r.unit === 'share' ? `${r.label} (share of lives)` : `${r.label} (average)`,
      unit: r.unit,
      plan: r.a,
      scenario: r.b,
      delta: r.delta.value,
      lo: r.delta.lo,
      hi: r.delta.hi,
      median: r.median ?? null,
      higher: r.lives?.higher ?? null,
      lower: r.lives?.lower ?? null,
      verdict: verdict(r).text,
    })),
  };
}

/** The interval against zero: a line with a dot, hollow when it includes zero. Scaled to this row. */
function IntervalGlyph({ row }: { row: MetricRow }) {
  const theme = useChartTheme();
  const { value, lo, hi } = row.delta;
  const width = 84;
  const height = 16;
  if (value === null || lo === null || hi === null) return <svg width={width} height={height} aria-hidden />;
  const extent = Math.max(Math.abs(lo), Math.abs(hi), Math.abs(value)) * 1.15 || 1;
  const x = (v: number) => width / 2 + (v / extent) * (width / 2 - 5);
  const still = lo === 0 && hi === 0;
  const color = row.withinNoise || still ? theme.ink.tertiary : theme.scenario.line;
  return (
    <svg width={width} height={height} aria-hidden className="block">
      <line x1={width / 2 + 0.5} x2={width / 2 + 0.5} y1={0} y2={height} stroke={theme.annotation} strokeWidth={1} />
      {!still && <line x1={x(lo)} x2={x(hi)} y1={height / 2} y2={height / 2} stroke={color} strokeWidth={2} strokeLinecap="round" />}
      {still ? null : row.withinNoise ? <circle cx={x(value)} cy={height / 2} r={3.5} fill={theme.surface} stroke={color} strokeWidth={1.5} /> : <circle cx={x(value)} cy={height / 2} r={4} fill={color} stroke={theme.surface} strokeWidth={1.5} />}
    </svg>
  );
}

const unchanged = (r: MetricRow) => r.delta.value === 0 && r.delta.lo === 0 && r.delta.hi === 0;

export function MetricTable({ comparison }: { comparison: Comparison }) {
  const rows = metricRows(comparison);
  return (
    <div className="scroll-thin -mx-1 overflow-x-auto px-1" tabIndex={0} role="region" aria-label="Every measure under the plan and with the change">
      {/* Narrow: four columns, with the verdict under the difference and no drawing. */}
      <Table>
        <colgroup>
          <col />
          <col className="w-[17%] @xl:w-[11%]" />
          <col className="w-[19%] @xl:w-[12%]" />
          <col className="w-[33%] @xl:w-[20%]" />
          <col className="hidden @xl:table-column" style={{ width: 100 }} />
          <col className="hidden w-[12%] @xl:table-column" />
        </colgroup>
        <thead>
          <tr>
            <Th>Measure</Th>
            <Th numeric>Plan</Th>
            <Th numeric>Changed</Th>
            <Th numeric>Difference</Th>
            <Th className="hidden @xl:table-cell">
              <span className="sr-only">Interval against no change</span>
            </Th>
            <Th className="hidden @xl:table-cell">Verdict</Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => {
            const v = verdict(r);
            const firstScalar = r.unit !== 'share' && rows[i - 1]?.unit === 'share';
            const still = unchanged(r);
            return (
              <tr key={r.key} className={cx(firstScalar && '[&>td]:border-t [&>td]:border-t-line-strong')}>
                <Td className="py-2 !align-top">
                  <span className="text-ink-1">{r.label}</span>
                  <span className="block text-xs text-ink-3">{r.unit === 'share' ? 'lives of 100' : `average${r.among ? `, ${r.among}` : ''}`}</span>
                </Td>
                <Td numeric className="py-2 !align-top whitespace-nowrap text-ink-2">
                  {formatLevel(r.a, r.unit).replace(' of 100', '')}
                </Td>
                <Td numeric className="py-2 !align-top whitespace-nowrap text-ink-1">
                  {formatLevel(r.b, r.unit).replace(' of 100', '')}
                </Td>
                <Td numeric className="py-2 !align-top @xl:whitespace-nowrap">
                  <span className={cx('whitespace-nowrap', still ? 'text-ink-3' : 'font-medium text-ink-1')}>{still ? 'none' : formatChange(r.delta.value, r.unit)}</span>
                  {!still && r.delta.lo !== null && r.delta.hi !== null && (
                    <span className="block text-xs text-ink-3">
                      <span className="whitespace-nowrap">{formatChange(r.delta.lo, r.unit)}</span> <span className="whitespace-nowrap">to {formatChange(r.delta.hi, r.unit)}</span>
                    </span>
                  )}
                  {!still && <span className={cx('block font-sans text-xs @xl:hidden', v.tone === 'good' ? 'font-medium text-good' : v.tone === 'bad' ? 'font-medium text-bad' : 'text-ink-3')}>{v.text}</span>}
                </Td>
                <Td className="hidden py-2 !align-top @xl:table-cell">
                  <div className="pt-0.5 pl-1">
                    <IntervalGlyph row={r} />
                  </div>
                </Td>
                <Td className={cx('hidden py-2 !align-top text-xs leading-5 whitespace-nowrap @xl:table-cell', v.tone === 'good' ? 'font-medium text-good' : v.tone === 'bad' ? 'font-medium text-bad' : 'text-ink-3')}>{v.text}</Td>
              </tr>
            );
          })}
        </tbody>
      </Table>
    </div>
  );
}

/** "Better on whether the plan holds and lifetime tax; worse on lifetime spending. 4 of 11 measures do not move by more than sampling noise." */
export function metricTakeaway(c: Comparison): string {
  const rows = metricRows(c).filter((r) => r.delta.value !== null);
  const real = rows.filter((r) => !r.withinNoise && !unchanged(r));
  const better = real.filter((r) => r.higherIsBetter !== null && (r.delta.value as number) > 0 === r.higherIsBetter);
  const worse = real.filter((r) => r.higherIsBetter !== null && (r.delta.value as number) > 0 !== r.higherIsBetter);
  if (real.length === 0) return `None of these ${rows.length} measures moves by more than sampling noise.`;
  const list = (rs: MetricRow[]) => {
    const names = rs.slice(0, 4).map((r) => r.short);
    if (rs.length > 4) names.push(`${rs.length - 4} more`);
    return names.length <= 1 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
  };
  const parts = [better.length ? `better on ${list(better)}` : '', worse.length ? `worse on ${list(worse)}` : ''].filter(Boolean);
  const noise = rows.length - real.length;
  const lead = parts.length ? `With the change, things are ${parts.join('; ')}.` : 'The change moves the timing of retirement without being better or worse on these measures.';
  return `${lead}${noise > 0 ? ` ${noise} of ${rows.length} measures do not move by more than sampling noise.` : ''}`;
}
