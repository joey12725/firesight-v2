// A stand-in for the simulation engine that returns plausible, deterministic Summaries
// computed from the profile, so tools can be tested (and the assistant smoke-tested)
// without the wasm simulation. Life i is the same "world" in every run, as with the
// real engine's common random numbers.
import { budgetTotals } from '@/editor/labels';
import type { ManyJob, ManyOptions, RunResult } from '@/engine/pool';
import type { AgeCurve, Estimate, PathScalar, PathTrace, Spread, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';

export interface Knobs {
  /** Share of lives that succeed. */
  success: number;
  /** Median FI age. */
  fiAge: number;
  /** Median terminal net worth. */
  terminal: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** A smooth, monotone response to the levers the tools vary. */
export function defaultKnobs(profile: Profile): Knobs {
  const spend = budgetTotals(profile).now;
  const trigger = profile.retirement.trigger;
  const retire = trigger.type === 'at_age' ? trigger.age : 60;
  const salary = profile.incomes.reduce((n, i) => n + (i.kind.type === 'job' ? i.kind.salary : 0), 0);
  const premium = profile.assumptions.market.equity_premium_adjustment;
  const success = clamp(0.8 + (retire - 60) * 0.03 - (spend - 60_000) / 150_000 + (salary - 120_000) / 600_000 + premium * 6, 0.02, 0.99);
  return { success, fiAge: clamp(52 + (spend - 60_000) / 8_000 - (salary - 120_000) / 30_000, 40, 75), terminal: Math.max(0, 1_200_000 + (retire - 60) * 150_000 - (spend - 60_000) * 12) };
}

function estimate(p: number, n: number): Estimate {
  const se = Math.sqrt((p * (1 - p)) / n);
  return { value: p, std_error: se, lo: clamp(p - 1.96 * se, 0, 1), hi: clamp(p + 1.96 * se, 0, 1) };
}

const spread = (p50: number, width: number, count: number): Spread => ({ p10: p50 - width, p25: p50 - width / 2, p50, p75: p50 + width / 2, p90: p50 + width, p50_lo: p50 - width / 20, p50_hi: p50 + width / 20, count });

function ageCurve(median: number, never: number): AgeCurve {
  const ages = Array.from({ length: 31 }, (_, i) => Math.round(median) - 15 + i);
  const cumulative = ages.map((a) => clamp((1 - never) / (1 + Math.exp(-(a - median) / 2)), 0, 1));
  return { ages, counts: ages.map(() => 0), cumulative, lo: cumulative.map((c) => Math.max(0, c - 0.01)), hi: cumulative.map((c) => Math.min(1, c + 0.01)), never, dots: [] };
}

/** A fixed pseudo-random number in (0, 1) for life `i`: the same in every run. */
const luck = (i: number) => {
  const x = Math.sin((i + 1) * 12.9898) * 43758.5453;
  return x - Math.floor(x);
};

export function fakeSummary(knobs: Knobs, paths: number, startAge = 35): Summary {
  const years = Array.from({ length: 60 }, (_, i) => 2026 + i);
  const ages = years.map((_, i) => startAge + i);
  const failed = Math.round((1 - knobs.success) * paths);
  const sample = Math.min(paths, 5000);
  const scatterPaths = Array.from({ length: sample }, (_, i) => Math.floor((i * paths) / sample));
  const column = (scalar: PathScalar, f: (u: number) => number | null) => ({ scalar, values: scatterPaths.map((p) => f(luck(p))) });
  const levels = [0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99];
  const curve = (final: number) => levels.map((l) => years.map((_, y) => Math.round(final * (0.2 + (0.8 * y) / 59) * (0.4 + 1.2 * l))));
  return {
    version: 1,
    paths,
    axis: { start: '2026-01', start_age: startAge, first_year_months: 12, years, ages, alive: years.map((_, y) => Math.round(paths * (1 - Math.max(0, y - 40) / 40))) },
    headline: {
      success_rate: estimate(knobs.success, paths),
      fi_rate: estimate(clamp(knobs.success + 0.05, 0, 1), paths),
      retired_rate: estimate(clamp(knobs.success + 0.05, 0, 1), paths),
      depletion_rate: estimate(1 - knobs.success, paths),
      median_fi_age: knobs.fiAge,
      fi_age: spread(knobs.fiAge, 5, paths),
      median_retirement_age: knobs.fiAge,
      retirement_age: spread(knobs.fiAge, 5, paths),
      depletion_age: spread(84, 6, failed),
      years_depleted: spread(5, 3, failed),
      median_terminal_net_worth: knobs.terminal,
      p10_terminal_net_worth: knobs.terminal * 0.1,
      terminal_net_worth: { ...spread(knobs.terminal, knobs.terminal * 0.9, paths), p10: knobs.terminal * 0.1 },
      investable_at_retirement: spread(1_500_000, 400_000, paths),
      delivered_spending_ratio: { value: 0.97, std_error: 0.002, lo: 0.966, hi: 0.974 },
      years_cutting_share: 0.06,
      median_lifetime_tax: 640_000,
      lifetime_tax: spread(640_000, 200_000, paths),
      lifetime_spending: spread(3_600_000, 500_000, paths),
      funded_ratio: null,
    },
    spending: {
      retired_paths: paths,
      paths_below_90: estimate(0.12, paths),
      paths_below_75: estimate(0.04, paths),
      years_below_90: estimate(0.03, paths),
      years_below_75: estimate(0.01, paths),
      years_cut: spread(4, 2, paths),
      worst_cut_levels: [0.5, 0.75, 0.9, 0.95, 0.99],
      worst_cut: [0, 0.02, 0.14, 0.22, 0.4],
      by_year: { retired: [], below_90: [], below_75: [] },
    },
    quantiles: [
      { series: 'net_worth', levels, values: curve(knobs.terminal) },
      { series: 'spending', levels, values: curve(70_000) },
    ],
    density: [],
    status: { statuses: ['working', 'retired_surplus', 'retired_on_plan', 'retired_cutting', 'depleted', 'dead'], shares: [] },
    survival: { household: [], people: [] },
    composition: [],
    fi: ageCurve(knobs.fiAge, clamp(1 - knobs.success - 0.05, 0, 1)),
    retirement: ageCurve(knobs.fiAge, clamp(1 - knobs.success - 0.05, 0, 1)),
    depletion: ageCurve(84, knobs.success),
    distributions: [],
    scatter: {
      paths: scatterPaths,
      columns: [
        column('success', (u) => (u < knobs.success ? 1 : 0)),
        column('terminal_net_worth', (u) => Math.round(knobs.terminal * 2 * (1 - u))),
        column('fi_age', (u) => (u < knobs.success + 0.05 ? knobs.fiAge - 5 + 10 * u : null)),
        column('retirement_age', (u) => (u < knobs.success + 0.05 ? knobs.fiAge - 5 + 10 * u : null)),
        column('depletion_age', (u) => (u < knobs.success ? null : 80 + 10 * u)),
        column('death_age_0', (u) => 78 + 20 * u),
        column('investable_at_retirement', (u) => 1_000_000 + 1_000_000 * (1 - u)),
        column('early_retirement_return', (u) => 0.08 - 0.12 * u),
      ],
    },
    failure: {
      failed,
      never_retired: 0,
      depleted: failed,
      causes: [
        { cause: 'market_sequence', count: Math.round(failed * 0.6), share_of_failed: failed ? 0.6 : null, share_of_all: (failed * 0.6) / paths },
        { cause: 'longevity', count: Math.round(failed * 0.25), share_of_failed: failed ? 0.25 : null, share_of_all: (failed * 0.25) / paths },
        { cause: 'job_loss', count: Math.round(failed * 0.15), share_of_failed: failed ? 0.15 : null, share_of_all: (failed * 0.15) / paths },
        { cause: 'health', count: 0, share_of_failed: 0, share_of_all: 0 },
      ],
      factors: [],
      contrasts: [],
    },
    picks: [
      { label: 'p10', scalar: 'terminal_net_worth', path_index: 3, value: knobs.terminal * 0.1 },
      { label: 'median', scalar: 'terminal_net_worth', path_index: 7, value: knobs.terminal },
      { label: 'p90', scalar: 'terminal_net_worth', path_index: 11, value: knobs.terminal * 1.9 },
      { label: 'median_fi', scalar: 'fi_age', path_index: 13, value: knobs.fiAge },
      ...(failed > 0 ? [{ label: 'failing' as const, scalar: 'depletion_age' as const, path_index: 17, value: 84 }] : []),
    ],
    samples: [],
  };
}

export function fakeTrace(path: number, seed: number): PathTrace {
  const years = [2026, 2027, 2028, 2029];
  const column = (key: string, values: number[]) => ({ key, unit: 'dollars', values });
  return {
    path_index: path,
    seed,
    start: '2026-01',
    months: 48,
    statuses: ['working', 'retired_surplus', 'retired_on_plan', 'retired_cutting', 'depleted', 'dead'],
    status: [],
    columns: [],
    annual: {
      years,
      months: [12, 12, 12, 12],
      status: [0, 0, 2, 3],
      columns: [
        column('gross_income', [128_400, 131_900, 0, 0]),
        column('spending', [61_200, 62_800, 58_100, 49_300]),
        column('total_tax', [27_300, 28_100, 3_200, 2_100]),
        column('net_worth', [212_000, 265_400, 241_900, 198_700]),
      ],
    },
    tax_returns: years.map((year, i) => ({
      year,
      price_level: 1 + 0.03 * i,
      result: {},
      lines: [
        { key: 'wages', label: 'Wages, salaries, tips', amount: 128_400 * (1 + 0.03 * i), note: null },
        { key: 'standard_deduction', label: 'Standard deduction', amount: 16_100, note: 'Single filer' },
        { key: 'total_tax', label: 'Total tax', amount: 27_300, note: null },
      ],
    })),
    events: [
      { month: '2027-03', kind: 'market', label: 'Stocks fall 31% over 14 months', amount: null },
      { month: '2027-09', kind: 'laid_off', label: 'Laid off', amount: null },
      { month: '2028-01', kind: 'retired', label: 'Retired', amount: null },
      { month: '2029-02', kind: 'spending_cut', label: 'Spending cut 15%', amount: -9_000 },
    ],
    scalars: [
      { scalar: 'fi_age', value: 37 },
      { scalar: 'terminal_net_worth', value: 198_700 },
      { scalar: 'success', value: 0 },
      { scalar: 'depletion_age', value: null },
    ],
  };
}

export interface FakePool {
  runMany(jobs: ManyJob[], options?: ManyOptions): Promise<Map<string, RunResult>>;
  trace(profile: Profile, path: number, seed?: number): Promise<PathTrace>;
  /** How many profiles have been simulated, for cache assertions. */
  simulated: number;
  batches: number[];
}

export function fakePool(knobs: (profile: Profile) => Knobs = defaultKnobs, options: { fail?: () => Error | null } = {}): FakePool {
  const pool: FakePool = {
    simulated: 0,
    batches: [],
    async runMany(jobs, opts = {}) {
      const failure = options.fail?.();
      if (failure) throw failure;
      if (opts.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
      pool.simulated += jobs.length;
      pool.batches.push(jobs.length);
      return new Map(
        jobs.map((job) => [
          job.id,
          { summary: fakeSummary(knobs(job.profile), job.paths), acc: new Uint8Array(0), paths: job.paths, seed: opts.seed ?? 1, elapsedMs: 1, pathsPerSecond: 1e6, profileKey: job.id },
        ]),
      );
    },
    async trace(_profile, path, seed = 1) {
      return fakeTrace(path, seed);
    },
  };
  return pool;
}
