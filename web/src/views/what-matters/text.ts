// The words of the What matters view, computed from the paired results. Kept apart from
// the components so they can be tested and reused as plain text (image export, the
// assistant's context).
import { count, money } from '@/ui/format';
import { ofHundred } from '@/ui/charts/format';
import {
  METRICS,
  biggestRisk,
  topLever,
  formatDelta,
  formatLevel,
  formatMagnitude,
  gridPosition,
  rankLevers,
  type Effect,
  type Goal,
  type LeverDef,
  type Level,
  type MetricId,
  type SolveOutcome,
  type VarDef,
} from './model';

const lowerFirst = (s: string) => (/^(Social Security|Roth|HSA|IRA)/.test(s) ? s : s.charAt(0).toLowerCase() + s.slice(1));
const list = (items: string[]) => (items.length <= 1 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`);

/** A change in a share as lives: "6 more of 100", or "8 fewer of 1,000" when it is small. */
function livesChange(d: number): string {
  const a = Math.abs(d);
  const word = d > 0 ? 'more' : 'fewer';
  if (a >= 0.015) return `${Math.round(a * 100)} ${word} of 100`;
  return `${Math.max(1, Math.round(a * 1000))} ${word} of 1,000`;
}

/** What a paired change means, as a clause: "the plan holds in 6 more of 100 lives". */
export function changeClause(metric: MetricId, delta: number): string {
  switch (metric) {
    case 'holds':
      return `the plan holds in ${livesChange(delta)} lives`;
    case 'short':
      return `money runs short in ${livesChange(delta)} lives`;
    case 'fi_age':
      return `work becomes optional ${formatMagnitude(metric, delta)} ${delta < 0 ? 'sooner' : 'later'} in the middle life`;
    case 'wealth':
      return `the middle life ends with ${formatMagnitude(metric, delta)} ${delta > 0 ? 'more' : 'less'}`;
    case 'spending':
      return `the share of planned retirement spending delivered is ${formatMagnitude(metric, delta)} ${delta > 0 ? 'higher' : 'lower'}`;
  }
}

/** The 95% interval of a paired change in words: "5 to 8 of 100", "0.9 to 1.4 years", "−1.2 to +0.4 of 100". */
export function intervalWords(metric: MetricId, lo: number | null, hi: number | null): string {
  if (lo === null || hi === null) return '';
  const unit = METRICS[metric].unit;
  const tail = unit === 'share' ? ' of 100' : unit === 'ratio' ? ' points' : unit === 'age' ? ' years' : '';
  const bare = (v: number) => formatDelta(metric, v).replace(/ (yr|pts)$/, '');
  if (lo * hi > 0) {
    const [a, b] = Math.abs(lo) <= Math.abs(hi) ? [lo, hi] : [hi, lo];
    const [from, to] = [bare(a).replace(/^[+−]/, ''), bare(b).replace(/^[+−]/, '')];
    // Ages move in whole months, so the interval of a middle-life change can be narrower than the rounding.
    return from === to ? `${from}${tail}, to within rounding` : `${from} to ${to}${tail}`;
  }
  return `${bare(lo)} to ${bare(hi)}${tail}`;
}

/** One effect as a full statement for tooltips, the detail panel and screen readers. */
export function effectStatement(metric: MetricId, effect: Effect): string {
  if (effect.state === 'unknown' || effect.delta === null) return 'This measure does not exist for this setting (too few lives reach it both ways).';
  if (effect.state === 'none') return metric === 'fi_age' || metric === 'wealth' ? 'No change on this measure: the middle life is unaffected.' : 'No simulated life changes on this measure.';
  if (effect.state === 'noise') return `No change that can be told from chance: the paired difference is ${formatDelta(metric, effect.delta)}, with a 95% interval of ${intervalWords(metric, effect.lo, effect.hi)} that includes zero.`;
  const clause = changeClause(metric, effect.delta);
  return `${clause.charAt(0).toUpperCase()}${clause.slice(1)} (95% interval ${intervalWords(metric, effect.lo, effect.hi)}).`;
}

export interface Segment {
  text: string;
  strong?: boolean;
}

/** The view's lead sentence: the decision that moves the chosen measure most. */
export function leadSentence(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, metric: MetricId, paths: number): Segment[] {
  // Ranked as the tornado ranks: the largest paired change either way, so the lead and the chart's takeaway name the same lever.
  const top = topLever(levers, effects);
  if (!top || top.effect.delta === null) {
    return [{ text: `None of the decisions tested moves this measure by more than chance can explain at ${count(paths)} lives each.` }];
  }
  return [
    { text: 'Of the things you control, ' },
    { text: lowerFirst(top.lever.label), strong: true },
    { text: ` ${/s$/.test(top.lever.label) ? 'move' : 'moves'} this measure most: ` },
    { text: `${top.side.label.replace(/\s*\(.*\)\s*$/, '')}`, strong: true },
    { text: ', and ' },
    { text: changeClause(metric, top.effect.delta), strong: true },
    { text: '.' },
  ];
}

/** The assumption that could hurt most, as one sentence; null when none is distinguishable. */
export function riskSentence(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, metric: MetricId): string | null {
  const risk = biggestRisk(levers, effects, metric);
  if (!risk || risk.effect.delta === null) return null;
  return `Of the things you do not control, ${lowerFirst(risk.lever.label)} matter${/s$/.test(risk.lever.label) ? '' : 's'} most: ${risk.side.label}, and ${changeClause(metric, risk.effect.delta)} (${intervalWords(metric, risk.effect.lo, risk.effect.hi)}).`;
}

export function tornadoTakeaway(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, paths: number): string {
  const control = rankLevers(levers, effects, 'control');
  const world = rankLevers(levers, effects, 'world');
  const loud = (r: typeof control) => r.filter((x) => !x.quiet).map((x) => lowerFirst(x.lever.label));
  const c = loud(control);
  const w = loud(world);
  const quiet = [...control, ...world].filter((x) => x.quiet).length;
  const parts: string[] = [];
  if (c.length > 0) parts.push(`Among your decisions, ${list(c.slice(0, 2))} ${c.length === 1 ? 'moves' : 'move'} this measure most`);
  if (w.length > 0) parts.push(`${parts.length ? 'among' : 'Among'} the things you cannot choose, ${list(w.slice(0, 1))}`);
  const lead = parts.length ? `${parts.join('; ')}.` : 'No lever tested moves this measure by more than chance can explain.';
  const tail = quiet > 0 && parts.length ? ` ${quiet} of ${control.length + world.length} levers cannot be told from no effect at ${count(paths)} lives.` : '';
  return lead + tail;
}

// ---- trade-off map ------------------------------------------------------------------

/** Where a row of values first crosses `target`, in the axis's own unit (linear between nodes). */
export function crossingAlong(values: readonly (number | null)[], axis: readonly number[], target: number): number | null {
  for (let i = 1; i < values.length; i++) {
    const a = values[i - 1];
    const b = values[i];
    if (a === null || a === undefined || b === null || b === undefined || a === b) continue;
    if (a >= target !== b >= target) return (axis[i - 1] as number) + ((target - a) / (b - a)) * ((axis[i] as number) - (axis[i - 1] as number));
  }
  return null;
}

export interface MapReading {
  metric: MetricId;
  x: VarDef;
  y: VarDef;
  /** `values[yi][xi]` of the chosen measure. */
  values: (number | null)[][];
  /** Lives per cell of the sharpest complete pass. */
  paths: number;
}

export function mapTakeaway({ metric, x, y, values, paths }: MapReading): string {
  const flat = values.flat().filter((v): v is number => v !== null);
  if (flat.length === 0) return 'Simulating the first combinations.';
  const lo = Math.min(...flat);
  const hi = Math.max(...flat);
  const px = gridPosition(x.values, x.current);
  const py = gridPosition(y.values, y.current);
  const def = METRICS[metric];
  if (def.unit !== 'share' || px === null || py === null) {
    const where = lo === hi ? `is ${formatLevel(metric, lo)} everywhere on this map` : `runs from ${formatLevel(metric, lo)} to ${formatLevel(metric, hi)} across this map`;
    const subject = metric === 'fi_age' ? 'The age work becomes optional in the middle life' : metric === 'wealth' ? 'Net worth at the end in the middle life' : metric === 'spending' ? 'Retirement spending delivered' : metric === 'holds' ? 'The share of lives in which the plan holds' : 'The share of lives that run short';
    return `${subject} ${where}.${px === null || py === null ? ' The plan as entered does not sit on these axes, so it is not marked.' : ''}`;
  }
  const target = metric === 'holds' ? 0.9 : 0.05;
  const row = values[Math.round(py)] ?? [];
  const column = values.map((r) => r[Math.round(px)] ?? null);
  const here = row[Math.round(px)] ?? null;
  const line = `${ofHundred(target).text} lives`;
  const ax = crossingAlong(row, x.values, target);
  const ay = crossingAlong(column, y.values, target);
  const routes = [ax !== null ? x.phrase(x.round(ax)) : null, ay !== null ? y.phrase(y.round(ay)) : null].filter((s): s is string => s !== null);
  const meets = here !== null && (metric === 'holds' ? here >= target : here <= target);
  const now = here === null ? 'The plan as entered is not on a measured cell.' : metric === 'holds' ? `The plan as entered holds in ${ofHundred(here).text} lives.` : `Under the plan as entered money runs short in ${ofHundred(here).text} lives.`;
  // Within the sampling error of one cell the plan cannot be said to be on either side of the line.
  if (here !== null && paths > 0 && Math.abs(here - target) <= 1.96 * Math.sqrt((target * (1 - target)) / paths)) {
    return `${now} That is on the ${line.replace(' lives', '')} line to within the sampling error of a cell, so small moves along either axis cross it.`;
  }
  if (routes.length === 0) {
    if (meets) return `${now} It stays ${metric === 'holds' ? 'at or above' : 'at or below'} ${line} along both axes of this map.`;
    return `${now} Moving along either axis alone does not reach ${line}; the map shows the combinations that do.`;
  }
  if (meets) return `${now} It would ${metric === 'holds' ? 'fall below' : 'rise above'} ${line} at about ${routes.join(', or ')}, the rest as planned.`;
  return `${now} It would reach ${line} by ${routes.join(', or by ')}, the rest as planned.`;
}

// ---- solver -------------------------------------------------------------------------

export function goalClause(goal: Goal): string {
  if (goal.id === 'holds') return `the plan holds in at least ${ofHundred(goal.target).text} lives`;
  if (goal.id === 'short') return `money runs short in at most ${ofHundred(goal.target).text} lives`;
  return `work becomes optional by ${formatLevel('fi_age', goal.target)} in at least half of lives`;
}

const SOLVER_NOUN: Partial<Record<VarDef['id'], { noun: string; verb: string }>> = {
  spending: { noun: 'the annual budget', verb: 'is' },
  withdrawal_rate: { noun: 'the withdrawal rate that triggers retirement', verb: 'is' },
  retirement_age: { noun: 'the retirement age', verb: 'is' },
  salary: { noun: 'salaries', verb: 'are' },
};

/** A solver value in a sentence, to the search's own resolution: "$46,300 a year", "3.25%", "57", "88% of today's". */
export function solverValue(variable: VarDef, x: number, bare = false): string {
  if (variable.id === 'salary') return `${Math.round(x * 100)}% of today’s`;
  if (variable.id === 'spending') return `${money(Math.round(x / 100) * 100)}${bare ? '' : ' a year'}`;
  return variable.format(x);
}

/** A probe's level precisely enough that the two sides of a boundary read differently: shares as counts of the lives simulated. */
function levelWords(metric: MetricId, level: Level, paths: number): string {
  if (level.value === null) return 'undefined (too few lives get there)';
  if (METRICS[metric].unit === 'share') return `${count(Math.round(level.value * paths))} of ${count(paths)} lives`;
  return formatLevel(metric, level.value);
}

export function solverSentence(goal: Goal, variable: VarDef, outcome: SolveOutcome, paths: number): { answer: string; detail: string } {
  const { noun, verb } = SOLVER_NOUN[variable.id] ?? { noun: variable.label.toLowerCase(), verb: 'is' };
  const clause = goalClause(goal);
  const Clause = clause.charAt(0).toUpperCase() + clause.slice(1);
  const [lo, hi] = variable.bounds ?? [0, 0];
  if (outcome.status === 'always') {
    return { answer: `${Clause} everywhere between ${solverValue(variable, lo)} and ${solverValue(variable, hi)}.`, detail: `At the two ends the measure is ${levelWords(goal.metric, outcome.lo.level, paths)} and ${levelWords(goal.metric, outcome.hi.level, paths)}, so there is no boundary to find in this range.` };
  }
  if (outcome.status === 'never') {
    return {
      answer: `No value of ${noun} between ${solverValue(variable, lo)} and ${solverValue(variable, hi)} gets there.`,
      detail: `The closest is ${solverValue(variable, outcome.closest.x)}, where the measure is ${levelWords(goal.metric, outcome.closest.level, paths)}. Try another variable, or a gentler goal.`,
    };
  }
  const { safe, unsafe, band } = outcome;
  const side = safe.x < unsafe.x ? 'at most' : 'at least';
  const wide = band !== null && band[1] - band[0] > outcome.tolerance;
  const width = solverValue(variable, variable.round(Math.abs(unsafe.x - safe.x)), true);
  // The tolerance belongs in the answer: the search's own resolution, or the sampling range when that is wider.
  const tolerance = wide ? `; on a different draw of lives the boundary could fall anywhere from ${solverValue(variable, variable.round(band[0]), true)} to ${solverValue(variable, variable.round(band[1]), true)}` : `, to within ${width}`;
  return {
    answer: `${Clause} when ${noun} ${verb} ${side} ${solverValue(variable, safe.x)}${tolerance}.`,
    detail: `At ${solverValue(variable, safe.x)} the measure is ${levelWords(goal.metric, safe.level, paths)}; at ${solverValue(variable, unsafe.x)} it is ${levelWords(goal.metric, unsafe.level, paths)}. The search narrowed the boundary to within ${width}, assuming the measure moves in one direction as ${noun} ${verb === 'are' ? 'grow' : 'grows'}; the sampling range comes from the 95% intervals of the ${count(paths)}-life runs.`,
  };
}
