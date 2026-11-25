// What the Equity view draws and counts, derived from `Summary.equity`, the per-life
// distributions and a paired side run. Pure functions: no React, no engine calls, so
// every number behind a mark or a sentence is tested on the startup demo's real output.
import { paperValue } from '@/engine/equity-math';
import type { AgeCurve, Comparison, EquityConditional, EquityOutcome, EquitySummary, OutcomeComparison, PairedByOutcome, PathTrace, ScalarDistribution, Summary } from '@/engine/output-types';
import { EQUITY_OUTCOMES } from '@/engine/output-types';
import type { PrivateCompany, Profile } from '@/engine/types';
import { compositionSeries } from '@/ui/charts/summary';
import { livesDots } from '../outcomes/data';
import { equityDecisions, equityTypicalOdds, equityZero, type QuickScenario } from '../scenarios/quick';

// ---- which plan this is -----------------------------------------------------------------

const companiesOf = (profile: Profile): PrivateCompany[] => (profile as { private_equity?: PrivateCompany[] }).private_equity ?? [];

/** The plan holds private-company equity at all (counted or not): the Equity tab is shown. */
export const hasEquity = (profile: Profile): boolean => companiesOf(profile).some((c) => c.holdings.length > 0);

/** At least one company with holdings is counted in the plan. */
export const equityCounted = (profile: Profile): boolean => companiesOf(profile).some((c) => c.included && c.holdings.length > 0);

/** The companies with holdings that the plan leaves out. */
export const uncountedCompanies = (profile: Profile): PrivateCompany[] => companiesOf(profile).filter((c) => !c.included && c.holdings.length > 0);

/** True when a counted company uses the household's own outcome table instead of the stage's. */
export const usesOwnOdds = (profile: Profile): boolean => companiesOf(profile).some((c) => c.included && c.holdings.length > 0 && c.outcomes.length > 0);

/**
 * The company the engine's outcome statistics describe: the largest position at the
 * latest price, vested and unvested, counted in the plan or not; ties go to the first
 * (core `equity.rs`, `primary_company`). Null for a plan with no company.
 */
export function primaryCompany(profile: Profile): PrivateCompany | null {
  let best: PrivateCompany | null = null;
  let value = -Infinity;
  for (const c of companiesOf(profile)) {
    const v = paperValue(c).total;
    if (v > value) {
      best = c;
      value = v;
    }
  }
  return best;
}

/** True when the company the outcome statistics describe is itself counted in the plan (and holds something). */
export const primaryCounted = (profile: Profile): boolean => {
  const c = primaryCompany(profile);
  return c !== null && c.included && c.holdings.length > 0;
};

export interface CompanyScope {
  /** The company whose fate the page describes. */
  primary: string;
  primaryCounted: boolean;
  /** Every counted company with holdings: what the dollar figures total. */
  counted: string[];
}

/**
 * Which company the outcomes are about and which companies the dollars total, when
 * those are not one and the same company. Null for the ordinary plan (one company with
 * holdings, counted), where nothing needs saying.
 */
export function companyScope(profile: Profile): CompanyScope | null {
  const primary = primaryCompany(profile);
  if (!primary) return null;
  const name = (c: PrivateCompany) => c.name.trim() || 'the company';
  const counted = companiesOf(profile).filter((c) => c.included && c.holdings.length > 0);
  const isCounted = counted.includes(primary);
  if (counted.length === 0 || (counted.length === 1 && isCounted)) return null;
  return { primary: name(primary), primaryCounted: isCounted, counted: counted.map(name) };
}

export type PanelId = 'with_without' | 'outcomes' | 'pays' | 'when' | 'by_outcome' | 'cash' | 'lives' | 'decisions';

export const PANELS: PanelId[] = ['with_without', 'outcomes', 'pays', 'when', 'by_outcome', 'cash', 'lives', 'decisions'];

