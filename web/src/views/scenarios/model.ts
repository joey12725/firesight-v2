// Data transforms for the Scenarios view: from the engine's Summary and Comparison to
// what the charts draw. No React here, so every function can be tested on real output.
import type { Comparison, Estimate, NullableEstimate, OutcomeComparison, PathScalar, ScalarComparison, Status, Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { STATUS_STACK, statusShares } from '@/ui/charts';
import type { ScenarioComparison } from '../types';

// ---- selectors ----------------------------------------------------------------------

export const outcomeOf = (c: Comparison, outcome: OutcomeComparison['outcome']) => c.outcomes.find((o) => o.outcome === outcome) ?? null;
export const scalarOf = (c: Comparison, scalar: PathScalar) => c.scalars.find((s) => s.scalar === scalar) ?? null;

// ---- success flips ------------------------------------------------------------------

export interface Flips {
  /** Lives compared (both runs). */
  paths: number;
  /** Fall short under the baseline, hold under the scenario. */
  rescued: number;
  /** Hold under the baseline, fall short under the scenario. */
  harmed: number;
  /** Hold either way. */
  both: number;
  /** Fall short either way. */
  neither: number;
  /** Net change in the share of lives in which the plan holds, with its paired 95% interval. */
  net: Estimate;
  withinNoise: boolean;
}

export function flipsOf(c: Comparison): Flips | null {
  const s = outcomeOf(c, 'success');
  if (!s) return null;
  return { paths: c.paths, rescued: s.b_only, harmed: s.a_only, both: s.both, neither: s.neither, net: s.difference, withinNoise: s.within_noise };
}

/**
 * The flips as counts out of one denominator that add up. The denominator is the
 * smallest of 100, 1,000 and the lives themselves at which the larger group shows; the
 * smaller group may then round to nothing, which `flipSentence` states in raw lives.
 */
export function flipCounts(f: Flips): { rescued: number; harmed: number; unchanged: number; of: number } {
  const n = Math.max(1, f.paths);
  const larger = Math.max(f.rescued, f.harmed) / n;
  const fits = (of: number) => larger === 0 || Math.round(larger * of) >= 1;
  const of = n < 100 ? n : fits(100) ? 100 : n >= 1000 && fits(1000) ? 1000 : n;
  const rescued = Math.round((f.rescued / n) * of);
  const harmed = Math.round((f.harmed / n) * of);
  return { rescued, harmed, unchanged: of - rescued - harmed, of };
}

// ---- metrics that scenarios are ranked on -------------------------------------------

export type MetricKey = 'success' | 'fi_age' | 'terminal' | 'spending';
export type MetricUnit = 'share' | 'age' | 'dollars';

export interface MetricDef {
  key: MetricKey;
  /** For the control. */
  label: string;
  /** What a difference is, for axis titles and sentences. */
  axis: string;
  unit: MetricUnit;
  /** Whether more of it is the better outcome. */
  higherIsBetter: boolean;
  /** The level in one run, for panels. */
  level: (summary: Summary) => number | null;
  /** Scenario minus baseline over the same lives, with its paired interval. */
  delta: (paired: Comparison) => { estimate: NullableEstimate; withinNoise: boolean; paired: number } | null;
}

const scalarDelta = (scalar: PathScalar) => (paired: Comparison) => {
  const s = scalarOf(paired, scalar);
  return s ? { estimate: s.mean_delta, withinNoise: s.within_noise, paired: s.both } : null;
};

export const METRICS: MetricDef[] = [
  {
    key: 'success',
    label: 'Plan holds',
    axis: 'Lives in which the plan holds, change per 100',
    unit: 'share',
    higherIsBetter: true,
    level: (s) => s.headline.success_rate.value,
    delta: (p) => {
      const o = outcomeOf(p, 'success');
      return o ? { estimate: o.difference, withinNoise: o.within_noise, paired: p.paths } : null;
    },
  },
  { key: 'fi_age', label: 'Work optional', axis: 'Age work becomes optional, average change in years', unit: 'age', higherIsBetter: false, level: (s) => s.headline.fi_age.p50, delta: scalarDelta('fi_age') },
  { key: 'terminal', label: 'Wealth at the end', axis: 'Net worth at the end of life, average change', unit: 'dollars', higherIsBetter: true, level: (s) => s.headline.terminal_net_worth.p50, delta: scalarDelta('terminal_net_worth') },
  { key: 'spending', label: 'Lifetime spending', axis: 'Lifetime spending, average change', unit: 'dollars', higherIsBetter: true, level: (s) => s.headline.lifetime_spending.p50, delta: scalarDelta('lifetime_spending') },
];

export const metricOf = (key: MetricKey) => METRICS.find((m) => m.key === key) ?? (METRICS[0] as MetricDef);

/** A difference with its sign, in the metric's unit: "+4 of 100", "1.8 years earlier", "+$210k". */
export function formatDelta(v: number | null, unit: MetricUnit): string {
  if (v === null || !Number.isFinite(v)) return 'n/a';
  const sign = v > 0 ? '+' : v < 0 ? '−' : '';
  switch (unit) {
    case 'share': {
      const points = Math.abs(v) * 100;
      const text = points > 0 && points < 0.95 ? points.toFixed(1) : String(Math.round(points));
      return points === 0 ? 'no change' : `${sign}${text} of 100`;
    }
    case 'age': {
      const years = Math.abs(v);
      if (years < 0.05) return 'no change';
      return years < 1 ? `${Math.round(years * 12)} ${Math.round(years * 12) === 1 ? 'month' : 'months'} ${v < 0 ? 'earlier' : 'later'}` : `${years.toFixed(1)} years ${v < 0 ? 'earlier' : 'later'}`;
    }
    case 'dollars':
      return Math.abs(v) < 500 ? 'no change' : `${sign}${moneyCompact(Math.abs(v))}`;
  }
}

/** Tick and interval labels: signed, compact, no words. */
export function formatDeltaTick(v: number, unit: MetricUnit): string {
  if (v === 0) return unit === 'dollars' ? '$0' : '0';
  const sign = v > 0 ? '+' : '−';
  switch (unit) {
    case 'share':
      return `${sign}${Math.round(Math.abs(v) * 1000) / 10}`;
    case 'age':
      return `${sign}${Math.round(Math.abs(v) * 10) / 10}`;
    case 'dollars':
      return `${sign}${moneyCompact(Math.abs(v))}`;
  }
}

export interface RankRow {
  id: string;
  name: string;
  /** Null while the scenario is still running, or when no life has a value in both runs. */
  value: number | null;
  lo: number | null;
  hi: number | null;
  withinNoise: boolean;
  /** Lives behind the estimate. */
  paired: number;
  pending: boolean;
}

/**
 * Every enabled scenario's difference from the baseline on one metric, best first.
 * Scenarios whose intervals include zero keep their place in the order but are flagged:
 * the order among them means nothing.
 */
export function rankRows(comparison: ScenarioComparison[], metric: MetricDef): RankRow[] {
  const rows = comparison.map((c): RankRow => {
    const d = c.paired ? metric.delta(c.paired) : null;
    if (!d || d.estimate.value === null) return { id: c.id, name: c.name, value: null, lo: null, hi: null, withinNoise: false, paired: d?.paired ?? 0, pending: c.paired === null };
    return { id: c.id, name: c.name, value: d.estimate.value, lo: d.estimate.lo, hi: d.estimate.hi, withinNoise: d.withinNoise, paired: d.paired, pending: false };
  });
  const better = metric.higherIsBetter ? -1 : 1;
  return rows.sort((a, b) => (a.value === null ? 1 : 0) - (b.value === null ? 1 : 0) || better * ((a.value ?? 0) - (b.value ?? 0)));
}

// ---- the distribution of per-life differences ---------------------------------------

export interface DeltaBin {
  /** Centre of the bin in data units. */
  mid: number;
  lo: number;
  hi: number;
  count: number;
  /** The bin also holds every dot beyond the axis on its side. */
  open: 'below' | 'above' | null;
}

export interface DeltaDots {
  /** Dots that changed, in bins either side of zero. */
  bins: DeltaBin[];
  /** Dots at exactly no change. */
  unchanged: number;
  total: number;
  /** Axis extent in data units; always includes zero. */
  domain: [number, number];
  step: number;
}

const NICE = [1, 2, 2.5, 5, 10];

function niceStep(raw: number): number {
  if (!(raw > 0)) return 1;
  const power = 10 ** Math.floor(Math.log10(raw));
  return (NICE.find((n) => n * power >= raw - 1e-12) ?? 10) * power;
}

/**
 * Bins the engine's 100 quantile dots of a per-life difference for a dot plot. Zero is
 * always a bin edge, so no bin mixes gains with losses; unchanged dots (the same life,
 * untouched by the decision) are counted apart. The axis spans the middle 84% of
 * the changed dots so a few extreme lives cannot flatten the rest; anything beyond is
 * piled into the end bins, which are marked open.
 */
export function binDeltas(dots: (number | null)[], columns = 24, tolerance = 0, equalShare: number | null = null): DeltaDots {
  const values = dots.filter((d): d is number => d !== null && Number.isFinite(d)).sort((a, b) => a - b);
  // When the engine says what share of all lives is exactly unchanged, that many dots
  // (the ones nearest zero) are the unchanged ones, so the plot and the sentence above it
  // count the same lives. Otherwise a dot within `tolerance` of zero is unchanged.
  const forced = equalShare === null ? null : Math.min(values.length, Math.round(equalShare * values.length));
  const nearest = forced === null ? null : new Set([...values.keys()].sort((i, j) => Math.abs(values[i] as number) - Math.abs(values[j] as number)).slice(0, forced));
  const moved = nearest ? values.filter((_, i) => !nearest.has(i)) : values.filter((v) => Math.abs(v) > tolerance);
  const unchanged = values.length - moved.length;
  if (moved.length === 0) return { bins: [], unchanged, total: values.length, domain: [-1, 1], step: 1 };
  const at = (q: number) => moved[Math.min(moved.length - 1, Math.max(0, Math.round(q * (moved.length - 1))))] as number;
  const lo = Math.min(0, at(0.08));
  const hi = Math.max(0, at(0.92));
  const step = niceStep((hi - lo) / Math.max(4, columns));
  const first = Math.min(-1, Math.floor(lo / step + 1e-9));
  const last = Math.max(1, Math.ceil(hi / step - 1e-9));
  const counts = new Map<number, number>();
  for (const v of moved) {
    // Bin k spans [k * step, (k + 1) * step); negative values fall in negative bins.
    const k = Math.min(last - 1, Math.max(first, v < 0 ? Math.ceil(v / step) - 1 : Math.floor(v / step)));
    counts.set(k, (counts.get(k) ?? 0) + 1);
  }
  const min = moved[0] as number;
  const max = moved[moved.length - 1] as number;
  const bins = [...counts.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([k, count]): DeltaBin => ({ mid: (k + 0.5) * step, lo: k * step, hi: (k + 1) * step, count, open: k === first && min < first * step ? 'below' : k === last - 1 && max > last * step ? 'above' : null }));
  return { bins, unchanged, total: values.length, domain: [first * step, last * step], step };
}

/** The smallest per-life difference worth calling a change, by unit: a dollar is not a decision. */
export function deltaTolerance(s: ScalarComparison): number {
  switch (s.unit) {
    case 'dollars':
      return 500;
    case 'age':
    case 'years':
      return 0.04;
    case 'months':
      return 0.4;
    default:
      return 1e-6;
  }
}

// ---- life stages --------------------------------------------------------------------

export interface StageDiff {
  status: Status;
  /** Scenario share minus baseline share of all lives, by year index. */
  values: number[];
  /** Largest absolute difference and where it is. */
  peak: number;
  peakIndex: number;
}

/** Scenario minus baseline share of lives in each life stage, by year; stages that never differ by `floor` are dropped. */
export function stageDiffs(base: Summary, scenario: Summary, floor = 0.005): StageDiff[] {
  const n = Math.min(base.axis.ages.length, scenario.axis.ages.length);
  return STATUS_STACK.flatMap((status) => {
    const a = statusShares(base, status);
    const b = statusShares(scenario, status);
    const values = Array.from({ length: n }, (_, y) => (b[y] ?? 0) - (a[y] ?? 0));
    let peakIndex = 0;
    values.forEach((v, y) => {
      if (Math.abs(v) > Math.abs(values[peakIndex] ?? 0)) peakIndex = y;
    });
    const peak = values[peakIndex] ?? 0;
    return Math.abs(peak) >= floor ? [{ status, values, peak, peakIndex }] : [];
  });
}

// ---- the same life under both decisions ---------------------------------------------

export type LifeWhy = 'rescued' | 'harmed' | 'gain' | 'loss' | 'typical';

export interface LifeChoice {
  why: LifeWhy;
  pathIndex: number;
  /** How many lives of this kind are among those the engine keeps per-life values for. */
  among: number;
  /** Net worth at the end under each plan, when the engine reported it. */
  endA: number | null;
  endB: number | null;
}

function column(summary: Summary, scalar: PathScalar): Map<number, number | null> {
  const values = summary.scatter.columns.find((c) => c.scalar === scalar)?.values ?? [];
  return new Map(summary.scatter.paths.map((p, i) => [p, values[i] ?? null]));
}

/**
 * Lives worth replaying under both plans, found by joining the two runs' per-life
 * samples on path index: one the change rescues, one it harms, the largest gain and
 * loss in wealth at the end, and the life whose change is the median one.
 */
export function chooseLives(base: Summary, scenario: Summary): LifeChoice[] {
  const okA = column(base, 'success');
  const okB = column(scenario, 'success');
  const endA = column(base, 'terminal_net_worth');
  const endB = column(scenario, 'terminal_net_worth');
  const joined = base.scatter.paths.filter((p) => okB.has(p));
  const rescued = joined.filter((p) => okA.get(p) === 0 && okB.get(p) === 1);
  const harmed = joined.filter((p) => okA.get(p) === 1 && okB.get(p) === 0);
  const deltas = joined
    .map((p) => ({ p, d: (endB.get(p) ?? NaN) - (endA.get(p) ?? NaN) }))
    .filter((x) => Number.isFinite(x.d))
    .sort((a, b) => a.d - b.d);
  const choice = (why: LifeWhy, p: number | undefined, among: number): LifeChoice[] => (p === undefined ? [] : [{ why, pathIndex: p, among, endA: endA.get(p) ?? null, endB: endB.get(p) ?? null }]);
  // Of the flipped lives, take the one whose end wealth changes most typically (the median of them), not the first found.
  const middle = (paths: number[]) => {
    const ranked = paths.map((p) => ({ p, d: (endB.get(p) ?? 0) - (endA.get(p) ?? 0) })).sort((a, b) => a.d - b.d);
    return ranked[Math.floor(ranked.length / 2)]?.p;
  };
  const out: LifeChoice[] = [...choice('rescued', middle(rescued), rescued.length), ...choice('harmed', middle(harmed), harmed.length)];
  const typical = deltas[Math.floor(deltas.length / 2)];
  const best = deltas[deltas.length - 1];
  const worst = deltas[0];
  out.push(...choice('typical', typical?.p, deltas.length));
  if (best && best.d > 500) out.push(...choice('gain', best.p, deltas.length));
  if (worst && worst.d < -500) out.push(...choice('loss', worst.p, deltas.length));
  // One entry per life.
  return out.filter((c, i) => out.findIndex((o) => o.pathIndex === c.pathIndex) === i);
}

/**
 * The first year index at which two series of the same life part ways by more than
 * `relative` of the larger and `absolute` dollars. Null when they never do.
 */
export function divergence(a: readonly (number | null)[], b: readonly (number | null)[], relative = 0.01, absolute = 1000): number | null {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    const x = a[i];
    const y = b[i];
    if (x === null || y === null || x === undefined || y === undefined) continue;
    if (Math.abs(x - y) > Math.max(absolute, relative * Math.max(Math.abs(x), Math.abs(y)))) return i;
  }
  // One life outlasting the other is a divergence too.
  return a.length !== b.length ? n - 1 : null;
}
