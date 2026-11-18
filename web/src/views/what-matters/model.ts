// The arithmetic of the What matters view, kept free of React and of the engine so it
// can be tested: which measures exist, how a paired comparison becomes one effect with
// its interval, which levers and variables a plan offers, how a ranking and a
// bisection proceed. Nothing here computes a simulation statistic the engine already
// gives; it selects, combines and words them.
import { budgetTotals } from '@/editor/labels';
import type { Comparison, Headline, OutcomeComparison, ScalarComparison } from '@/engine/output-types';
import type { Lever, PatchOp, Profile } from '@/engine/types';
import { moneyCompact } from '@/ui/format';
import { age as formatAge, ofHundred } from '@/ui/charts/format';

// ---- measures -----------------------------------------------------------------------

export const METRIC_IDS = ['holds', 'short', 'fi_age', 'wealth', 'spending'] as const;
export type MetricId = (typeof METRIC_IDS)[number];

export interface MetricDef {
  id: MetricId;
  /** In the measure switch. */
  label: string;
  /** Names the quantity on an axis, said once. */
  axis: string;
  unit: 'share' | 'age' | 'dollars' | 'ratio';
  /** +1 when more is better for the household, -1 when less is. */
  better: 1 | -1;
  /** How the paired change is measured, for captions. */
  paired: string;
}

export const METRICS: Record<MetricId, MetricDef> = {
  holds: { id: 'holds', label: 'Plan holds', axis: 'Lives in 100 in which the plan holds', unit: 'share', better: 1, paired: 'the change in the share of lives in which the plan holds' },
  short: { id: 'short', label: 'Runs short', axis: 'Lives in 100 in which money runs short', unit: 'share', better: -1, paired: 'the change in the share of lives that run short of money' },
  fi_age: { id: 'fi_age', label: 'Work optional', axis: 'Age work becomes optional, middle life', unit: 'age', better: -1, paired: 'the middle (median) per-life change in the age work becomes optional, over lives in which it happens either way' },
  wealth: { id: 'wealth', label: 'Wealth at the end', axis: "Net worth at the end, middle life, today's dollars", unit: 'dollars', better: 1, paired: 'the middle (median) per-life change in net worth at the end' },
  spending: { id: 'spending', label: 'Spending delivered', axis: 'Retirement spending delivered, % of plan', unit: 'ratio', better: 1, paired: 'the average per-life change in retirement spending delivered against the plan' },
};

export interface Level {
  value: number | null;
  lo: number | null;
  hi: number | null;
}

const finite = (v: number | null | undefined): number | null => (v === null || v === undefined || !Number.isFinite(v) ? null : v);

/** A measure's level in one run, with the 95% sampling interval the engine gives for it. */
export function readLevel(h: Headline, metric: MetricId): Level {
  switch (metric) {
    case 'holds':
      return { value: finite(h.success_rate.value), lo: finite(h.success_rate.lo), hi: finite(h.success_rate.hi) };
    case 'short':
      return { value: finite(h.depletion_rate.value), lo: finite(h.depletion_rate.lo), hi: finite(h.depletion_rate.hi) };
    case 'fi_age':
      return { value: finite(h.fi_age.p50), lo: finite(h.fi_age.p50_lo), hi: finite(h.fi_age.p50_hi) };
    case 'wealth':
      return { value: finite(h.terminal_net_worth.p50), lo: finite(h.terminal_net_worth.p50_lo), hi: finite(h.terminal_net_worth.p50_hi) };
    case 'spending':
      return { value: finite(h.delivered_spending_ratio.value), lo: finite(h.delivered_spending_ratio.lo), hi: finite(h.delivered_spending_ratio.hi) };
  }
}

/** The five levels of a run: all a heat-map cell or a solver probe keeps of its Summary. */
export type Levels = Record<MetricId, Level>;
export const readLevels = (h: Headline): Levels => Object.fromEntries(METRIC_IDS.map((m) => [m, readLevel(h, m)])) as Levels;

// ---- paired effects -----------------------------------------------------------------

/** The parts of a `Comparison` this view reads (a full one carries 27 scalars of 100 dots each). */
export interface PairedLite {
  paths: number;
  outcomes: OutcomeComparison[];
  scalars: ScalarComparison[];
}

const KEPT_OUTCOMES = ['success', 'depleted'];
const KEPT_SCALARS = ['fi_age', 'terminal_net_worth', 'delivered_spending_ratio'];

