// Compact forms of engine output for tool results. A Summary is hundreds of kilobytes;
// the model gets the few dozen numbers that answer questions, each probability with
// its interval, and the ids to ask for more.
import type { Comparison, Estimate, PathScalar, Spread, Summary } from '@/engine/output-types';
import type { RunRecord } from './runs';

const dollars = (v: number | null | undefined) => (v === null || v === undefined || !Number.isFinite(v) ? null : Math.round(v / 1000) * 1000);
const round = (v: number | null | undefined, dp: number) => {
  if (v === null || v === undefined || !Number.isFinite(v)) return null;
  const f = 10 ** dp;
  return Math.round(v * f) / f;
};
const rate = (e: Estimate | undefined) => (e ? { p: round(e.value, 3), lo: round(e.lo, 3), hi: round(e.hi, 3) } : null);
const ages = (s: Spread | undefined) => (s ? { p10: round(s.p10, 1), p50: round(s.p50, 1), p90: round(s.p90, 1) } : null);
const money = (s: Spread | undefined) => (s ? { p10: dollars(s.p10), p50: dollars(s.p50), p90: dollars(s.p90) } : null);

/** At most `n` evenly spaced indices of 0..length-1, always including both ends. */
function pick(length: number, n: number): number[] {
  if (length <= n) return Array.from({ length }, (_, i) => i);
  return Array.from({ length: n }, (_, i) => Math.round((i * (length - 1)) / (n - 1)));
}

function curve(c: Summary['fi'] | undefined) {
  if (!c || c.ages.length === 0) return { ages: [], p: [], never: round(c?.never, 3) };
  // Keep the part of the curve where something happens.
  const first = Math.max(0, c.cumulative.findIndex((v) => v > 0.005) - 1);
  const top = c.cumulative[c.cumulative.length - 1] ?? 0;
  const lastHit = c.cumulative.findIndex((v) => v >= top - 0.005);
  const last = lastHit < 0 ? c.ages.length - 1 : lastHit;
  const idx = pick(Math.max(1, last - first + 1), 6).map((i) => i + first);
  return { ages: idx.map((i) => c.ages[i] ?? null), p: idx.map((i) => round(c.cumulative[i], 3)), never: round(c.never, 3) };
}

/** Quantile `level` of a histogram series at the chosen year indices. */
function band(summary: Summary, series: string, level: number, years: number[]) {
  const q = summary.quantiles?.find((x) => x.series === series);
  const row = q ? q.values[q.levels.findIndex((l) => Math.abs(l - level) < 1e-9)] : undefined;
  return years.map((y) => dollars(row?.[y]));
}

export function digestSummary(summary: Summary, run: { id: string; paths: number; seed: number }) {
  const h = summary.headline;
  const axis = summary.axis;
  const years = pick(axis?.years.length ?? 0, 6);
  const label = (name: string) => summary.picks?.find((p) => p.label === name)?.path_index ?? null;
  return {
    run_id: run.id,
    paths: run.paths,
    seed: run.seed,
    years: [axis?.years[0] ?? null, axis?.years[axis.years.length - 1] ?? null],
    start_age: axis?.start_age ?? null,
    success: rate(h.success_rate),
    fi: rate(h.fi_rate),
    depletion: rate(h.depletion_rate),
    fi_age: ages(h.fi_age),
    retirement_age: ages(h.retirement_age),
    terminal_net_worth: money(h.terminal_net_worth),
    lifetime_tax_p50: dollars(h.median_lifetime_tax),
    fi_by_age: curve(summary.fi),
    depletion_by_age: curve(summary.depletion),
    by_age: {
      ages: years.map((y) => axis?.ages[y] ?? null),
      alive: years.map((y) => axis?.alive[y] ?? null),
      net_worth_p10: band(summary, 'net_worth', 0.1, years),
      net_worth_p50: band(summary, 'net_worth', 0.5, years),
      net_worth_p90: band(summary, 'net_worth', 0.9, years),
      spending_p50: band(summary, 'spending', 0.5, years),
    },
    spending_delivery: {
      delivered_ratio: round(h.delivered_spending_ratio?.value, 3),
      paths_below_90: rate(summary.spending?.paths_below_90),
      years_below_90: rate(summary.spending?.years_below_90),
      worst_cut_p90: round(summary.spending?.worst_cut?.[summary.spending.worst_cut_levels.indexOf(0.9)], 3),
    },
    failure_causes: [...(summary.failure?.causes ?? [])]
      .filter((c) => c.count > 0)
      .sort((a, b) => b.count - a.count)
      .slice(0, 3)
      .map((c) => ({ cause: c.cause, share_of_failed: round(c.share_of_failed, 3) })),
    trace_paths: { p10: label('p10'), median: label('median'), p90: label('p90'), failing: label('failing') },
  };
}