/**
 * The panels to show. Without `summary.equity` there is nothing to draw. When no company
 * is counted the dollar figures are all zero and are not results, so only the first two
 * panels remain: that the equity is left out, and what happens to the company anyway.
 */
export function visiblePanels(profile: Profile, summary: Summary | null): PanelId[] {
  if (!summary?.equity) return [];
  if (!equityCounted(profile)) return ['with_without', 'outcomes'];
  return decisionRows(profile).length > 0 ? PANELS : PANELS.filter((p) => p !== 'decisions');
}

// ---- counting lives ---------------------------------------------------------------------

/**
 * Shares as whole counts that add up to `of` exactly (largest remainder): a reader who
 * adds the counts in a sentence gets 100, not 99 or 101.
 */
export function countsOf(shares: number[], of = 100): number[] {
  const total = shares.reduce((t, v) => t + Math.max(0, v), 0);
  if (!(total > 0)) return shares.map(() => 0);
  const exact = shares.map((v) => (Math.max(0, v) / total) * of);
  const out = exact.map(Math.floor);
  let left = of - out.reduce((t, v) => t + v, 0);
  const order = exact.map((v, i) => ({ i, frac: v - Math.floor(v) })).sort((a, b) => b.frac - a.frac || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    out[i] = (out[i] ?? 0) + 1;
    left--;
  }
  return out;
}

/** The denominator lives are counted out of: 100, or the lives themselves when fewer were simulated. */
export const livesOf = (paths: number): number => (paths < 100 ? Math.max(1, paths) : 100);

export interface OutcomeCount {
  outcome: EquityOutcome;
  /** Lives with this outcome, of `of`. The four add up to `of`. */
  lives: number;
  /** Simulated paths with it. */
  paths: number;
  share: number;
  lo: number;
  hi: number;
}

/** Panel 2: the four outcomes as counts of 100 that add up, in the engine's order. */
export function outcomeCounts(equity: EquitySummary): { of: number; rows: OutcomeCount[] } {
  const of = livesOf(equity.paths);
  const byKey = new Map(equity.outcomes.map((o) => [o.outcome, o]));
  const counts = countsOf(
    EQUITY_OUTCOMES.map((o) => byKey.get(o)?.count ?? 0),
    of,
  );
  return {
    of,
    rows: EQUITY_OUTCOMES.map((outcome, i) => {
      const o = byKey.get(outcome);
      return { outcome, lives: counts[i] ?? 0, paths: o?.count ?? 0, share: o?.share.value ?? 0, lo: o?.share.lo ?? 0, hi: o?.share.hi ?? 0 };
    }),
  };
}

// ---- panel 3: what the equity pays ------------------------------------------------------

export type PaysView = 'net' | 'gross';
export type PaysGroup = 'loss' | 'nothing' | 'paid';

const median = (sorted: number[]): number | null => {
  if (sorted.length === 0) return null;
  const mid = (sorted.length - 1) / 2;
  return ((sorted[Math.floor(mid)] as number) + (sorted[Math.ceil(mid)] as number)) / 2;
};

/**
 * How close to zero a life's net proceeds must be to count as "nothing". A life in which
 * the company fails and nothing is exercised is not at exactly zero: the cost of shares
 * already held is a capital loss, and the tax it saves over the following years is booked
 * as tax the equity caused, with a negative sign (about $1,000 in the demo). The band is
 * $1,000 or 40% of what those shares cost (the most that loss can save), whichever is more.
 */
export function nothingBand(profile: Profile): number {
  let basis = 0;
  for (const c of companiesOf(profile)) {
    if (!c.included) continue;
    for (const h of c.holdings) if (h.kind.type === 'shares') basis += Math.max(0, h.shares) * Math.max(0, h.kind.basis_per_share);
  }
  return Math.max(1000, Math.ceil((basis * 0.4) / 500) * 500);
}