export function trimComparison(c: Comparison): PairedLite {
  return { paths: c.paths, outcomes: c.outcomes.filter((o) => KEPT_OUTCOMES.includes(o.outcome)), scalars: c.scalars.filter((s) => KEPT_SCALARS.includes(s.scalar)) };
}

/** A quantile of a 100-dot quantile function (dot i sits at (i + 0.5) / 100), linear between dots. */
export function quantileOfDots(dots: readonly number[], p: number): number | null {
  if (dots.length === 0) return null;
  const at = Math.min(dots.length - 1, Math.max(0, p * dots.length - 0.5));
  const i = Math.floor(at);
  const a = dots[i] as number;
  const b = dots[Math.min(dots.length - 1, i + 1)] as number;
  return a + (b - a) * (at - i);
}

/**
 * Distribution-free 95% interval for the median of `n` paired differences, read off
 * their quantile function: the order statistics at ranks n/2 -/+ 1.96 * sqrt(n) / 2.
 */
export function medianInterval(dots: readonly number[], n: number): { median: number; lo: number; hi: number } | null {
  const median = quantileOfDots(dots, 0.5);
  if (median === null || n <= 0) return null;
  const half = Math.min(0.5, 0.98 / Math.sqrt(n));
  return { median, lo: quantileOfDots(dots, 0.5 - half) as number, hi: quantileOfDots(dots, 0.5 + half) as number };
}

/**
 * 'effect': the paired change is distinguishable from zero. 'noise': its 95% interval
 * contains zero. 'none': no simulated life changes (or, for a middle-life measure, the
 * middle life does not). 'unknown': the measure does
 * not exist for this pair (nobody retires, the setting is not a valid plan).
 */
export type EffectState = 'effect' | 'noise' | 'none' | 'unknown';

export interface Effect {
  state: EffectState;
  /** Paired change, variant minus plan, in the measure's unit. */
  delta: number | null;
  lo: number | null;
  hi: number | null;
  /** The plan's level and the variant's level on these lives. */
  base: number | null;
  value: number | null;
  /** Lives the paired statistic rests on. */
  n: number;
  /** Lives that flip: `gained` have the outcome only with the change, `lost` only without it. */
  gained: number;
  lost: number;
}

const UNKNOWN: Effect = { state: 'unknown', delta: null, lo: null, hi: null, base: null, value: null, n: 0, gained: 0, lost: 0 };

/** One lever setting's effect on one measure, from core's paired comparison (variant minus plan). */
export function effectOf(metric: MetricId, paired: PairedLite, base: Headline, variant: Headline): Effect {
  const a = readLevel(base, metric).value;
  const b = readLevel(variant, metric).value;
  if (metric === 'holds' || metric === 'short') {
    const o = paired.outcomes.find((x) => x.outcome === (metric === 'holds' ? 'success' : 'depleted'));
    if (!o) return UNKNOWN;
    const state: EffectState = o.a_only + o.b_only === 0 ? 'none' : o.within_noise ? 'noise' : 'effect';
    return { state, delta: o.difference.value, lo: o.difference.lo, hi: o.difference.hi, base: o.a.value, value: o.b.value, n: paired.paths, gained: o.b_only, lost: o.a_only };
  }
  const s = paired.scalars.find((x) => x.scalar === (metric === 'fi_age' ? 'fi_age' : metric === 'wealth' ? 'terminal_net_worth' : 'delivered_spending_ratio'));
  if (!s || s.both === 0) return { ...UNKNOWN, base: a, value: b, gained: s?.b_only ?? 0, lost: s?.a_only ?? 0 };
  const dots = s.dots.filter((d): d is number => d !== null);
  const flips = { n: s.both, gained: s.b_only, lost: s.a_only };
  const unchanged = s.equal !== null && s.equal >= 1;
  if (metric === 'spending') {
    const m = s.mean_delta;
    if (m.value === null || m.lo === null || m.hi === null) return { ...UNKNOWN, base: a, value: b, ...flips };
    return { state: unchanged ? 'none' : s.within_noise || (m.lo <= 0 && m.hi >= 0) ? 'noise' : 'effect', delta: m.value, lo: m.lo, hi: m.hi, base: a, value: b, ...flips };
  }
  const ci = medianInterval(dots, s.both);
  if (!ci) return { ...UNKNOWN, base: a, value: b, ...flips };
  // An interval of exactly zero: most lives do not move at all, so the middle life is unchanged.
  const state: EffectState = unchanged || (ci.lo === 0 && ci.hi === 0) ? 'none' : ci.lo <= 0 && ci.hi >= 0 ? 'noise' : 'effect';
  return { state, delta: ci.median, lo: ci.lo, hi: ci.hi, base: a, value: b, ...flips };
}