// ---- metrics ------------------------------------------------------------------------

export const METRICS = [
  'success_rate',
  'depletion_rate',
  'median_fi_age',
  'median_retirement_age',
  'median_terminal_net_worth',
  'p10_terminal_net_worth',
  'delivered_spending_ratio',
  'median_lifetime_tax',
] as const;

export type Metric = (typeof METRICS)[number];

export const METRIC_UNIT: Record<Metric, 'probability' | 'age' | 'dollars' | 'ratio'> = {
  success_rate: 'probability',
  depletion_rate: 'probability',
  median_fi_age: 'age',
  median_retirement_age: 'age',
  median_terminal_net_worth: 'dollars',
  p10_terminal_net_worth: 'dollars',
  delivered_spending_ratio: 'ratio',
  median_lifetime_tax: 'dollars',
};

/** The metric from a summary (scenario.rs `Metric::read`), null when it does not exist. */
export function readMetric(summary: Summary, metric: Metric): number | null {
  const h = summary.headline;
  const value = (() => {
    switch (metric) {
      case 'success_rate':
        return h.success_rate.value;
      case 'depletion_rate':
        return h.depletion_rate.value;
      case 'median_fi_age':
        return h.median_fi_age;
      case 'median_retirement_age':
        return h.median_retirement_age;
      case 'median_terminal_net_worth':
        return h.median_terminal_net_worth;
      case 'p10_terminal_net_worth':
        return h.p10_terminal_net_worth;
      case 'delivered_spending_ratio':
        return h.delivered_spending_ratio.value;
      case 'median_lifetime_tax':
        return h.median_lifetime_tax;
    }
  })();
  return value === null || value === undefined || !Number.isFinite(value) ? null : value;
}

export function roundMetric(metric: Metric, value: number | null): number | null {
  const unit = METRIC_UNIT[metric];
  return unit === 'dollars' ? dollars(value) : unit === 'age' ? round(value, 1) : round(value, 3);
}

/** Sampling standard error of a probability metric from one run; null for the others. */
export function metricStdError(summary: Summary, metric: Metric): number | null {
  const h = summary.headline;
  const e = metric === 'success_rate' ? h.success_rate : metric === 'depletion_rate' ? h.depletion_rate : metric === 'delivered_spending_ratio' ? h.delivered_spending_ratio : null;
  return e ? round(e.std_error, 4) : null;
}

// ---- paired comparison --------------------------------------------------------------

function column(summary: Summary, scalar: PathScalar): (number | null)[] | null {
  return summary.scatter?.columns.find((c) => c.scalar === scalar)?.values ?? null;
}

function quantile(sorted: number[], q: number): number | null {
  if (sorted.length === 0) return null;
  const pos = q * (sorted.length - 1);
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return (sorted[lo] as number) + ((sorted[hi] as number) - (sorted[lo] as number)) * (pos - lo);
}

export interface PairedOutcome {
  a: number | null;
  b: number | null;
  delta: number | null;
  std_error: number | null;
  lo: number | null;
  hi: number | null;
  /** Lives that fail under A and succeed under B. */
  rescued: number;
  /** Lives that succeed under A and fail under B. */
  harmed: number;
  unchanged: number;
  mcnemar_z: number | null;
  within_noise: boolean;
}

export interface PairedScalar {
  scalar: string;
  /** Lives with a value in both runs. */
  n: number;
  mean_delta: number | null;
  std_error: number | null;
  lo: number | null;
  hi: number | null;
  delta_p10: number | null;
  delta_p50: number | null;
  delta_p90: number | null;
  b_higher: number;
  a_higher: number;
  within_noise: boolean;
}

