// The words on the Outcomes view, computed from the Summary: one sentence per chart.
import type { Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { age, ofHundred } from '@/ui/charts';
import type { Segment } from '../overview/text';
import { contrasts, distribution, formatUnit, lastingModel, type CohortYear, type Contrast, type LuckSplit } from './data';

const pct = (v: number, digits = 0) => `${(v * 100).toFixed(digits).replace(/^-(0(\.0+)?)$/, '$1').replace('-', '−')}%`;
const years = (n: number) => `${Math.round(n)} ${Math.round(n) === 1 ? 'year' : 'years'}`;

/** "83 of 100", or of the lives actually simulated when there are fewer than 100. */
export function livesOf(share: number, paths: number): string {
  return paths < 100 ? `${Math.round(share * paths)} of ${paths}` : ofHundred(share).text;
}

/** The view's opening sentence: how wide the range is, and what most separates its ends. */
export function rangeSentence(summary: Summary): Segment[] {
  const w = summary.headline.terminal_net_worth;
  const out: Segment[] = [];
  if (w.p10 !== null && w.p50 !== null && w.p90 !== null) {
    out.push({ text: 'The middle life ends with ' }, { text: moneyCompact(w.p50), strong: true }, { text: '. One in ten ends below ' }, { text: moneyCompact(w.p10), strong: true }, { text: ' and one in ten above ' }, { text: moneyCompact(w.p90), strong: true }, { text: '.' });
  }
  const failed = summary.failure.failed / Math.max(1, summary.paths);
  if (summary.failure.failed === 0) {
    out.push({ text: ' No simulated life falls short of the plan.' });
    return out;
  }
  out.push({ text: ' ' }, { text: livesOf(failed, summary.paths), strong: true }, { text: ' fall short of the plan' });
  const top = contrasts(summary)[0];
  if (top && top.separation >= 0.5) out.push({ text: ', and what most sets those lives apart is ' }, { text: top.direction, strong: true });
  out.push({ text: '.' });
  return out;
}

export function agesTakeaway(summary: Summary, same: boolean): string {
  const fi = summary.headline.fi_age;
  const ret = summary.headline.retirement_age;
  if (fi.p50 === null && ret.p50 === null) return 'In most simulated lives work never becomes optional and retirement never comes.';
  const spread = (s: typeof fi) => (s.p10 !== null && s.p90 !== null ? `, and between ${age(s.p10)} and ${age(s.p90)} in 80 of 100` : '');
  const never = summary.retirement.never >= 0.005 ? ` In ${ofHundred(summary.retirement.never).text} lives retirement never comes.` : '';
  if (same) return `Work becomes optional at ${age(fi.p50 ?? 0)} in the middle life${spread(fi)}, and this plan retires the moment it does.${never}`;
  if (fi.p50 === null) return `The middle life retires at ${age(ret.p50 ?? 0)}${spread(ret)}, although in most lives savings never meet the funded test.${never}`;
  if (ret.p50 === null) return `Work becomes optional at ${age(fi.p50)} in the middle life${spread(fi)}, but fewer than half of lives actually retire.${never}`;
  const gap = ret.p50 - fi.p50;
  const relation = Math.abs(gap) < 0.25 ? 'and the middle life retires at once, though in other lives the plan’s own rules hold the two ages apart' : gap > 0 ? `but retirement comes at ${age(ret.p50)}, ${years(gap)} later` : `yet the plan retires at ${age(ret.p50)}, ${years(-gap)} before the savings are there`;
  return `Work becomes optional at ${age(fi.p50)} in the middle life${spread(fi)}, ${relation}.${never}`;
}

export function wealthTakeaway(summary: Summary): string {
  const w = summary.headline.terminal_net_worth;
  if (w.p10 === null || w.p50 === null || w.p90 === null) return 'Net worth at the end of each simulated life.';
  const ratio = w.p10 > 0 ? w.p90 / w.p10 : null;
  return `Half of lives end with more than ${moneyCompact(w.p50)}. One in ten ends with less than ${moneyCompact(w.p10)} and one in ten with more than ${moneyCompact(w.p90)}${ratio !== null && ratio >= 3 ? `, ${Math.round(ratio)} times as much` : ''}.`;
}

/** Share of the dots at or above `threshold` of plan in their worst year. */
export function cutTakeaway(summary: Summary): string {
  const sp = summary.spending;
  const dist = distribution(summary, 'worst_spending_ratio');
  if (sp.retired_paths === 0 || !dist || dist.count === 0) return 'No simulated life reaches retirement, so there is no retirement spending to cut.';
  const onPlan = dist.dots.filter((d) => d !== null && d >= 0.95).length / Math.max(1, dist.dots.length);
  const deep = sp.worst_cut[sp.worst_cut_levels.indexOf(0.9)];
  if (onPlan >= 0.995) return 'In every retired life spending stays within 5% of the plan in every year.';
  const tail = deep !== null && deep !== undefined && deep >= 0.1 ? ` In the hardest-hit one in ten, the worst year is a cut of ${pct(deep)} or more.` : '';
  return `In ${ofHundred(onPlan).text} retired lives spending never falls more than 5% below the plan.${tail}`;
}

export function taxTakeaway(summary: Summary): string {
  const t = summary.headline.lifetime_tax;
  if (t.p10 === null || t.p50 === null || t.p90 === null) return 'Tax paid over each simulated life.';
  const skew = t.p90 / t.p50 >= 2.5 ? ' The long tail is the lives whose investments do best: they pay the most.' : '';
  return `The middle life pays ${moneyCompact(t.p50)} in tax over its lifetime; one in ten pays less than ${moneyCompact(t.p10)} and one in ten more than ${moneyCompact(t.p90)}.${skew}`;
}

export function shortYearsTakeaway(summary: Summary): string {
  const rate = summary.headline.depletion_rate.value;
  const depth = summary.headline.years_depleted;
  if (rate <= 0) return 'No simulated life is ever short of essentials.';
  const typical = depth.p50 !== null ? ` Those that do are short for ${years(depth.p50)} in the middle case${depth.p90 !== null && depth.p90 > depth.p50 ? `, and ${years(depth.p90)} or more in one in ten of them` : ''}.` : '';
  return `${ofHundred(1 - rate).text} lives are never short of essentials.${typical}`;
}

export function lastingTakeaway(summary: Summary): string {
  const m = lastingModel(summary);
  const ages = summary.axis.ages;
  if (ages.length === 0) return '';
  const peak = Math.max(0, ...m.short);
  const peakAt = m.short.indexOf(peak);
  // Read it at 90 when the plan runs that far, else near the end.
  let at = ages.findIndex((a) => a >= 90);
  if (at < 0) at = Math.max(0, ages.length - 2);
  const a = Math.floor(ages[at] as number);
  const lead = `At ${a}, someone is still alive in ${ofHundred(m.alive[at] ?? 0).text} lives, and the money has lasted in ${ofHundred(m.lasted[at] ?? 1).text}.`;
  if (peak < 0.0005) return `${lead} No life is both alive and short of essentials at any age.`;
  const among = m.shortAmongLiving[peakAt];
  return `${lead} Being alive and short of essentials is most common around ${Math.floor(ages[peakAt] as number)}: ${ofHundred(peak).text} lives${among !== null && among !== undefined && among - peak >= 0.01 ? `, or ${ofHundred(among).text} of those still living` : ''}.`;
}

export function luckTakeaway(split: LuckSplit | null, lives: number): string {
  if (!split) return lives === 0 ? 'No simulated life has retired yet, so there are no early retirement returns to compare.' : 'Too few retired lives so far to see a pattern.';
  const u = split.unlucky;
  const r = split.rest;
  const wealth = u.medianWealth !== null && r.medianWealth !== null ? ` The middle unlucky life ends with ${moneyCompact(u.medianWealth)}, against ${moneyCompact(r.medianWealth)}.` : '';
  if (u.shortShare === 0 && r.shortShare === 0) return `Even the unluckiest fifth of lives, whose first ten retirement years return less than ${pct(split.threshold, 1)} a year after inflation, never run short.${wealth}`;
  return `When the first ten years of retirement return less than ${pct(split.threshold, 1)} a year after inflation (the unluckiest fifth), ${ofHundred(u.shortShare).text} lives run short. Among the rest, ${ofHundred(r.shortShare).n} of ${ofHundred(r.shortShare).of} do.${wealth}`;
}

export function contrastValue(c: Contrast, value: number): string {
  if (c.scalar === 'worst_drawdown') return pct(value);
  if (c.scalar === 'death_age_0' || c.scalar === 'retirement_age') return age(value);
  return formatUnit(value, c.unit, moneyCompact);
}

export function contrastTakeaway(summary: Summary, rows: Contrast[]): string {
  if (summary.failure.failed === 0) return 'No simulated life falls short, so there is nothing to compare.';
  const top = rows[0];
  if (!top) return 'Too few lives fall short to say what they have in common.';
  const second = rows[1];
  const least = rows.length > 3 ? rows[rows.length - 1] : undefined;
  const lead = `The lives that fall short stand out most for ${top.direction}: a median of ${contrastValue(top, top.failed)}, against ${contrastValue(top, top.succeeded)} for the lives that hold`;
  const next = second && second.separation >= 0.25 ? `, then for ${second.direction}` : '';
  const last = least && least.separation < 0.15 ? ` ${least.label} barely differs.` : '';
  return `${lead}${next}.${last}`;
}

export function contrastSentence(c: Contrast): string {
  if (c.tail) {
    const side = c.tail.above ? 'top' : 'bottom';
    if (Math.abs(c.tail.failed - c.tail.succeeded) < 0.03) return `About ${ofHundred(c.tail.failed).text} of each group sit in the ${side} quarter of all lives on this: it does not tell the two apart.`;
    return `${ofHundred(c.tail.failed).text} lives that fell short were in the ${side} quarter of all lives on this, against ${ofHundred(c.tail.succeeded).text} that held.`;
  }
  return c.separation < 0.15 ? 'The two groups are almost indistinguishable on this.' : `Lives that fell short had ${c.direction}.`;
}

export function cohortTakeaway(rows: CohortYear[]): string {
  const usable = rows.filter((r) => r.shortShare !== null && r.lives >= 4);
  if (usable.length === 0) return 'Too few lives per starting year to compare them yet.';
  const clean = usable.filter((r) => r.short === 0).length;
  const worst = [...usable].sort((a, b) => (b.shortShare as number) - (a.shortShare as number))[0] as CohortYear;
  if ((worst.shortShare as number) === 0) return 'Whichever year of the record a life starts in, the money lasts.';
  // The run of consecutive bad starting years around the worst one.
  const bad = (r: CohortYear | undefined) => r !== undefined && r.shortShare !== null && r.shortShare >= Math.max(0.25, (worst.shortShare as number) / 2);
  let from = usable.indexOf(worst);
  let to = from;
  while (bad(usable[from - 1]) && (usable[from - 1] as CohortYear).year === (usable[from] as CohortYear).year - 1) from--;
  while (bad(usable[to + 1]) && (usable[to + 1] as CohortYear).year === (usable[to] as CohortYear).year + 1) to++;
  const span = from === to ? `in ${worst.year}` : `between ${(usable[from] as CohortYear).year} and ${(usable[to] as CohortYear).year}`;
  return `Lives that start ${span} fare worst: of those starting in ${worst.year}, ${worst.short} of ${worst.lives} run short. In ${clean} of ${usable.length} starting years, none do.`;
}

export interface ModelResult {
  kind: string;
  label: string;
  success: { value: number; lo: number; hi: number };
}

export function triangulationTakeaway(results: ModelResult[], current: string): string {
  if (results.length < 2) return '';
  const values = results.map((r) => r.success.value);
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const mine = results.find((r) => r.kind === current);
  const sampling = Math.max(...results.map((r) => (r.success.hi - r.success.lo) / 2));
  const lowest = results.find((r) => r.success.value === lo) as ModelResult;
  const highest = results.find((r) => r.success.value === hi) as ModelResult;
  const spread = Math.round(hi * 100) - Math.round(lo * 100);
  if (spread <= Math.max(1, Math.round(sampling * 200))) return `The answer barely depends on the market model: the plan holds in ${ofHundred(lo).n} to ${ofHundred(hi).text} lives under all four, within sampling error of each other.`;
  const where = mine ? (mine.kind === lowest.kind ? ' The plan as entered uses the most cautious of them.' : mine.kind === highest.kind ? ' The plan as entered uses the most generous of them.' : '') : '';
  return `The plan holds in ${ofHundred(lo).n} of 100 lives under ${lowest.label.toLowerCase()} and ${ofHundred(hi).n} under ${highest.label.toLowerCase()}: the choice of market model moves the answer by ${spread} lives in 100, against about ±${Math.max(1, Math.round(sampling * 100))} from sampling.${where}`;
}