// ---- levers -------------------------------------------------------------------------

export type LeverGroup = 'control' | 'world';

export interface LeverSide {
  /** `<lever id>:<low|high>`. */
  key: string;
  leverId: string;
  side: 'low' | 'high';
  label: string;
  ops: PatchOp[];
}

export interface LeverDef {
  id: string;
  label: string;
  group: LeverGroup;
  sides: LeverSide[];
}

/** Engine levers that are assumptions about the world, not decisions. */
const WORLD = new Set(['stock_returns', 'inflation', 'social_security', 'longevity', 'healthcare_inflation', 'long_term_care', 'job_loss']);

export const groupOf = (leverId: string): LeverGroup => (WORLD.has(leverId) ? 'world' : 'control');

export const GROUP_LABEL: Record<LeverGroup, string> = { control: 'What you control', world: 'What the world decides' };

const GROWTH_DEFENSIVE = new Set(['bonds', 'cash']);

/** Share of the target mix in growth assets (everything but bonds and cash). */
export function equityShare(profile: Profile): number | null {
  const entries = Object.entries(profile.portfolio.target).filter(([, w]) => (w ?? 0) > 0) as [string, number][];
  const total = entries.reduce((n, [, w]) => n + w, 0);
  if (!(total > 0)) return null;
  return entries.filter(([k]) => !GROWTH_DEFENSIVE.has(k)).reduce((n, [, w]) => n + w, 0) / total;
}

/** The target mix moved to a growth share, keeping the proportions inside each group. */
export function allocationOps(profile: Profile, share: number): PatchOp[] | null {
  const entries = Object.entries(profile.portfolio.target).filter(([, w]) => (w ?? 0) > 0) as [string, number][];
  const total = entries.reduce((n, [, w]) => n + w, 0);
  if (!(total > 0)) return null;
  const g = Math.min(1, Math.max(0, share));
  const growth = entries.filter(([k]) => !GROWTH_DEFENSIVE.has(k));
  const defensive = entries.filter(([k]) => GROWTH_DEFENSIVE.has(k));
  const gSum = growth.reduce((n, [, w]) => n + w, 0);
  const dSum = defensive.reduce((n, [, w]) => n + w, 0);
  const out: Record<string, number> = {};
  const put = (k: string, w: number) => {
    const r = Math.round(w * 10) / 10;
    if (r > 0) out[k] = r;
  };
  if (gSum > 0) growth.forEach(([k, w]) => put(k, (w / gSum) * g * 100));
  else put('us_stocks', g * 100);
  if (dSum > 0) defensive.forEach(([k, w]) => put(k, (w / dSum) * (1 - g) * 100));
  else put('bonds', (1 - g) * 100);
  return [{ op: 'set', path: '/portfolio/target', value: out }];
}

/** The first person's age at the plan's start, in years. */
export function ageNow(profile: Profile): number {
  const [by = 1990, bm = 1] = (profile.household.people[0]?.birth ?? '').split('-').map(Number);
  const [sy = new Date().getFullYear(), sm = 1] = (profile.start ?? '').split('-').map(Number);
  return sy - by + (sm - bm) / 12;
}

/** Decisions the engine's own lever list leaves out, where this plan has them. */
function extraLevers(profile: Profile, have: Set<string>): Omit<LeverDef, 'group'>[] {
  const out: Omit<LeverDef, 'group'>[] = [];
  const side = (leverId: string, which: 'low' | 'high', label: string, ops: PatchOp[] | null): LeverSide[] => (ops && ops.length > 0 ? [{ key: `${leverId}:${which}`, leverId, side: which, label, ops }] : []);

  const trigger = profile.retirement.trigger;
  if (trigger.type === 'at_age' && !have.has('retirement_age')) {
    const earlier = trigger.age - 2 > ageNow(profile) + 0.5;
    out.push({
      id: 'retirement_age',
      label: 'Retirement age',
      sides: [
        ...side('retirement_age', 'low', '2 years earlier', earlier ? [{ op: 'set', path: '/retirement/trigger/age', value: trigger.age - 2 }] : null),
        ...side('retirement_age', 'high', '2 years later', [{ op: 'set', path: '/retirement/trigger/age', value: trigger.age + 2 }]),
      ],
    });
  }

  const g = equityShare(profile);
  if (g !== null && !have.has('allocation') && !have.has('stock_allocation')) {
    out.push({
      id: 'allocation',
      label: 'Stocks in the target mix',
      sides: [
        ...side('allocation', 'low', '10 points fewer', g >= 0.1 ? allocationOps(profile, g - 0.1) : null),
        ...side('allocation', 'high', '10 points more', g <= 0.9 ? allocationOps(profile, g + 0.1) : null),
      ],
    });
  }

  const people = profile.household.people;
  if (people.length > 0 && !have.has('claim_age') && !have.has('social_security_claim')) {
    const claim = (ageTo: number) => (people.every((p) => p.social_security.claim_age === ageTo) ? null : people.map((_, i): PatchOp => ({ op: 'set', path: `/household/people/${i}/social_security/claim_age`, value: ageTo })));
    out.push({
      id: 'claim_age',
      label: 'Social Security claiming age',
      sides: [...side('claim_age', 'low', people.length > 1 ? 'both claim at 62' : 'claim at 62', claim(62)), ...side('claim_age', 'high', people.length > 1 ? 'both claim at 70' : 'claim at 70', claim(70))],
    });
  }
  return out.filter((l) => l.sides.length > 0);
}