export interface PaysModel {
  view: PaysView;
  /** The dots the chart draws, one per equally likely life. */
  dots: number[];
  band: number;
  /** Which stack each value belongs to. */
  groupOf: (value: number) => PaysGroup;
  /** Dots in each group. The three add up to `dots.length`. */
  loss: number;
  nothing: number;
  paid: number;
  /** Median of the out-of-pocket dots (a negative number), and of the dots that pay. */
  lossMedian: number | null;
  paidMedian: number | null;
}

/**
 * One partition of the lives, computed from the dots the chart draws, so the sentence
 * and the picture count the same lives. After cost and tax: out of pocket (below the
 * band), nothing (within it), nets money (above it). Before: nothing (exactly zero
 * received) or something.
 */
export function paysModel(summary: Summary, profile: Profile, view: PaysView): PaysModel {
  const dist = distributionOf(summary, view === 'net' ? 'equity_net_proceeds' : 'equity_gross_proceeds');
  const dots = livesDots(dist, summary.paths)
    .filter((d): d is number => d !== null)
    .sort((a, b) => a - b);
  const band = view === 'net' ? nothingBand(profile) : 0;
  const groupOf = (v: number): PaysGroup => (v < -band ? 'loss' : v <= band ? 'nothing' : 'paid');
  const of = (g: PaysGroup) => dots.filter((d) => groupOf(d) === g);
  const loss = of('loss');
  const paid = of('paid');
  return { view, dots, band, groupOf, loss: loss.length, nothing: of('nothing').length, paid: paid.length, lossMedian: median(loss), paidMedian: median(paid) };
}

export const distributionOf = (summary: Summary, scalar: ScalarDistribution['scalar']): ScalarDistribution | undefined => summary.distributions.find((d) => d.scalar === scalar);

// ---- panel 4: when ----------------------------------------------------------------------

export interface WhenModel {
  /** Whole ages; entry i is the share of lives by the end of that age. */
  ages: number[];
  liquidity: number[];
  failed: number[];
  /** Index of the first age by whose end half of the eventual liquidity has happened; null when liquidity never reaches 1 life in 100. */
  half: number | null;
  /** The last index worth drawing: both curves have (almost) stopped rising. */
  last: number;
}

export function whenModel(equity: EquitySummary): WhenModel {
  const liq: AgeCurve = equity.liquidity_by_age;
  const fail: AgeCurve = equity.failed_by_age;
  const ages = liq.ages.length >= fail.ages.length ? liq.ages : fail.ages;
  const at = (curve: AgeCurve, age: number) => {
    const i = curve.ages.indexOf(age);
    if (i >= 0) return curve.cumulative[i] ?? 0;
    // Outside the curve's ages: nothing yet, or everything that will ever happen.
    return age < (curve.ages[0] ?? Infinity) ? 0 : (curve.cumulative[curve.cumulative.length - 1] ?? 0);
  };
  const liquidity = ages.map((a) => at(liq, a));
  const failed = ages.map((a) => at(fail, a));
  const eventual = liquidity[liquidity.length - 1] ?? 0;
  const half = eventual >= 0.01 ? liquidity.findIndex((v) => v >= eventual / 2) : -1;
  const done = (values: number[]) => {
    const end = values[values.length - 1] ?? 0;
    const i = values.findIndex((v) => v >= end - 0.002);
    return i < 0 ? values.length - 1 : i;
  };
  const last = Math.min(ages.length - 1, Math.max(done(liquidity), done(failed)) + 2);
  return { ages, liquidity, failed, half: half >= 0 ? half : null, last: Math.max(1, last) };
}

// ---- panel 5: the plan, by what happens to the company ----------------------------------

/**
 * The two classes the sentences compare: the lives where the company fails, and the best
 * outcome with enough lives to say anything (going public, else being acquired). Null
 * when the class has fewer than 30 lives.
 */