export interface ComparisonDigest {
  paired: true;
  /** Lives matched one to one between the two runs. */
  paired_paths: number;
  seed: number;
  method: 'engine' | 'sampled_lives';
  success: PairedOutcome | null;
  depleted?: PairedOutcome | null;
  scalars: PairedScalar[];
  /** B minus A on the headline figures of the full runs. */
  headline: { metric: string; a: number | null; b: number | null; delta: number | null }[];
}

const SCALARS: PathScalar[] = ['fi_age', 'retirement_age', 'terminal_net_worth', 'lifetime_tax', 'lifetime_spending', 'delivered_spending_ratio'];
const DOLLAR_SCALARS = new Set<string>(['terminal_net_worth', 'lifetime_tax', 'lifetime_spending', 'investable_at_retirement']);
const fit = (scalar: string, v: number | null) => (DOLLAR_SCALARS.has(scalar) ? dollars(v) : round(v, 3));

function headline(a: Summary, b: Summary): ComparisonDigest['headline'] {
  return (['success_rate', 'depletion_rate', 'median_fi_age', 'median_retirement_age', 'median_terminal_net_worth', 'p10_terminal_net_worth', 'delivered_spending_ratio', 'median_lifetime_tax'] as Metric[]).map(
    (metric) => {
      const va = readMetric(a, metric);
      const vb = readMetric(b, metric);
      return { metric, a: roundMetric(metric, va), b: roundMetric(metric, vb), delta: va === null || vb === null ? null : roundMetric(metric, vb - va) };
    },
  );
}

function fromEngine(c: Comparison, seed: number, a: Summary, b: Summary): ComparisonDigest {
  const outcome = (name: string): PairedOutcome | null => {
    const o = c.outcomes.find((x) => x.outcome === name);
    if (!o) return null;
    return {
      a: round(o.a.value, 3),
      b: round(o.b.value, 3),
      delta: round(o.difference.value, 4),
      std_error: round(o.difference.std_error, 4),
      lo: round(o.difference.lo, 4),
      hi: round(o.difference.hi, 4),
      rescued: o.b_only,
      harmed: o.a_only,
      unchanged: o.both + o.neither,
      mcnemar_z: round(o.z, 2),
      within_noise: o.within_noise,
    };
  };
  return {
    paired: true,
    paired_paths: c.paths,
    seed,
    method: 'engine',
    success: outcome('success'),
    depleted: outcome('depleted'),
    scalars: c.scalars
      .filter((s) => SCALARS.includes(s.scalar))
      .map((s) => ({
        scalar: s.scalar,
        n: s.both,
        mean_delta: fit(s.scalar, s.mean_delta.value),
        std_error: fit(s.scalar, s.mean_delta.std_error),
        lo: fit(s.scalar, s.mean_delta.lo),
        hi: fit(s.scalar, s.mean_delta.hi),
        delta_p10: fit(s.scalar, s.quantiles[2] ?? null),
        delta_p50: fit(s.scalar, s.quantiles[4] ?? null),
        delta_p90: fit(s.scalar, s.quantiles[6] ?? null),
        b_higher: s.b_higher,
        a_higher: s.a_higher,
        within_noise: s.within_noise,
      })),
    headline: c.headline.length > 0 ? c.headline.map((h) => ({ ...h })) : headline(a, b),
  };
}

/**
 * Paired statistics from the per-life sample every Summary carries (`scatter`: up to
 * 5,000 evenly spaced lives with their outcome and a few scalars). Both runs used the
 * same seed, so life i in one is the same world as life i in the other. Used when the
 * wasm build has no `compare` export; the formulas are the engine's (McNemar for the
 * success flips, a paired t interval for scalar differences).
 */