/** The levers tested for a plan: the engine's, then the view's own, each marked as a decision or an assumption. */
export function buildLevers(profile: Profile, engineLevers: Lever[]): LeverDef[] {
  const have = new Set(engineLevers.map((l) => l.id));
  const fromEngine: LeverDef[] = engineLevers.map((l) => ({
    id: l.id,
    label: l.label,
    group: WORLD.has(l.id) ? 'world' : 'control',
    sides: [
      { key: `${l.id}:low`, leverId: l.id, side: 'low' as const, label: l.low_label, ops: l.low },
      { key: `${l.id}:high`, leverId: l.id, side: 'high' as const, label: l.high_label, ops: l.high },
    ].filter((s) => s.ops.length > 0),
  }));
  return [...fromEngine, ...extraLevers(profile, have).map((l) => ({ ...l, group: 'control' as const }))].filter((l) => l.sides.length > 0);
}

/** A setting's label without its parenthetical gloss, for narrow layouts. */
export const shortLabel = (label: string) => label.replace(/\s*\(.*\)\s*$/, '');

export interface RankedLever {
  lever: LeverDef;
  sides: { side: LeverSide; effect: Effect }[];
  /** Largest absolute paired change across the settings (0 when none is known). */
  swing: number;
  /** True when no setting is distinguishable from zero. */
  quiet: boolean;
}

/** Levers of one group, largest swing first; levers that cannot be told from zero sink below those that can. */
export function rankLevers(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, group: LeverGroup): RankedLever[] {
  return levers
    .filter((l) => l.group === group)
    .map((lever) => {
      const sides = lever.sides.map((side) => ({ side, effect: effects.get(side.key) ?? UNKNOWN }));
      const swing = Math.max(0, ...sides.map((s) => Math.abs(s.effect.delta ?? 0)));
      return { lever, sides, swing, quiet: !sides.some((s) => s.effect.state === 'effect') };
    })
    .sort((a, b) => Number(a.quiet) - Number(b.quiet) || b.swing - a.swing);
}

export interface Win {
  lever: LeverDef;
  side: LeverSide;
  effect: Effect;
}

/** The controllable settings that most improve the measure, one per lever, distinguishable from zero only. */
export function biggestWins(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, metric: MetricId, limit = 3): Win[] {
  const better = METRICS[metric].better;
  const wins: Win[] = [];
  for (const lever of levers) {
    if (lever.group !== 'control') continue;
    let best: Win | null = null;
    for (const side of lever.sides) {
      const effect = effects.get(side.key);
      if (!effect || effect.state !== 'effect' || effect.delta === null || effect.delta * better <= 0) continue;
      if (!best || effect.delta * better > (best.effect.delta as number) * better) best = { lever, side, effect };
    }
    if (best) wins.push(best);
  }
  return wins.sort((a, b) => (b.effect.delta as number) * better - (a.effect.delta as number) * better).slice(0, limit);
}

/** The assumption whose worse setting costs the most, distinguishable from zero. */
export function biggestRisk(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, metric: MetricId): Win | null {
  const better = METRICS[metric].better;
  let worst: Win | null = null;
  for (const lever of levers) {
    if (lever.group !== 'world') continue;
    for (const side of lever.sides) {
      const effect = effects.get(side.key);
      if (!effect || effect.state !== 'effect' || effect.delta === null || effect.delta * better >= 0) continue;
      if (!worst || effect.delta * better < (worst.effect.delta as number) * better) worst = { lever, side, effect };
    }
  }
  return worst;
}