export function outcomeEnds(rows: EquityConditional[]): { low: EquityConditional | null; high: EquityConditional | null } {
  const enough = (o: EquityOutcome) => rows.find((r) => r.outcome === o && r.enough && r.success_rate.value !== null) ?? null;
  return { low: enough('failed'), high: enough('ipo') ?? enough('acquired') };
}

/** The acquired and public classes pooled, weighted by their lives: "if it is sold or listed". Null when neither has enough lives. */
export function soldOrListed(rows: EquityConditional[]): { paths: number; success: number } | null {
  const classes = rows.filter((r) => (r.outcome === 'acquired' || r.outcome === 'ipo') && r.enough && r.success_rate.value !== null);
  const paths = classes.reduce((t, r) => t + r.paths, 0);
  if (paths === 0) return null;
  return { paths, success: classes.reduce((t, r) => t + (r.success_rate.value as number) * r.paths, 0) / paths };
}

/** True when two classes' success rates cannot be told apart: the gap is under 1.96 standard errors of their difference. */
export function sameWithinNoise(a: EquityConditional, b: EquityConditional): boolean {
  const [av, bv] = [a.success_rate.value, b.success_rate.value];
  if (av === null || bv === null) return true;
  const se = Math.hypot(a.success_rate.std_error ?? 0, b.success_rate.std_error ?? 0);
  return Math.abs(av - bv) <= 1.96 * se;
}

// ---- panel 6: cash in and out -----------------------------------------------------------

export interface CashModel {
  ages: number[];
  years: number[];
  /** Mean per living household by year: proceeds, exercise cost (both positive) and paper value at year end. */
  proceeds: number[];
  exercise: number[];
  paper: number[];
  /** First index whose flows cover a whole year, and the last index drawn. */
  first: number;
  last: number;
  /** Lifetime means per life, from the per-life scalars. */
  meanIn: number | null;
  meanOut: number | null;
  /** Mean paper value at the end of the first simulated year, and that year. */
  paperNow: number;
  paperYear: number;
}

export function cashModel(summary: Summary): CashModel {
  const n = summary.axis.ages.length;
  const row = (key: 'income_equity' | 'equity_exercise' | 'equity_paper_value') => {
    const values = compositionSeries(summary, key);
    return Array.from({ length: n }, (_, i) => Math.max(0, values[i] ?? 0));
  };
  const proceeds = row('income_equity');
  const exercise = row('equity_exercise');
  const paper = row('equity_paper_value');
  const first = summary.axis.first_year_months < 12 && n > 1 ? 1 : 0;
  // Stop once nothing is left to show, and before averages rest on a handful of survivors.
  let last = Math.max(first + 1, n - 2);
  while (last > first + 1 && (summary.axis.alive[last] ?? 0) < summary.paths * 0.25) last--;
  const peak = Math.max(1, ...proceeds, ...paper);
  while (last > first + 4 && (proceeds[last] ?? 0) < peak * 0.002 && (exercise[last] ?? 0) < peak * 0.002 && (paper[last] ?? 0) < peak * 0.002) last--;
  return {
    ages: summary.axis.ages,
    years: summary.axis.years,
    proceeds,
    exercise,
    paper,
    first,
    last: Math.min(n - 1, last + 1),
    meanIn: distributionOf(summary, 'equity_gross_proceeds')?.mean ?? null,
    meanOut: distributionOf(summary, 'equity_exercise_cost')?.mean ?? null,
    paperNow: paper[0] ?? 0,
    paperYear: summary.axis.years[0] ?? 0,
  };
}

// ---- panel 7: lives to read -------------------------------------------------------------

export const EQUITY_PICKS = [
  { label: 'equity_failed', button: 'A life where it fails' },
  { label: 'equity_paid_median', button: 'A life with a middling payout' },
  { label: 'equity_paid_p90', button: 'A life with a large payout' },
] as const;

