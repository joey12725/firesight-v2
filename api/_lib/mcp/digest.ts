// Compact forms of engine output for tool results. The engine's full summary of a run is
// hundreds of kilobytes; an agent gets the engine's own digest (about 2 KB) and a cut of
// its paired comparison. Nothing here computes a statistic: the engine does that, and
// this file chooses and rounds.
//
// The server's copy of web/src/assistant/tools/digest.ts, so that a figure read through
// the hosted MCP server is the figure the website's own tools give. A parity test
// (api/_tests/mcp-parity.test.ts) holds the two equal.

import { compare, digest, headline } from '../engine.js';
import type { Comparison, OutcomeComparison, Run } from '../engine.js';

const dollars = (v: number | null | undefined) => (v === null || v === undefined || !Number.isFinite(v) ? null : Math.round(v / 1000) * 1000);
const round = (v: number | null | undefined, dp: number) => {
  if (v === null || v === undefined || !Number.isFinite(v)) return null;
  const f = 10 ** dp;
  return Math.round(v * f) / f;
};

/** The engine's digest of a run, with the seed and the three headline figures it leaves out. */
export function digestRun(run: Run): Record<string, unknown> {
  const h = headline(run);
  return {
    seed: run.seed,
    ...digest(run),
    // Lives that sell a home as a last resort count as success, so the share is reported with it.
    home_sale: { p: round(h.home_sale_rate.value, 3) ?? 0, lo: round(h.home_sale_rate.lo, 3) ?? 0, hi: round(h.home_sale_rate.hi, 3) ?? 0 },
    plan_delivered: round(h.plan_delivered_share.value, 3),
    initial_withdrawal_rate_p50: round(h.initial_withdrawal_rate.p50, 4),
  };
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

export interface ComparisonDigest {
  paired: true;
  /** Lives matched one to one between the two runs. */
  paired_paths: number;
  seed: number;
  success: PairedOutcome | null;
  depleted: PairedOutcome | null;
  /** Share of the paired lives that sell a home as a last resort under A and under B. */
  home_sale: { a: number; b: number; delta: number } | null;
  scalars: Record<string, unknown>[];
  /** B minus A on the headline figures of the full runs. */
  headline: { metric: string; a: number | null; b: number | null; delta: number | null }[];
}

const SCALARS = ['fi_age', 'retirement_age', 'terminal_net_worth', 'lifetime_tax', 'lifetime_spending', 'delivered_spending_ratio', 'extra_spending_ratio'];
/** A quantile of the per-life difference, found by the levels the comparison itself states. */
const at = (levels: readonly number[], quantiles: (number | null)[], level: number) => quantiles[levels.findIndex((l) => Math.abs(l - level) < 1e-9)] ?? null;

function fitHeadline(metric: string, v: number | null) {
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

/** Home sales under each plan, from the presence counts of the `home_sale_age` scalar. */
function homeSale(c: Comparison): ComparisonDigest['home_sale'] {
  const s = c.scalars.find((x) => x.scalar === 'home_sale_age');
  if (!s || c.paths <= 0) return null;
  const a = (s.both + s.a_only) / c.paths;
  const b = (s.both + s.b_only) / c.paths;
  return { a: round(a, 3) ?? 0, b: round(b, 3) ?? 0, delta: round(b - a, 4) ?? 0 };
}

/** The part of the engine's paired comparison (B minus A, life by life) that answers a what-if. */
export function digestComparison(c: Comparison, seed: number): ComparisonDigest {
  return {
    paired: true,
    paired_paths: c.paths,
    seed,
    success: outcome(c.outcomes.find((o) => o.outcome === 'success')),
    depleted: outcome(c.outcomes.find((o) => o.outcome === 'depleted')),
    home_sale: homeSale(c),
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
          delta_p10: fit(at(c.quantile_levels, s.quantiles, 0.1)),
          delta_p50: fit(at(c.quantile_levels, s.quantiles, 0.5)),
          delta_p90: fit(at(c.quantile_levels, s.quantiles, 0.9)),
          b_higher: round(s.b_higher, 3),
          a_higher: round(s.a_higher, 3),
          within_noise: s.within_noise,
        };
      }),
    headline: c.headline.map((h) => ({ metric: h.metric, a: fitHeadline(h.metric, h.a), b: fitHeadline(h.metric, h.b), delta: fitHeadline(h.metric, h.delta) })),
  };
}

/** The paired comparison of two runs made on the same lives. `a` is the baseline. */
export function compareRuns(a: Run, b: Run): ComparisonDigest {
  return digestComparison(compare(a, b), a.seed);
}