// ---- formatting ---------------------------------------------------------------------

export type Unit = MetricDef['unit'];
/** Format functions take a measure of this view or, for other callers (the assistant's rankings), a bare unit. */
const unitOf = (m: MetricId | Unit): Unit => (m in METRICS ? METRICS[m as MetricId].unit : (m as Unit));

const trim1 = (v: number) => (Math.abs(v) >= 9.95 ? String(Math.round(v)) : String(Math.round(v * 10) / 10));
const sign = (v: number) => (v > 0 ? '+' : v < 0 ? '−' : '');

/** A measure's level in words short enough for a label: "83 of 100", "51.8", "$2.4M", "128%". */
export function formatLevel(metric: MetricId | Unit, v: number | null): string {
  if (v === null) return 'n/a';
  switch (unitOf(metric)) {
    case 'share':
      return ofHundred(v).text;
    case 'age':
      return formatAge(v);
    case 'dollars':
      return moneyCompact(v);
    case 'ratio':
      return `${Math.round(v * 100)}%`;
  }
}

/** A 95% interval of a level: "91 to 95 of 100", "51.2 to 52.4", "$2.1M to $2.6M". */
export function formatRange(metric: MetricId | Unit, lo: number | null, hi: number | null): string {
  if (lo === null || hi === null) return 'n/a';
  if (unitOf(metric) === 'share') {
    const a = ofHundred(lo);
    const b = ofHundred(hi);
    return a.of === b.of ? `${a.n} to ${b.text}` : `${a.text} to ${b.text}`;
  }
  return `${formatLevel(metric, lo)} to ${formatLevel(metric, hi)}`;
}

/** Magnitude of a change, unsigned: "6.3", "1.2 years", "$310k", "9 points". */
export function formatMagnitude(metric: MetricId | Unit, d: number): string {
  const a = Math.abs(d);
  switch (unitOf(metric)) {
    case 'share':
      return trim1(a * 100);
    case 'age':
      return `${trim1(a)} ${Math.abs(a - 1) < 0.05 ? 'year' : 'years'}`;
    case 'dollars':
      return moneyCompact(Math.abs(a) >= 1000 ? a : Math.round(a));
    case 'ratio':
      return `${trim1(a * 100)} ${Math.abs(a * 100 - 1) < 0.05 ? 'point' : 'points'}`;
  }
}

/** A signed change for a bar label: "+6.3", "−1.2 yr", "+$310k", "+9 pts". */
export function formatDelta(metric: MetricId | Unit, d: number | null): string {
  if (d === null) return 'n/a';
  const unit = unitOf(metric);
  const a = Math.abs(d);
  const zero = unit === 'share' || unit === 'ratio' ? a * 100 < 0.05 : unit === 'age' ? a < 0.05 : a < 500;
  if (zero) return unit === 'dollars' ? '$0' : '0';
  const s = sign(d);
  switch (unit) {
    case 'share':
      return `${s}${trim1(a * 100)}`;
    case 'age':
      return `${s}${trim1(a)} yr`;
    case 'dollars':
      return `${s}${moneyCompact(a)}`;
    case 'ratio':
      return `${s}${trim1(a * 100)} pts`;
  }
}

// ---- variables (trade-off map, solver) ------------------------------------------------

export const VAR_IDS = ['spending', 'withdrawal_rate', 'retirement_age', 'salary', 'stocks', 'claim_age'] as const;
export type VarId = (typeof VAR_IDS)[number];

export interface VarDef {
  id: VarId;
  /** In a select: "Annual spending". */
  label: string;
  /** In a tooltip row or a scenario name: "Spending". */
  short: string;
  /** Axis title, unit included. */
  axis: string;
  /** The plan's value, when it has one on this variable. */
  current: number | null;
  /** Grid values, ascending. */
  values: number[];
  /** Search range and resolution for the solver. Null when the solver does not offer it. */
  bounds: [number, number] | null;
  tolerance: number;
  round(v: number): number;
  ops(v: number): PatchOp[];
  /** Tick label: "$105k", "3.5%", "55". */
  format(v: number): string;
  /** In a sentence: "spending $105k a year". */
  phrase(v: number): string;
}

export const GRID_STEPS = 9;
const steps = (lo: number, step: number, n = GRID_STEPS) => Array.from({ length: n }, (_, i) => lo + step * i);
const round = (v: number, step: number) => Math.round(v / step) * step;