export function equityLives(summary: Summary): { label: string; button: string; path: number; value: number | null }[] {
  return EQUITY_PICKS.flatMap((p) => {
    const pick = summary.picks.find((x) => x.label === p.label);
    return pick ? [{ label: p.label, button: p.button, path: pick.path_index, value: pick.value }] : [];
  });
}

// ---- side runs --------------------------------------------------------------------------

/**
 * Lives for a side run: as many as the plan's own run, up to 10,000. At the default run
 * size this is every life the plan was simulated on, so the standing comparison gives the
 * same answer as the "Count the equity at zero" scenario in Scenarios (2,000 lives could
 * call a difference real that 10,000 show to be noise).
 */
export const sidePaths = (total: number): number => Math.min(10_000, Math.max(500, Math.floor(total) || 2000));

/** The patch of the standing comparison: the same one the "Count the equity at zero" what-if makes. */
export const zeroPatch = (profile: Profile) => equityZero(profile)?.patch ?? null;

export const typicalOddsScenario = (profile: Profile): QuickScenario | null => equityTypicalOdds(profile);
export const zeroScenario = (profile: Profile): QuickScenario | null => equityZero(profile);

/** Panel 8: one row per decision that would change something in this plan. */
export const decisionRows = (profile: Profile): QuickScenario[] => equityDecisions(profile);

export const successOf = (c: Comparison): OutcomeComparison | null => c.outcomes.find((o) => o.outcome === 'success') ?? null;

