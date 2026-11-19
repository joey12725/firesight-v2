// What the Outcomes charts draw, derived from the Summary. Pure functions, so the
// numbers behind every mark and sentence can be tested without a browser.
import type { AgeCurve, PathScalar, ScalarDistribution, ScalarUnit, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { statusShares } from '@/ui/charts';

export const distribution = (summary: Summary, scalar: PathScalar): ScalarDistribution | undefined => summary.distributions.find((d) => d.scalar === scalar);

/** `n` values picked evenly from equally likely `dots`, keeping their order. */
export function resample<T>(dots: T[], n: number): T[] {
  if (n <= 0 || dots.length === 0) return [];
  if (n === dots.length) return dots;
  return Array.from({ length: n }, (_, j) => dots[Math.min(dots.length - 1, Math.floor(((j + 0.5) / n) * dots.length))] as T);
}

/**
 * Dots that stand for every simulated life. A scalar's own dots cover only the lives
 * that have a value; the rest ("never retired", so no retirement spending to cut) are
 * added as nulls in proportion, and under 100 lives one dot is drawn per life.
 */
export function livesDots(dist: ScalarDistribution | undefined, lives: number): (number | null)[] {
  if (!dist) return [];
  const all = dist.count + dist.missing;
  if (all === 0) return [];
  const total = Math.min(100, Math.max(1, lives));
  const missing = Math.round((dist.missing / all) * total);
  const values = resample(dist.dots.filter((d): d is number => d !== null), dist.dots.length === 0 ? 0 : total - missing);
  return [...values, ...new Array<null>(total - values.length).fill(null)];
}

/** The dots of an age curve (null is "never"), one per life when fewer than 100 were simulated. */
export function curveDots(curve: AgeCurve, lives: number): (number | null)[] {
  return lives >= curve.dots.length || lives < 1 ? curve.dots : resample(curve.dots, lives);
}

/** True when two sets of quantile dots are the same distribution for a reader: all but a few dots within a quarter of a year. */
export function sameDots(a: (number | null)[], b: (number | null)[]): boolean {
  if (a.length !== b.length || a.length === 0) return false;
  const close = a.filter((v, i) => v === b[i] || (v !== null && b[i] !== null && Math.abs(v - (b[i] as number)) < 0.25)).length;
  return close >= a.length * 0.97;
}

/** Quantile `q` (0..1) of sorted values, linear between neighbours. */
export function quantileSorted(sorted: number[], q: number): number | null {
  if (sorted.length === 0) return null;
  const at = Math.min(1, Math.max(0, q)) * (sorted.length - 1);
  const i = Math.floor(at);
  const a = sorted[i] as number;
  const b = sorted[Math.min(sorted.length - 1, i + 1)] as number;
  return a + (b - a) * (at - i);
}

const median = (values: number[]) => quantileSorted([...values].sort((a, b) => a - b), 0.5);

// ---- luck early in retirement --------------------------------------------------------

export interface ScatterLife {
  /** Path index: `navigateToLife(path, 'scatter')` opens it. */
  path: number;
  /** Annualised real portfolio return over the first ten retirement years. */
  ret: number;
  wealth: number;
  depletionAge: number | null;
  retirementAge: number | null;
  deathAge: number | null;
  /** True when the money never ran short. */
  held: boolean;
}

/** The scatter sample as rows: lives that retired and so have a first-decade return. */
export function scatterLives(summary: Summary): ScatterLife[] {
  const col = (scalar: PathScalar) => summary.scatter.columns.find((c) => c.scalar === scalar)?.values ?? [];
  const ret = col('early_retirement_return');
  const wealth = col('terminal_net_worth');
  const depletion = col('depletion_age');
  const retirement = col('retirement_age');
  const death = col('death_age_0');
  const out: ScatterLife[] = [];
  summary.scatter.paths.forEach((path, i) => {
    const r = ret[i];
    const w = wealth[i];
    if (r === null || r === undefined || w === null || w === undefined) return;
    const d = depletion[i] ?? null;
    out.push({ path, ret: r, wealth: w, depletionAge: d, retirementAge: retirement[i] ?? null, deathAge: death[i] ?? null, held: d === null });
  });
  return out;
}

export interface ReturnBin {
  lo: number;
  hi: number;
  /** Median return of the lives in the bin: where its trend point sits. */
  mid: number;
  lives: number;
  short: number;
  shortShare: number;
  medianWealth: number;
  p25Wealth: number;
  p75Wealth: number;
}

/**
 * The trend through the scatter: lives ranked by first-decade return and cut into bins
 * of equal size, so every point of the trend rests on the same number of lives.
 */
export function returnBins(lives: ScatterLife[], wanted?: number): ReturnBin[] {
  const n = lives.length;
  if (n < 20) return [];
  const count = wanted ?? Math.min(12, Math.max(4, Math.floor(n / 60)));
  const ranked = [...lives].sort((a, b) => a.ret - b.ret);
  const out: ReturnBin[] = [];
  for (let b = 0; b < count; b++) {
    const slice = ranked.slice(Math.floor((b * n) / count), Math.floor(((b + 1) * n) / count));
    if (slice.length === 0) continue;
    const wealth = slice.map((l) => l.wealth).sort((x, y) => x - y);
    const short = slice.filter((l) => !l.held).length;
    out.push({
      lo: (slice[0] as ScatterLife).ret,
      hi: (slice[slice.length - 1] as ScatterLife).ret,
      mid: quantileSorted(slice.map((l) => l.ret), 0.5) as number,
      lives: slice.length,
      short,
      shortShare: short / slice.length,
      medianWealth: quantileSorted(wealth, 0.5) as number,
      p25Wealth: quantileSorted(wealth, 0.25) as number,
      p75Wealth: quantileSorted(wealth, 0.75) as number,
    });
  }
  return out;
}

export interface LuckSplit {
  /** Lives at or below this first-decade return are the unluckiest fifth. */
  threshold: number;
  unlucky: { lives: number; shortShare: number; medianWealth: number | null };
  rest: { lives: number; shortShare: number; medianWealth: number | null };
}

/** The unluckiest fifth of early-retirement returns against everyone else. */
export function luckSplit(lives: ScatterLife[]): LuckSplit | null {
  if (lives.length < 20) return null;
  const ranked = [...lives].sort((a, b) => a.ret - b.ret);
  const cut = Math.max(1, Math.round(ranked.length / 5));
  const part = (rows: ScatterLife[]) => ({ lives: rows.length, shortShare: rows.length ? rows.filter((l) => !l.held).length / rows.length : 0, medianWealth: median(rows.map((l) => l.wealth)) });
  return { threshold: (ranked[cut - 1] as ScatterLife).ret, unlucky: part(ranked.slice(0, cut)), rest: part(ranked.slice(cut)) };
}

export interface Selection {
  lo: number;
  hi: number;
  lives: ScatterLife[];
  shortShare: number;
  medianWealth: number | null;
}

/** The lives whose first-decade return lies in [lo, hi], worst endings first. */
export function selectLives(lives: ScatterLife[], lo: number, hi: number): Selection {
  const rows = lives.filter((l) => l.ret >= lo && l.ret <= hi).sort((a, b) => Number(a.held) - Number(b.held) || (a.held ? a.wealth - b.wealth : (a.depletionAge ?? 0) - (b.depletionAge ?? 0)));
  return { lo, hi, lives: rows, shortShare: rows.length ? rows.filter((l) => !l.held).length / rows.length : 0, medianWealth: median(rows.map((l) => l.wealth)) };
}

// ---- does the money outlast you ------------------------------------------------------

/** Share of all lives whose money has run short by exact age `age` (entry i of the curve is the share by the END of age `ages[i]`). */
export function depletedBy(curve: AgeCurve, age: number): number {
  const { ages, cumulative } = curve;
  if (ages.length === 0) return 0;
  const first = ages[0] as number;
  if (age <= first) return 0;
  const i = Math.floor(age) - first;
  if (i >= ages.length) return cumulative[cumulative.length - 1] ?? 0;
  const before = i === 0 ? 0 : (cumulative[i - 1] ?? 0);
  const after = cumulative[i] ?? before;
  return before + (after - before) * (age - Math.floor(age));
}

export interface LastingModel {
  /** Share of lives with someone still alive, by year index. */
  alive: number[];
  /** Share of lives whose money has not run short by that age (whether or not anyone is still alive). */
  lasted: number[];
  /** Share of all lives that are alive and short of essentials that year: the engine's joint count, not a product. */
  short: number[];
  /** Of the lives still going, the share short of essentials (the engine's own figure). Null where almost nobody is alive. */
  shortAmongLiving: (number | null)[];
  /** Among the lives short that year, the mean shortfall of spending against plan, a year. Null when nobody is. */
  shortfall: (number | null)[];
  people: number[][];
}

export function lastingModel(summary: Summary): LastingModel {
  const alive = summary.survival.household;
  const short = statusShares(summary, 'depleted');
  return {
    alive,
    lasted: summary.axis.ages.map((age) => 1 - depletedBy(summary.depletion, age)),
    short: summary.axis.ages.map((_, y) => short[y] ?? 0),
    shortAmongLiving: summary.axis.ages.map((_, y) => ((summary.axis.alive[y] ?? 0) >= 30 ? (summary.spending.by_year.depleted[y] ?? null) : null)),
    shortfall: summary.axis.ages.map((_, y) => summary.spending.by_year.depleted_shortfall[y] ?? null),
    people: summary.survival.people,
  };
}

// ---- what was different about the lives that ran short -------------------------------

/** The circumstances a life is dealt or chooses before the outcome is known: the outcome scalars themselves are left out. */
export const CIRCUMSTANCES: { scalar: PathScalar; label: string; higher: string; lower: string }[] = [
  { scalar: 'early_retirement_return', label: 'Return in the first ten retirement years', higher: 'better early returns', lower: 'worse early returns' },
  { scalar: 'worst_drawdown', label: 'Deepest market fall lived through', higher: 'a deeper market fall', lower: 'a shallower market fall' },
  { scalar: 'long_term_care_cost', label: 'Long-term care paid', higher: 'more care', lower: 'less care' },
  { scalar: 'death_age_0', label: 'Age at death', higher: 'a longer life', lower: 'a shorter life' },
  { scalar: 'months_unemployed', label: 'Months out of work', higher: 'more time out of work', lower: 'less time out of work' },
  { scalar: 'inflation', label: 'Average inflation', higher: 'higher inflation', lower: 'lower inflation' },
  { scalar: 'retirement_age', label: 'Age at retirement', higher: 'a later retirement', lower: 'an earlier retirement' },
  { scalar: 'investable_at_retirement', label: 'Savings at retirement', higher: 'more saved', lower: 'less saved' },
  { scalar: 'funded_ratio', label: 'Funded ratio at retirement', higher: 'a fuller pot', lower: 'a thinner pot' },
  { scalar: 'initial_withdrawal_rate', label: 'Withdrawal rate in the first retired year', higher: 'a higher starting withdrawal rate', lower: 'a lower starting withdrawal rate' },
];

export interface Contrast {
  scalar: PathScalar;
  label: string;
  unit: ScalarUnit;
  failed: number;
  succeeded: number;
  failedCount: number;
  succeededCount: number;
  /** The gap between the two medians in units of the middle half of all lives: how far apart the groups sit. */
  separation: number;
  /** Which way the lives that ran short differ, in words. */
  direction: string;
  /** Axis extent: the 1st to 99th percentile of all lives. */
  domain: [number, number];
  edges: number[];
  counts: number[];
  /** Share of each group in the tail quarter of all lives, when the engine tags this scalar. */
  tail: { failed: number; succeeded: number; above: boolean; threshold: number } | null;
}

/** Failed against successful lives on each circumstance, most different first. */
export function contrasts(summary: Summary): Contrast[] {
  const out: Contrast[] = [];
  for (const c of CIRCUMSTANCES) {
    const row = summary.failure.contrasts.find((x) => x.scalar === c.scalar);
    const dist = distribution(summary, c.scalar);
    if (!row || !dist || row.failed_median === null || row.succeeded_median === null || row.failed_count < 5 || row.succeeded_count < 5) continue;
    const q = dist.quantiles;
    const [p1, p10, p25, p75, p90, p99] = [q[0], q[2], q[3], q[5], q[6], q[8]];
    if (p1 === null || p1 === undefined || p99 === null || p99 === undefined || !(p99 > p1)) continue;
    const middle = (p75 ?? 0) - (p25 ?? 0) > 0 ? (p75 as number) - (p25 as number) : (p90 ?? 0) - (p10 ?? 0) > 0 ? ((p90 as number) - (p10 as number)) / 2 : (dist.std_dev ?? 0) || p99 - p1;
    const gap = row.failed_median - row.succeeded_median;
    const factor = summary.failure.factors.find((f) => f.scalar === c.scalar && f.failed !== null && f.succeeded !== null && f.threshold !== null);
    out.push({
      scalar: c.scalar,
      label: c.label,
      unit: dist.unit,
      failed: row.failed_median,
      succeeded: row.succeeded_median,
      failedCount: row.failed_count,
      succeededCount: row.succeeded_count,
      separation: Math.abs(gap) / middle,
      direction: gap > 0 ? c.higher : c.lower,
      domain: [Math.min(p1, row.failed_median, row.succeeded_median), Math.max(p99, row.failed_median, row.succeeded_median)],
      edges: dist.edges,
      counts: dist.counts,
      tail: factor ? { failed: factor.failed as number, succeeded: factor.succeeded as number, above: factor.above, threshold: factor.threshold as number } : null,
    });
  }
  return out.sort((a, b) => b.separation - a.separation);
}

// ---- historical cohorts --------------------------------------------------------------

/** The historical record the engine replays: path i starts at month i (mod the count) after this month. docs/internal/module-economy.md. */
export const HISTORY = { firstYear: 1926, firstMonth: 7, months: 1203 };

export const isHistorical = (profile: Profile) => profile.assumptions.market.kind.type === 'historical';

/** The month of history a path starts in under the historical model. */
export function cohortOf(pathIndex: number): { year: number; month: number } {
  const offset = HISTORY.firstMonth - 1 + (((pathIndex % HISTORY.months) + HISTORY.months) % HISTORY.months);
  return { year: HISTORY.firstYear + Math.floor(offset / 12), month: (offset % 12) + 1 };
}

/** The lowest path index that starts in January of `year` (July for the first year of the record), or null outside the record. */
export function pathForYear(year: number): number | null {
  const index = year === HISTORY.firstYear ? 0 : (year - HISTORY.firstYear) * 12 - (HISTORY.firstMonth - 1);
  return index >= 0 && index < HISTORY.months ? index : null;
}

/** The last calendar year of the record (it ends part-way through). */
export const historyLastYear = () => cohortOf(HISTORY.months - 1).year;

export interface CohortYear {
  year: number;
  lives: number;
  short: number;
  shortShare: number | null;
  medianWealth: number | null;
  /** True when a life starting this year outruns the record and continues from its first month. */
  wraps: boolean;
}

/** Outcomes by the year of history a life starts in, from the scatter sample (lives that never retire included). */
export function cohortYears(summary: Summary): CohortYear[] {
  const depletion = summary.scatter.columns.find((c) => c.scalar === 'depletion_age')?.values ?? [];
  const wealth = summary.scatter.columns.find((c) => c.scalar === 'terminal_net_worth')?.values ?? [];
  const last = historyLastYear();
  const horizon = summary.axis.years.length;
  const groups = new Map<number, { short: number; wealth: number[] }>();
  summary.scatter.paths.forEach((path, i) => {
    const { year } = cohortOf(path);
    const g = groups.get(year) ?? { short: 0, wealth: [] };
    if ((depletion[i] ?? null) !== null) g.short += 1;
    const w = wealth[i];
    if (w !== null && w !== undefined) g.wealth.push(w);
    groups.set(year, g);
  });
  const out: CohortYear[] = [];
  for (let year = HISTORY.firstYear; year <= last; year++) {
    const g = groups.get(year);
    const lives = g?.wealth.length ?? 0;
    out.push({ year, lives, short: g?.short ?? 0, shortShare: lives ? (g?.short ?? 0) / lives : null, medianWealth: g ? median(g.wealth) : null, wraps: year + horizon - 1 > last });
  }
  return out;
}

// ---- formatting by unit ----------------------------------------------------------------

export function formatUnit(value: number, unit: ScalarUnit, compactMoney: (v: number) => string): string {
  switch (unit) {
    case 'dollars':
      return compactMoney(value);
    case 'rate':
      return `${(value * 100).toFixed(1).replace(/^-(0(\.0+)?)$/, '$1').replace('-', '−')}%`;
    case 'ratio':
      return value.toFixed(2);
    case 'age':
      return String(Math.round(value));
    case 'months':
      return `${Math.round(value)} mo`;
    case 'years':
      return `${Math.round(value)} yr`;
    default:
      return String(Math.round(value * 100) / 100);
  }
}