/** The quantities a plan can be mapped and solved over. Order is the order offered. */
export function variablesFor(profile: Profile): VarDef[] {
  const out: VarDef[] = [];
  const trigger = profile.retirement.trigger;
  const now = budgetTotals(profile).now;

  if (trigger.type === 'when_funded') {
    const pct = (v: number) => `${trim1Fixed(v * 100)}%`;
    out.push({
      id: 'withdrawal_rate',
      label: 'Withdrawal rate that triggers retirement',
      short: 'Trigger rate',
      axis: 'Retire when savings cover spending at this withdrawal rate',
      current: trigger.withdrawal_rate,
      values: steps(0.02, 0.005).map((v) => round(v, 0.0005)),
      bounds: [0.02, 0.07],
      tolerance: 0.0005,
      round: (v) => round(v, 0.0005),
      ops: (v) => [{ op: 'set', path: '/retirement/trigger/withdrawal_rate', value: Math.round(v * 1e5) / 1e5 }],
      format: pct,
      phrase: (v) => `retiring once savings cover spending at a ${pct(v)} withdrawal rate`,
    });
  }

  {
    const from = Math.max(40, Math.ceil(ageNow(profile)) + 1);
    const lo = Math.min(from, 62);
    const step = Math.max(1, Math.round((70 - lo) / (GRID_STEPS - 1)));
    let first = lo;
    if (trigger.type === 'at_age') {
      // Put the plan's own age on a grid node.
      const k = Math.min(GRID_STEPS - 1, Math.max(0, Math.round((trigger.age - lo) / step)));
      first = Math.max(Math.ceil(ageNow(profile)) + 1, Math.round(trigger.age) - k * step);
    }
    out.push({
      id: 'retirement_age',
      label: 'Retirement age (fixed)',
      short: 'Retire at',
      axis: `Retire at this age${profile.household.people[0]?.name ? ` (${profile.household.people[0].name})` : ''}`,
      current: trigger.type === 'at_age' ? trigger.age : null,
      values: steps(first, step),
      bounds: [lo, Math.max(70, first + step * (GRID_STEPS - 1))],
      tolerance: 0.25,
      round: (v) => round(v, 0.25),
      ops: (v) => [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: round(v, 0.25) } }],
      format: (v) => formatAge(v, 2),
      phrase: (v) => `retiring at ${formatAge(v, 2)}`,
    });
  }

  if (now > 0) {
    const unit = now >= 40_000 ? 1000 : 100;
    out.push({
      id: 'spending',
      label: 'Annual spending',
      short: 'Spending',
      axis: "Budgeted spending a year, today's dollars (every budget line scaled together)",
      current: Math.round(now),
      values: steps(0.7, 0.075).map((f) => (Math.abs(f - 1) < 1e-9 ? Math.round(now) : round(now * f, unit))),
      bounds: [round(now * 0.5, 100), round(now * 1.5, 100)],
      tolerance: Math.max(100, round(now * 0.004, 100)),
      round: (v) => round(v, 100),
      ops: (v) => profile.spending.items.map((item, i): PatchOp => ({ op: 'set', path: `/spending/items/${i}/amount`, value: Math.round(((item.amount * v) / now) * 100) / 100 })),
      format: (v) => moneyCompact(v),
      phrase: (v) => `a budget of ${moneyCompact(v)} a year`,
    });
  }

  const jobs = profile.incomes.flatMap((income, i) => (income.kind.type === 'job' ? [{ i, salary: income.kind.salary }] : []));
  if (jobs.length > 0) {
    const rel = (v: number) => (Math.abs(v - 1) < 0.005 ? 'today' : `${v > 1 ? '+' : '−'}${Math.round(Math.abs(v - 1) * 100)}%`);
    out.push({
      id: 'salary',
      label: jobs.length > 1 ? 'Salaries' : 'Salary',
      short: jobs.length > 1 ? 'Salaries' : 'Salary',
      axis: `${jobs.length > 1 ? 'Salaries' : 'Salary'}, relative to today`,
      current: 1,
      values: steps(0.6, 0.1).map((v) => round(v, 0.01)),
      bounds: [0.5, 1.5],
      tolerance: 0.01,
      round: (v) => round(v, 0.01),
      ops: (v) => jobs.map(({ i, salary }): PatchOp => ({ op: 'set', path: `/incomes/${i}/kind/salary`, value: Math.round(salary * v) })),
      format: rel,
      phrase: (v) => (Math.abs(v - 1) < 0.005 ? 'today’s salaries' : `salaries ${Math.round(Math.abs(v - 1) * 100)}% ${v > 1 ? 'higher' : 'lower'}`),
    });
  }

  const g = equityShare(profile);
  if (g !== null) {
    out.push({
      id: 'stocks',
      label: 'Stocks in the target mix',
      short: 'Stocks',
      axis: 'Share of the target mix in stocks and other growth assets',
      current: Math.round(g * 100) / 100,
      values: steps(0.2, 0.1).map((v) => round(v, 0.01)),
      bounds: null,
      tolerance: 0.01,
      round: (v) => round(v, 0.01),
      ops: (v) => allocationOps(profile, v) ?? [],
      format: (v) => `${Math.round(v * 100)}%`,
      phrase: (v) => `${Math.round(v * 100)}% in stocks`,
    });
  }

  const people = profile.household.people;
  if (people.length > 0) {
    const claims = people.map((p) => p.social_security.claim_age);
    out.push({
      id: 'claim_age',
      label: 'Social Security claiming age',
      short: 'Claim at',
      axis: `Age ${people.length > 1 ? 'both claim' : 'of claiming'} Social Security`,
      current: claims.every((c) => c === claims[0]) ? (claims[0] ?? null) : null,
      values: steps(62, 1),
      bounds: null,
      tolerance: 1,
      round: (v) => Math.round(v),
      ops: (v) => people.map((_, i): PatchOp => ({ op: 'set', path: `/household/people/${i}/social_security/claim_age`, value: Math.round(v) })),
      format: (v) => String(Math.round(v)),
      phrase: (v) => `claiming Social Security at ${Math.round(v)}`,
    });
  }
  return out;
}

