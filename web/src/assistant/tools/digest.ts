// Compact forms of engine output for tool results. A Summary is hundreds of kilobytes;
// the model gets core's own digest of a run (about 1.7 KB) and a cut of core's paired
// comparison, with the ids to ask for more. Nothing here computes a statistic: the
// engine does that, and this file chooses and rounds.
import type { Comparison, ComparisonMetric, OutcomeComparison, PathScalar, Summary, SummaryDigest } from '@/engine/output-types';
import type { RunRecord } from './runs';
import type { EngineBridge } from './types';

const dollars = (v: number | null | undefined) => (v === null || v === undefined || !Number.isFinite(v) ? null : Math.round(v / 1000) * 1000);
const round = (v: number | null | undefined, dp: number) => {
  if (v === null || v === undefined || !Number.isFinite(v)) return null;
  const f = 10 ** dp;
  return Math.round(v * f) / f;
};

/** Core's digest of a run, with what the tools add: the id to refer back to it, and its seed. */
export type RunDigest = { run_id: string; seed: number } & SummaryDigest;

export function digestRun(engine: Pick<EngineBridge, 'digest'>, run: RunRecord): RunDigest {
  return { run_id: run.id, seed: run.seed, ...engine.digest(run) };
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

// ---- paired comparison --------------------------------------------------------------

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
  scalar: PathScalar;
  /** Lives with a value in both runs. */
  n: number;
  mean_delta: number | null;
  std_error: number | null;
  lo: number | null;
  hi: number | null;
  delta_p10: number | null;
  delta_p50: number | null;
  delta_p90: number | null;
  /** Shares of the `n` lives where B is higher, and where A is higher. */
  b_higher: number | null;
  a_higher: number | null;
  within_noise: boolean;
}

export interface ComparisonDigest {
  paired: true;
  /** Lives matched one to one between the two runs. */
  paired_paths: number;
  seed: number;
  success: PairedOutcome | null;
  depleted: PairedOutcome | null;
  scalars: PairedScalar[];
  /** B minus A on the headline figures of the full runs. */
  headline: { metric: ComparisonMetric; a: number | null; b: number | null; delta: number | null }[];
}

const SCALARS: PathScalar[] = ['fi_age', 'retirement_age', 'terminal_net_worth', 'lifetime_tax', 'lifetime_spending', 'delivered_spending_ratio'];
/** Core reports the per-life difference at these quantiles, in this order. */
const DELTA_LEVELS = [0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99];
const at = (quantiles: (number | null)[], level: number) => quantiles[DELTA_LEVELS.indexOf(level)] ?? null;

function fitHeadline(metric: ComparisonMetric, v: number | null) {
  if (metric.endsWith('_rate') || metric.endsWith('_ratio')) return round(v, 3);
  return metric.endsWith('_age') ? round(v, 1) : dollars(v);
}

function outcome(o: OutcomeComparison | undefined): PairedOutcome | null {
  if (!o) return null;
  // `a_only` and `b_only` count lives where the outcome happened in one run only; for
  // "depleted" it is the lives that deplete under A alone that the change rescues.
  const good = o.outcome !== 'depleted';
  return {
    a: round(o.a.value, 3),
    b: round(o.b.value, 3),
    delta: round(o.difference.value, 4),
    std_error: round(o.difference.std_error, 4),
    lo: round(o.difference.lo, 4),
    hi: round(o.difference.hi, 4),
    rescued: good ? o.b_only : o.a_only,
    harmed: good ? o.a_only : o.b_only,
    unchanged: o.both + o.neither,
    mcnemar_z: round(o.z, 2),
    within_noise: o.within_noise,
  };
}

/** The part of core's paired comparison (B minus A, life by life) that answers a what-if. */
export function digestComparison(c: Comparison, seed: number): ComparisonDigest {
  return {
    paired: true,
    paired_paths: c.paths,
    seed,
    success: outcome(c.outcomes.find((o) => o.outcome === 'success')),
    depleted: outcome(c.outcomes.find((o) => o.outcome === 'depleted')),
    scalars: c.scalars
      .filter((s) => SCALARS.includes(s.scalar))
      .map((s) => {
        const fit = (v: number | null) => (s.unit === 'dollars' ? dollars(v) : round(v, 3));
        return {
          scalar: s.scalar,
          n: s.both,
          mean_delta: fit(s.mean_delta.value),
          std_error: fit(s.mean_delta.std_error),
          lo: fit(s.mean_delta.lo),
          hi: fit(s.mean_delta.hi),
          delta_p10: fit(at(s.quantiles, 0.1)),
          delta_p50: fit(at(s.quantiles, 0.5)),
          delta_p90: fit(at(s.quantiles, 0.9)),
          b_higher: round(s.b_higher, 3),
          a_higher: round(s.a_higher, 3),
          within_noise: s.within_noise,
        };
      }),
    headline: c.headline.map((h) => ({ metric: h.metric, a: fitHeadline(h.metric, h.a), b: fitHeadline(h.metric, h.b), delta: fitHeadline(h.metric, h.delta) })),
  };
}

/** The path at quantile `q` of a per-life scalar, from the per-life sample every Summary carries. */
export function pathAtQuantile(summary: Summary, scalar: PathScalar, q: number): { path: number; value: number } | null {
  const values = summary.scatter?.columns.find((c) => c.scalar === scalar)?.values;
  const paths = summary.scatter?.paths;
  if (!values || !paths) return null;
  const rows = values.flatMap((v, i) => (typeof v === 'number' && Number.isFinite(v) ? [{ path: paths[i] as number, value: v }] : []));
  if (rows.length === 0) return null;
  rows.sort((x, y) => x.value - y.value);
  return rows[Math.min(rows.length - 1, Math.max(0, Math.round(q * (rows.length - 1))))] ?? null;
}