function fromSample(a: Summary, b: Summary, seed: number): ComparisonDigest {
  const indexB = new Map((b.scatter?.paths ?? []).map((path, i) => [path, i]));
  const pairs: [number, number][] = [];
  (a.scatter?.paths ?? []).forEach((path, i) => {
    const j = indexB.get(path);
    if (j !== undefined) pairs.push([i, j]);
  });
  const n = pairs.length;

  const sa = column(a, 'success');
  const sb = column(b, 'success');
  let success: PairedOutcome | null = null;
  if (sa && sb && n > 0) {
    let both = 0;
    let aOnly = 0;
    let bOnly = 0;
    let neither = 0;
    for (const [i, j] of pairs) {
      const x = (sa[i] ?? 0) >= 0.5;
      const y = (sb[j] ?? 0) >= 0.5;
      if (x && y) both++;
      else if (x) aOnly++;
      else if (y) bOnly++;
      else neither++;
    }
    const delta = (bOnly - aOnly) / n;
    const flips = (bOnly + aOnly) / n;
    const se = Math.sqrt(Math.max(0, flips - delta * delta) / n);
    success = {
      a: round((both + aOnly) / n, 3),
      b: round((both + bOnly) / n, 3),
      delta: round(delta, 4),
      std_error: round(se, 4),
      lo: round(delta - 1.96 * se, 4),
      hi: round(delta + 1.96 * se, 4),
      rescued: bOnly,
      harmed: aOnly,
      unchanged: both + neither,
      mcnemar_z: bOnly + aOnly > 0 ? round((bOnly - aOnly) / Math.sqrt(bOnly + aOnly), 2) : null,
      within_noise: Math.abs(delta) <= 1.96 * se,
    };
  }

  const scalars: PairedScalar[] = [];
  for (const scalar of SCALARS) {
    const ca = column(a, scalar);
    const cb = column(b, scalar);
    if (!ca || !cb) continue;
    const diffs: number[] = [];
    for (const [i, j] of pairs) {
      const x = ca[i];
      const y = cb[j];
      if (typeof x === 'number' && typeof y === 'number' && Number.isFinite(x) && Number.isFinite(y)) diffs.push(y - x);
    }
    if (diffs.length < 2) continue;
    const mean = diffs.reduce((s, d) => s + d, 0) / diffs.length;
    const variance = diffs.reduce((s, d) => s + (d - mean) ** 2, 0) / (diffs.length - 1);
    const se = Math.sqrt(variance / diffs.length);
    const sorted = [...diffs].sort((x, y) => x - y);
    scalars.push({
      scalar,
      n: diffs.length,
      mean_delta: fit(scalar, mean),
      std_error: fit(scalar, se),
      lo: fit(scalar, mean - 1.96 * se),
      hi: fit(scalar, mean + 1.96 * se),
      delta_p10: fit(scalar, quantile(sorted, 0.1)),
      delta_p50: fit(scalar, quantile(sorted, 0.5)),
      delta_p90: fit(scalar, quantile(sorted, 0.9)),
      b_higher: diffs.filter((d) => d > 0).length,
      a_higher: diffs.filter((d) => d < 0).length,
      within_noise: Math.abs(mean) <= 1.96 * se,
    });
  }
  return { paired: true, paired_paths: n, seed, method: 'sampled_lives', success, scalars, headline: headline(a, b) };
}

/** B minus A, life by life. `engineCompare` is core's `compare` when the build has it. */
export function compareRuns(a: RunRecord, b: RunRecord, engineCompare?: (a: RunRecord, b: RunRecord) => Comparison | null): ComparisonDigest {
  let exact: Comparison | null;
  try {
    exact = engineCompare?.(a, b) ?? null;
  } catch {
    exact = null;
  }
  return exact ? fromEngine(exact, a.seed, a.summary, b.summary) : fromSample(a.summary, b.summary, a.seed);
}

/** The path at quantile `q` of a per-life scalar, from the per-life sample. */
export function pathAtQuantile(summary: Summary, scalar: PathScalar, q: number): { path: number; value: number } | null {
  const values = column(summary, scalar);
  const paths = summary.scatter?.paths;
  if (!values || !paths) return null;
  const rows = values.flatMap((v, i) => (typeof v === 'number' && Number.isFinite(v) ? [{ path: paths[i] as number, value: v }] : []));
  if (rows.length === 0) return null;
  rows.sort((x, y) => x.value - y.value);
  return rows[Math.min(rows.length - 1, Math.max(0, Math.round(q * (rows.length - 1))))] ?? null;
}