const trim1Fixed = (v: number) => String(Math.round(v * 100) / 100);

/** Default axes of the trade-off map: when to retire against how much to spend. */
export function defaultAxes(vars: VarDef[]): { x: VarId; y: VarId } | null {
  const x = vars.find((v) => v.id === 'withdrawal_rate') ?? vars.find((v) => v.id === 'retirement_age');
  const y = vars.find((v) => v.id === 'spending') ?? vars.find((v) => v.id !== x?.id);
  return x && y && x.id !== y.id ? { x: x.id, y: y.id } : null;
}

/** Where a value falls among grid values, as a fractional index (for marking the plan between nodes). Null outside the grid. */
export function gridPosition(values: readonly number[], v: number | null): number | null {
  if (v === null || values.length < 2) return null;
  const first = values[0] as number;
  const last = values[values.length - 1] as number;
  const eps = Math.abs(last - first) * 1e-6;
  if (v < first - eps || v > last + eps) return null;
  for (let i = 1; i < values.length; i++) {
    const a = values[i - 1] as number;
    const b = values[i] as number;
    if (v <= b + eps) return i - 1 + Math.min(1, Math.max(0, (v - a) / (b - a)));
  }
  return values.length - 1;
}

/** Contour levels for a measure: the customary probabilities, or three round values inside the data. */
export function contourLevels(metric: MetricId, lo: number, hi: number): number[] {
  if (metric === 'holds') return [0.75, 0.9, 0.95];
  if (metric === 'short') return [0.05, 0.1, 0.25];
  if (!(hi > lo)) return [];
  const span = hi - lo;
  const raw = span / 4;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 2.5, 5, 10].find((m) => m * mag >= raw) ?? 10) * mag;
  const out: number[] = [];
  for (let v = Math.ceil((lo + span * 0.08) / step) * step; v <= hi - span * 0.08; v += step) out.push(Math.round(v * 1e6) / 1e6);
  return out.slice(0, 4);
}

/** A contour's label: "90" (lives in 100, like the cells), "age 55", "$2M", "110%". */
export function formatContour(metric: MetricId, v: number): string {
  switch (METRICS[metric].unit) {
    case 'share':
      return trim1(v * 100);
    case 'age':
      return `age ${formatAge(v)}`;
    case 'dollars':
      return moneyCompact(v);
    case 'ratio':
      return `${Math.round(v * 100)}%`;
  }
}

/** A cell's value, compact enough to print inside it: "93", "52", "2.4M", "128". */
export function formatCell(metric: MetricId | Unit, v: number | null): string {
  if (v === null) return '';
  switch (unitOf(metric)) {
    case 'share':
      return v > 0.995 && v < 1 ? '99+' : String(Math.round(v * 100));
    case 'age':
      return String(Math.round(v));
    case 'dollars':
      return moneyCompact(v).replace('$', '').replace('−', '-');
    case 'ratio':
      return String(Math.round(v * 100));
  }
}