export interface WithWithout {
  /** Lives paired. */
  paths: number;
  /** Plan holds, as modelled: the live run's figure, so the page never shows two answers to one question. */
  modelled: { value: number; lo: number; hi: number };
  /** Plan holds if the equity pays nothing: the live figure moved by the paired difference, with the side run's own interval. */
  zero: { value: number; lo: number; hi: number };
  /** As modelled minus at zero, in shares of lives, with the paired 95% interval. */
  difference: { value: number; lo: number; hi: number };
  withinNoise: boolean;
  /** Lives in which the plan holds only with the equity, and only without it. */
  rescued: number;
  harmed: number;
  /**
   * Of `rescued`: lives in which the company failed, as a share of all paired lives. No
   * payout made the difference there. Null when the comparison has no breakdown by outcome.
   */
  rescuedFailed: number | null;
  /** Median age at which work becomes optional, with the equity minus without it, in years. Null when either is "never". */
  fiShift: number | null;
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/**
 * Panel 1. `comparison` is at-zero minus as-modelled (B minus A) on the same lives; the
 * view states it the other way round, as what the equity adds.
 */
export function withWithout(live: Summary, comparison: Comparison): WithWithout | null {
  const s = successOf(comparison);
  if (!s || comparison.paths <= 0) return null;
  const a = live.headline.success_rate;
  const zero = clamp01(a.value + s.difference.value);
  const failed = comparison.by_equity_outcome?.find((o) => o.outcome === 'failed');
  const shift = fiAgeShift(comparison);
  return {
    paths: comparison.paths,
    modelled: { value: a.value, lo: a.lo, hi: a.hi },
    zero: { value: zero, lo: clamp01(zero - (s.b.value - s.b.lo)), hi: clamp01(zero + (s.b.hi - s.b.value)) },
    difference: { value: -s.difference.value, lo: -s.difference.hi, hi: -s.difference.lo },
    withinNoise: s.within_noise,
    rescued: s.a_only / comparison.paths,
    harmed: s.b_only / comparison.paths,
    rescuedFailed: failed ? failed.success.a_only / comparison.paths : null,
    // The comparison is at-zero minus as-modelled; the view states it the other way round.
    fiShift: shift === null ? null : -shift,
  };
}

/**
 * The change in the median age at which work becomes optional, B minus A, in years.
 * Null when it is "never" on either side.
 */
export function fiAgeShift(comparison: Comparison): number | null {
  const m = comparison.headline.find((h) => h.metric === 'median_fi_age');
  return m && m.a !== null && m.b !== null ? m.b - m.a : null;
}

/** Median of the per-life difference in a paired scalar, by the comparison's own quantile levels. */
export function medianDelta(levels: readonly number[], quantiles: readonly (number | null)[]): number | null {
  const i = levels.findIndex((l) => Math.abs(l - 0.5) < 1e-9);
  return i < 0 ? null : (quantiles[i] ?? null);
}

export interface DecisionRow {
  outcome: EquityOutcome;
  paths: number;
  /** False under 30 pairs: too few lives to say. */
  enough: boolean;
  /** Median change in net worth at the end, with the decision minus without. */
  wealth: number | null;
  /** Mean change in net worth at the end with its paired 95% interval. A decision that acts in few lives moves the mean and not the median. */
  mean: { value: number; lo: number; hi: number; withinNoise: boolean } | null;
  /** Change in the share of these lives in which the plan holds, with its paired interval. */
  holds: { value: number; lo: number; hi: number; withinNoise: boolean } | null;
}

export const MIN_PAIRS = 30;

function meanOf(r: PairedByOutcome): DecisionRow['mean'] {
  const m = r.terminal_net_worth.mean_delta;
  if (r.paths <= 0 || m.value === null || m.lo === null || m.hi === null) return null;
  return { value: m.value, lo: m.lo, hi: m.hi, withinNoise: m.lo <= 0 && m.hi >= 0 };
}

/** The by-outcome table of a paired comparison (the decisions panel, and Scenarios). Empty when the plan has no company. */
export function decisionTable(comparison: Comparison): DecisionRow[] {
  const rows: PairedByOutcome[] = comparison.by_equity_outcome ?? [];
  return rows.map((r) => ({
    outcome: r.outcome,
    paths: r.paths,
    enough: r.paths >= MIN_PAIRS,
    wealth: r.paths > 0 ? medianDelta(comparison.quantile_levels, r.terminal_net_worth.quantiles) : null,
    mean: meanOf(r),
    holds: r.paths > 0 ? { value: r.success.difference.value, lo: r.success.difference.lo, hi: r.success.difference.hi, withinNoise: r.success.within_noise } : null,
  }));
}

/** The rows the decision sentence compares: the company failing, and its best outcome with at least 30 pairs. */
export function decisionEnds(rows: DecisionRow[]): { low: DecisionRow | null; high: DecisionRow | null } {
  const enough = (o: EquityOutcome) => rows.find((r) => r.outcome === o && r.enough && r.wealth !== null) ?? null;
  return { low: enough('failed'), high: enough('ipo') ?? enough('acquired') };
}

// ---- what "exercise now" actually exercises ---------------------------------------------

export interface FirstExercise {
  /** Options exercised in the first simulated month, over every company. */
  options: number;
  /** The strike paid for them. */
  cost: number;
}

/**
 * What one traced life exercises in its first month. Exercising "now" is capped by what
 * savings above the emergency fund can pay (strike and tax), and that cap is the same in
 * every life because nothing random has happened yet: so one life's first month says how
 * much of the wished-for exercise the plan really makes.
 */
export function firstExercise(trace: PathTrace): FirstExercise {
  let options = 0;
  let cost = 0;
  for (const e of trace.events) {
    if (e.kind !== 'options_exercised' || e.month !== trace.start) continue;
    const m = /^Exercised ([\d,]+) /.exec(e.label);
    options += m ? Number((m[1] as string).replace(/,/g, '')) : 0;
    cost += e.amount ?? 0;
  }
  return { options, cost };
}

/** Vested options of the companies a patch switches to exercising now, and the strike on all of them. */
export function vestedOptions(companies: PrivateCompany[]): number {
  let n = 0;
  for (const c of companies) for (const h of c.holdings) if (h.kind.type === 'iso' || h.kind.type === 'nso') n += Math.min(Math.max(h.vested, 0), Math.max(h.shares, 0));
  return n;
}