// ---- solver -------------------------------------------------------------------------

export type GoalId = 'holds' | 'short' | 'fi_by';

export interface Goal {
  id: GoalId;
  metric: MetricId;
  target: number;
  /** True when the measured level meets the goal. A level that does not exist never does. */
  met(value: number | null): boolean;
}

export function makeGoal(id: GoalId, target: number): Goal {
  if (id === 'holds') return { id, metric: 'holds', target, met: (v) => v !== null && v >= target - 1e-9 };
  if (id === 'short') return { id, metric: 'short', target, met: (v) => v !== null && v <= target + 1e-9 };
  return { id, metric: 'fi_age', target, met: (v) => v !== null && v <= target + 1e-9 };
}

export interface Probe {
  /** 1-based order of testing. */
  order: number;
  x: number;
  level: Level;
  met: boolean;
}

export type SolveOutcome =
  /** The goal flips between `safe` (met) and `unsafe` (not met); the answer is `safe`. */
  | { status: 'converged'; safe: Probe; unsafe: Probe; tolerance: number; band: [number, number] | null }
  /** Met at both ends of the range (and so, by monotonicity, throughout). */
  | { status: 'always'; lo: Probe; hi: Probe }
  /** Met at neither end. */
  | { status: 'never'; lo: Probe; hi: Probe; closest: Probe };

/** At most this many simulations per search: two ends and eight halvings. */
export const MAX_PROBES = 10;

/**
 * The next step of a bisection for the boundary of a goal that is met on one side of
 * the range and not on the other (monotone by assumption). Returns the next value to
 * test, or the outcome when the search is over.
 */
export function nextProbe(probes: readonly Probe[], bounds: [number, number], tolerance: number, roundTo: (v: number) => number, goal: Goal): { x: number } | { outcome: SolveOutcome } {
  const [lo, hi] = bounds;
  if (probes.length === 0) return { x: lo };
  if (probes.length === 1) return { x: hi };
  const sorted = [...probes].sort((a, b) => a.x - b.x);
  const first = sorted[0] as Probe;
  const last = sorted[sorted.length - 1] as Probe;
  if (first.met === last.met) {
    if (first.met) return { outcome: { status: 'always', lo: first, hi: last } };
    const gap = (p: Probe) => (p.level.value === null ? Infinity : Math.abs(p.level.value - goal.target));
    return { outcome: { status: 'never', lo: first, hi: last, closest: gap(first) <= gap(last) ? first : last } };
  }
  // The bracket: the last probe from the left that agrees with the left end, and its right neighbour.
  let i = 0;
  while (i + 1 < sorted.length && (sorted[i + 1] as Probe).met === first.met) i++;
  const left = sorted[i] as Probe;
  const right = sorted[i + 1] as Probe;
  const mid = roundTo((left.x + right.x) / 2);
  if (right.x - left.x <= tolerance * 1.0001 || probes.length >= MAX_PROBES || mid <= left.x || mid >= right.x) {
    const safe = left.met ? left : right;
    const unsafe = left.met ? right : left;
    return { outcome: { status: 'converged', safe, unsafe, tolerance: Math.max(tolerance, right.x - left.x), band: samplingBand(sorted, goal.target) } };
  }
  return { x: mid };
}

/** Where a piecewise-linear curve through (x, y) points crosses `target`; every crossing, ascending. */
function crossings(points: readonly { x: number; y: number }[], target: number): number[] {
  const out: number[] = [];
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1] as { x: number; y: number };
    const b = points[i] as { x: number; y: number };
    if (a.y === target) out.push(a.x);
    else if (a.y < target !== b.y < target && b.y !== a.y) out.push(a.x + ((target - a.y) / (b.y - a.y)) * (b.x - a.x));
  }
  const end = points[points.length - 1];
  if (end && end.y === target) out.push(end.x);
  return out;
}

/**
 * The range of the variable over which the target lies inside the probes' 95% sampling
 * intervals: how far the answer could move on a different draw of lives. Null when the
 * interval curves never reach the target.
 */
export function samplingBand(sorted: readonly Probe[], target: number): [number, number] | null {
  const curve = (pick: (l: Level) => number | null) => sorted.flatMap((p) => (pick(p.level) === null ? [] : [{ x: p.x, y: pick(p.level) as number }]));
  const xs = [...crossings(curve((l) => l.lo), target), ...crossings(curve((l) => l.hi), target)];
  if (xs.length < 2) return null;
  return [Math.min(...xs), Math.max(...xs)];
}
